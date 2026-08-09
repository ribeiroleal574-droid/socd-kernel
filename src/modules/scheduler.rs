extern crate alloc;
// ============================================================
// SOC-D Kernel — Scheduler Preemptivo
// ============================================================
//
// O scheduler gerencia a execução de processos/tarefas no kernel.
// Na Fase 1 implementamos:
//   - Estrutura de Process Control Block (PCB)
//   - Filas de prioridade (Critical, High, Normal, Low)
//   - Algoritmo Round-Robin com quantum de tempo
//   - Troca de contexto (salva/restaura registradores)
//   - Estados de processo: Ready, Running, Blocked, Sleeping, Dead
//
// Na Fase 2 (com IA):
//   - O scheduler receberá hints do motor de IA
//   - Previsão de uso de CPU por processo
//   - Prioridade dinâmica baseada em comportamento
//   - Energy-aware scheduling (dispositivos móveis)
//
// Integração com interrupções:
//   - O timer IRQ0 chama scheduler::tick() a cada ~1ms
//   - tick() decrementa o quantum do processo atual
//   - Quando quantum = 0, preempção ocorre
// ============================================================

use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use spinning_top::Spinlock;

// ─── Identificadores ──────────────────────────────────────────────────────────

/// ID único de processo (incrementado atomicamente)
pub type Pid = u64;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

fn alloc_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

// ─── Quantum de tempo ────────────────────────────────────────────────────────

/// Quantum padrão em ticks de timer (~10ms a 100Hz)
const DEFAULT_QUANTUM: u32 = 10;

/// Quantum para processos de alta prioridade (~5ms)
const HIGH_PRIORITY_QUANTUM: u32 = 5;

/// Quantum mínimo (~2ms) — para processos interativos urgentes
const MIN_QUANTUM: u32 = 2;

// ─── Estado do Processo ───────────────────────────────────────────────────────

/// Estado atual de um processo no scheduler
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    /// Pronto para executar — na fila de ready
    Ready,
    /// Executando agora na CPU
    Running,
    /// Bloqueado aguardando I/O ou evento
    Blocked { reason: BlockReason },
    /// Dormindo por N ticks
    Sleeping { wake_at_tick: u64 },
    /// Execução concluída, aguardando coleta
    Dead { exit_code: i32 },
    /// Criado mas ainda não iniciado
    New,
}

/// Razão do bloqueio de um processo
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// Aguardando leitura de dispositivo
    WaitingIO,
    /// Aguardando mutex/semáforo
    WaitingLock,
    /// Aguardando sinal de outro processo
    WaitingSignal,
    /// Aguardando término de processo filho
    WaitingChild(Pid),
}

// ─── Prioridade ───────────────────────────────────────────────────────────────

/// Prioridade de escalonamento
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Processos do kernel (IRQ handlers, drivers críticos)
    Critical = 0,
    /// Serviços do sistema (security, p2p, ia-engine)
    High = 1,
    /// Apps do usuário
    Normal = 2,
    /// Background (sync, indexação, backup)
    Low = 3,
    /// Idle — só roda quando nada mais pode
    Idle = 4,
}

impl Priority {
    fn quantum(&self) -> u32 {
        match self {
            Priority::Critical => MIN_QUANTUM,
            Priority::High     => HIGH_PRIORITY_QUANTUM,
            Priority::Normal   => DEFAULT_QUANTUM,
            Priority::Low      => DEFAULT_QUANTUM * 2,
            Priority::Idle     => u32::MAX,
        }
    }
}

// ─── Contexto de CPU (registradores salvos) ──────────────────────────────────

/// Contexto completo de registradores de um processo.
/// Salvo quando o processo é preemptado, restaurado quando retoma.
///
/// Em x86_64, a troca de contexto envolve:
/// - Registradores de uso geral (rax..r15)
/// - Ponteiro de instrução (rip) — via stack de interrupção
/// - Ponteiro de stack (rsp)
/// - Flags de status (rflags)
/// - Segmentos de dados (cs, ss, ds, es)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CpuContext {
    // Registradores de uso geral (salvos em ordem de push)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    // Registradores de controle de fluxo
    pub rip:    u64,  // Próxima instrução a executar (informativo — ver kernel_rsp)
    pub cs:     u64,  // Code segment
    pub rflags: u64,  // Status flags (interrupts, zero, carry, etc.)
    pub rsp:    u64,  // Stack pointer (informativo — ver kernel_rsp)
    pub ss:     u64,  // Stack segment

    /// RSP real usado pela troca de contexto (`arch::context_switch`).
    /// Ao contrário dos campos acima (mantidos por razões informativas
    /// e de compatibilidade), este é o valor lido/escrito a cada troca
    /// de tarefa — aponta para a pilha da tarefa, construída por
    /// `arch::context_switch::prepare_initial_stack` na criação, e
    /// actualizado a cada `switch_context`.
    pub kernel_rsp: u64,
}

impl CpuContext {
    /// Cria um contexto inicial para uma nova tarefa de kernel.
    ///
    /// `stack_top` deve ser o topo de uma pilha exclusiva desta tarefa
    /// (ver `ProcessControlBlock::new_kernel_task`), usada para montar
    /// o frame inicial que o trampolim de arranque vai desempilhar.
    pub fn new_kernel_task(entry_point: u64, stack_top: u64) -> Self {
        let kernel_rsp = if stack_top == 0 {
            // stack_top == 0 é usado por `register_current_as_task`,
            // que regista uma execução já em curso (sem pilha própria
            // pré-construída) — o valor real é preenchido na primeira
            // troca de saída dessa tarefa.
            0
        } else {
            unsafe { crate::arch::context_switch::prepare_initial_stack(stack_top, entry_point) }
        };

        Self {
            rip: entry_point,
            rsp: stack_top,
            // Ring 0: cs = 0x08 (kernel code), ss = 0x10 (kernel data)
            cs: 0x08,
            ss: 0x10,
            // IF=1 (interrupções habilitadas), IOPL=0, reserved=1
            rflags: 0x200,
            kernel_rsp,
            // Registos de uso geral: 0 (só relevantes como snapshot
            // informativo — a troca real usa apenas `kernel_rsp`).
            ..Default::default()
        }
    }
}

// ─── Process Control Block (PCB) ─────────────────────────────────────────────

/// Process Control Block — toda a informação sobre um processo.
/// É a estrutura central do scheduler.
#[derive(Debug, Clone)]
pub struct ProcessControlBlock {
    /// ID único do processo
    pub pid: Pid,
    /// ID do processo pai (0 = kernel)
    pub parent_pid: Pid,
    /// Nome legível do processo
    pub name: String,
    /// Estado atual
    pub state: ProcessState,
    /// Prioridade de escalonamento
    pub priority: Priority,
    /// Contexto de CPU (registradores salvos)
    pub context: CpuContext,
    /// Ticks de CPU consumidos no total
    pub cpu_ticks_total: u64,
    /// Ticks restantes no quantum atual
    pub quantum_remaining: u32,
    /// Tick em que o processo foi criado
    pub created_at_tick: u64,
    /// Stack do processo (alocada no heap do kernel)
    pub stack: Vec<u8>,
    /// ID do sandbox de segurança
    pub sandbox_pid: crate::security::sandbox::ProcessId,
    /// Estatísticas de uso (para a IA na Fase 2)
    pub stats: ProcessStats,
}

/// Estatísticas de uso de um processo
#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    /// Total de vezes que foi escalonado
    pub schedule_count: u64,
    /// Total de vezes que foi preemptado
    pub preempt_count: u64,
    /// Total de tempo bloqueado (em ticks)
    pub blocked_ticks: u64,
    /// Maior tempo contínuo de CPU (em ticks)
    pub max_continuous_ticks: u32,
    /// Uso médio de CPU (estimativa, 0–100%)
    pub avg_cpu_usage: u8,
}

impl ProcessControlBlock {
    /// Cria um novo PCB para uma tarefa do kernel
    pub fn new_kernel_task(
        name: &str,
        entry: u64,
        priority: Priority,
        current_tick: u64,
    ) -> Self {
        // Aloca stack de 64 KB para a tarefa
        const STACK_SIZE: usize = 64 * 1024;
        let stack = alloc::vec![0u8; STACK_SIZE];

        // Stack pointer aponta para o topo (stacks crescem para baixo)
        let stack_top = stack.as_ptr() as u64 + STACK_SIZE as u64;

        // Cria sandbox para este processo
        let sandbox_pid = crate::security::sandbox::create_process_sandbox(
            name,
            crate::security::TrustLevel::System,
        );

        let quantum = priority.quantum();

        Self {
            pid: alloc_pid(),
            parent_pid: 0,
            name: name.to_string(),
            state: ProcessState::New,
            priority,
            context: CpuContext::new_kernel_task(entry, stack_top),
            cpu_ticks_total: 0,
            quantum_remaining: quantum,
            created_at_tick: current_tick,
            stack,
            sandbox_pid,
            stats: ProcessStats::default(),
        }
    }

    /// Verifica se o processo pode ser escalonado agora
    pub fn is_runnable(&self) -> bool {
        matches!(self.state, ProcessState::Ready | ProcessState::New)
    }

    /// Reseta o quantum para o valor padrão da prioridade
    pub fn reset_quantum(&mut self) {
        self.quantum_remaining = self.priority.quantum();
    }
}

// ─── Scheduler ───────────────────────────────────────────────────────────────

/// Filas de processos por prioridade
struct ReadyQueues {
    critical: VecDeque<Pid>,
    high:     VecDeque<Pid>,
    normal:   VecDeque<Pid>,
    low:      VecDeque<Pid>,
    idle:     VecDeque<Pid>,
}

impl ReadyQueues {
    const fn new() -> Self {
        Self {
            critical: VecDeque::new(),
            high:     VecDeque::new(),
            normal:   VecDeque::new(),
            low:      VecDeque::new(),
            idle:     VecDeque::new(),
        }
    }

    /// Adiciona um processo na fila correta
    fn enqueue(&mut self, pid: Pid, priority: Priority) {
        let queue = match priority {
            Priority::Critical => &mut self.critical,
            Priority::High     => &mut self.high,
            Priority::Normal   => &mut self.normal,
            Priority::Low      => &mut self.low,
            Priority::Idle     => &mut self.idle,
        };
        if !queue.contains(&pid) {
            queue.push_back(pid);
        }
    }

    /// Retira o próximo processo a executar (maior prioridade primeiro)
    fn dequeue(&mut self) -> Option<Pid> {
        if let Some(pid) = self.critical.pop_front() { return Some(pid); }
        if let Some(pid) = self.high.pop_front()     { return Some(pid); }
        if let Some(pid) = self.normal.pop_front()   { return Some(pid); }
        if let Some(pid) = self.low.pop_front()      { return Some(pid); }
        if let Some(pid) = self.idle.pop_front()     { return Some(pid); }
        None
    }

    /// Total de processos prontos
    fn total(&self) -> usize {
        self.critical.len() + self.high.len() +
        self.normal.len()   + self.low.len()  +
        self.idle.len()
    }
}

/// O Scheduler do SOC-D
pub struct Scheduler {
    /// Todos os processos conhecidos
    processes: Vec<ProcessControlBlock>,
    /// Filas de processos prontos por prioridade
    ready_queues: ReadyQueues,
    /// PID do processo atualmente em execução
    current_pid: Option<Pid>,
    /// Tick global do sistema
    tick: u64,
    /// Total de trocas de contexto realizadas
    context_switches: u64,
    /// Scheduler inicializado?
    initialized: bool,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            processes: Vec::new(),
            ready_queues: ReadyQueues::new(),
            current_pid: None,
            tick: 0,
            context_switches: 0,
            initialized: false,
        }
    }

    /// Inicializa o scheduler e cria o processo idle
    pub fn init(&mut self) {
        // Processo idle: roda quando nada mais pode rodar
        // (mantém a CPU em hlt loop economizando energia)
        let idle = ProcessControlBlock::new_kernel_task(
            "idle",
            idle_task as *const () as u64,
            Priority::Idle,
            self.tick,
        );
        let idle_pid = idle.pid;
        self.processes.push(idle);

        // Coloca idle na fila imediatamente
        self.ready_queues.enqueue(idle_pid, Priority::Idle);
        self.initialized = true;

        crate::serial_println!("[SCHED] Scheduler inicializado. Idle PID: {}", idle_pid);
    }

    /// Cria e registra uma nova tarefa
    pub fn spawn(
        &mut self,
        name: &str,
        entry: u64,
        priority: Priority,
    ) -> Pid {
        let pcb = ProcessControlBlock::new_kernel_task(name, entry, priority, self.tick);
        let pid = pcb.pid;
        let prio = pcb.priority;

        self.processes.push(pcb);
        self.ready_queues.enqueue(pid, prio);

        // Atualiza estado para Ready
        if let Some(p) = self.get_process_mut(pid) {
            p.state = ProcessState::Ready;
        }

        crate::serial_println!("[SCHED] Tarefa criada: '{}' PID={} prio={:?}", name, pid, priority);
        pid
    }

    /// Chamado pelo handler do timer a cada tick (~1ms a 1000Hz).
    /// Verifica se o processo atual deve ser preemptado.
    /// Retorna true se uma troca de contexto é necessária.
    pub fn tick(&mut self) -> bool {
        self.tick += 1;

        // Acorda processos que estavam dormindo
        self.wake_sleeping_processes();

        // Decrementa quantum do processo atual
        if let Some(pid) = self.current_pid {
            if let Some(proc) = self.get_process_mut(pid) {
                proc.cpu_ticks_total += 1;

                if proc.quantum_remaining > 0 {
                    proc.quantum_remaining -= 1;
                }

                // Quantum expirou? Preempção!
                if proc.quantum_remaining == 0 {
                    return true; // Solicita troca de contexto
                }
            }
            false
        } else {
            // Sem tarefa actual (ex: a última tarefa acabou de terminar
            // via exit_process, que limpa current_pid). Sem isto, o
            // scheduler nunca mais chamava `schedule()` — ficava preso
            // em hlt() para sempre, porque este `if let` nunca voltava
            // a entrar aqui. Há sempre trabalho a considerar (nem que
            // seja só o idle), por isso pedimos sempre uma troca.
            true
        }
    }

    /// Seleciona o próximo processo a executar (Round-Robin com prioridade).
    /// Retorna o PID do próximo processo, se houver.
    pub fn schedule(&mut self) -> Option<Pid> {
        // Coloca o processo atual de volta na fila — só se ainda
        // estiver `Running` (ou seja, não foi entretanto bloqueado,
        // posto a dormir, ou terminado por outro caminho enquanto era
        // a tarefa corrente).
        //
        // NOTA (bug corrigido): isto usava `proc.is_runnable()`, que só
        // aceita os estados Ready|New — mas a tarefa corrente está
        // sempre em `Running` neste ponto, nunca Ready. Com a condição
        // antiga, nenhuma tarefa alguma vez voltava à fila depois de
        // ser escalonada pela primeira vez: desaparecia do round-robin
        // para sempre. `Running` é a verificação correcta aqui.
        if let Some(current) = self.current_pid {
            if let Some(proc) = self.get_process_mut(current) {
                if matches!(proc.state, ProcessState::Running) {
                    proc.state = ProcessState::Ready;
                    proc.reset_quantum();
                    proc.stats.preempt_count += 1;
                    let prio = proc.priority;
                    self.ready_queues.enqueue(current, prio);
                }
            }
        }

        // Seleciona próximo da fila de maior prioridade
        let next_pid = self.ready_queues.dequeue()?;

        // Atualiza estado do novo processo
        if let Some(proc) = self.get_process_mut(next_pid) {
            proc.state = ProcessState::Running;
            proc.stats.schedule_count += 1;
        }

        self.current_pid = Some(next_pid);
        self.context_switches += 1;

        Some(next_pid)
    }

    /// Bloqueia o processo atual por uma razão específica
    pub fn block_current(&mut self, reason: BlockReason) {
        if let Some(pid) = self.current_pid {
            if let Some(proc) = self.get_process_mut(pid) {
                proc.state = ProcessState::Blocked { reason };
                proc.stats.blocked_ticks += 1;
            }
        }
    }

    /// Desbloqueia um processo (I/O concluído, lock liberado, etc.)
    pub fn unblock(&mut self, pid: Pid) {
        if let Some(proc) = self.get_process_mut(pid) {
            if matches!(proc.state, ProcessState::Blocked { .. }) {
                proc.state = ProcessState::Ready;
                let prio = proc.priority;
                self.ready_queues.enqueue(pid, prio);
            }
        }
    }

    /// Coloca o processo atual para dormir por N ticks
    pub fn sleep_current(&mut self, ticks: u64) {
        if let Some(pid) = self.current_pid {
            let wake_at = self.tick + ticks;
            if let Some(proc) = self.get_process_mut(pid) {
                proc.state = ProcessState::Sleeping {
                    wake_at_tick: wake_at,
                };
            }
        }
    }

    /// Acorda processos cujo tempo de sleep expirou
    fn wake_sleeping_processes(&mut self) {
        let current_tick = self.tick;
        for proc in &mut self.processes {
            if let ProcessState::Sleeping { wake_at_tick } = proc.state {
                if current_tick >= wake_at_tick {
                    proc.state = ProcessState::Ready;
                    // Será colocado na fila no próximo schedule()
                }
            }
        }
        // Re-enfileira processos que acordaram
        let to_enqueue: Vec<(Pid, Priority)> = self.processes.iter()
            .filter(|p| p.state == ProcessState::Ready && !self.ready_queues.critical.contains(&p.pid))
            .map(|p| (p.pid, p.priority))
            .collect();
        for (pid, prio) in to_enqueue {
            self.ready_queues.enqueue(pid, prio);
        }
    }

    /// Termina um processo
    pub fn exit_process(&mut self, pid: Pid, exit_code: i32) {
        if let Some(proc) = self.get_process_mut(pid) {
            proc.state = ProcessState::Dead { exit_code };
            crate::serial_println!("[SCHED] Processo PID={} terminou (exit={})", pid, exit_code);
        }
        if self.current_pid == Some(pid) {
            self.current_pid = None;
        }
    }

    /// Retorna referência a um processo pelo PID
    pub fn get_process(&self, pid: Pid) -> Option<&ProcessControlBlock> {
        self.processes.iter().find(|p| p.pid == pid)
    }

    /// Retorna referência mutável a um processo pelo PID
    fn get_process_mut(&mut self, pid: Pid) -> Option<&mut ProcessControlBlock> {
        self.processes.iter_mut().find(|p| p.pid == pid)
    }

    /// Retorna estatísticas globais do scheduler
    pub fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            total_processes:   self.processes.len(),
            running:  self.processes.iter().filter(|p| p.state == ProcessState::Running).count(),
            ready:    self.processes.iter().filter(|p| p.state == ProcessState::Ready).count(),
            blocked:  self.processes.iter().filter(|p| matches!(p.state, ProcessState::Blocked{..})).count(),
            sleeping: self.processes.iter().filter(|p| matches!(p.state, ProcessState::Sleeping{..})).count(),
            dead:     self.processes.iter().filter(|p| matches!(p.state, ProcessState::Dead{..})).count(),
            context_switches: self.context_switches,
            current_tick: self.tick,
            current_pid: self.current_pid,
            ready_in_queues: self.ready_queues.total(),
        }
    }

    /// Retorna lista de todos os processos (para o comando 'ps')
    pub fn list_processes(&self) -> Vec<ProcessInfo> {
        self.processes.iter().map(|p| ProcessInfo {
            pid: p.pid,
            name: p.name.clone(),
            state: match &p.state { 
                crate::modules::scheduler::ProcessState::Running => "Running".into(),
                crate::modules::scheduler::ProcessState::Ready => "Ready".into(),
                crate::modules::scheduler::ProcessState::New => "New".into(),
                _ => "Other".into(),
            },
            priority: p.priority,
            cpu_ticks: p.cpu_ticks_total,
            schedule_count: p.stats.schedule_count,
        }).collect()
    }
}

/// Estatísticas do scheduler
#[derive(Debug)]
pub struct SchedulerStats {
    pub total_processes: usize,
    pub running:         usize,
    pub ready:           usize,
    pub blocked:         usize,
    pub sleeping:        usize,
    pub dead:            usize,
    pub context_switches: u64,
    pub current_tick:    u64,
    pub current_pid:     Option<Pid>,
    pub ready_in_queues: usize,
}

/// Informações resumidas de um processo (para listagem)
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid:            Pid,
    pub name:           String,
    pub state:          String,
    pub priority:       Priority,
    pub cpu_ticks:      u64,
    pub schedule_count: u64,
}

// ─── Instância Global ────────────────────────────────────────────────────────

pub static SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler::new());

/// Inicializa o scheduler global
pub fn init() {
    SCHEDULER.lock().init();
    crate::serial_println!("[SCHED] Sistema de processos pronto");
}

/// Cria uma nova tarefa
pub fn spawn(name: &str, entry: fn(), priority: Priority) -> Pid {
    SCHEDULER.lock().spawn(name, entry as u64, priority)
}

/// Chamado pelo timer IRQ — verifica preempção
pub fn timer_tick() -> bool {
    SCHEDULER.lock().tick()
}

/// Seleciona o próximo processo (só bookkeeping — não troca de pilha).
/// Mantido para compatibilidade (ex: porta ARM); em x86_64 usar
/// `preempt()` ou `yield_now()`, que executam a troca real.
pub fn schedule() -> Option<Pid> {
    SCHEDULER.lock().schedule()
}

/// RSP "de despejo" usado quando não há nenhuma tarefa actual válida
/// para onde guardar o contexto que está a sair (não devia acontecer
/// em operação normal, uma vez que `register_current_as_task` garante
/// sempre uma tarefa actual). Single-core: sem necessidade de ser
/// per-CPU nem atómico.
static mut DUMMY_RSP: u64 = 0;

/// Regista a execução ACTUAL (por exemplo, o `kernel_loop` do
/// arranque) como uma tarefa normal do scheduler, para que possa ser
/// trocada de/para tal como qualquer outra.
///
/// Ao contrário de `spawn`, não constrói uma pilha nova nem um
/// trampolim — a "pilha inicial" desta tarefa é simplesmente onde ela
/// já está a correr agora. O `kernel_rsp` fica a 0 até à primeira vez
/// que for trocada para fora (nesse momento, `switch_context`
/// preenche-o com o RSP real desta execução).
///
/// Deve ser chamada exactamente uma vez, imediatamente depois de
/// `scheduler::init()`, a partir do código que vai tornar-se o loop
/// principal do kernel.
pub fn register_current_as_task(name: &str, priority: Priority) -> Pid {
    let mut sched = SCHEDULER.lock();
    let pid = alloc_pid();
    let sandbox_pid = crate::security::sandbox::create_process_sandbox(
        name,
        crate::security::TrustLevel::System,
    );
    let pcb = ProcessControlBlock {
        pid,
        parent_pid: 0,
        name: name.to_string(),
        state: ProcessState::Running,
        priority,
        // stack_top=0 ⇒ CpuContext::new_kernel_task não constrói frame
        // inicial (ver comentário nesse método).
        context: CpuContext::new_kernel_task(0, 0),
        cpu_ticks_total: 0,
        quantum_remaining: priority.quantum(),
        created_at_tick: sched.tick,
        stack: Vec::new(), // usa a pilha já activa desta execução
        sandbox_pid,
        stats: ProcessStats::default(),
    };
    sched.processes.push(pcb);
    sched.current_pid = Some(pid);
    crate::serial_println!("[SCHED] Tarefa actual registada: '{}' PID={}", name, pid);
    pid
}

/// Ponto de decisão + troca REAL de contexto, chamado pelo handler do
/// timer (IRQ0) a cada tick. Faz o bookkeeping normal (`tick`) e, se o
/// quantum da tarefa actual expirou, escolhe a próxima e efectivamente
/// troca de pilha via `arch::context_switch::switch_context`.
///
/// Tem de ser chamada SEM nenhum outro Spinlock do scheduler retido, e
/// o lock interno é sempre largado antes da troca de pilha em si.
pub fn preempt() {
    let mut sched = SCHEDULER.lock();

    // Actualiza contadores; diz se o quantum da tarefa actual expirou.
    if !sched.tick() {
        return;
    }

    let old_pid = sched.current_pid;

    let next_pid = match sched.schedule() {
        Some(pid) => pid,
        None => return, // não devia acontecer — há sempre o idle
    };

    // Nada a trocar se a própria tarefa foi re-seleccionada (única
    // tarefa pronta, por exemplo).
    if old_pid == Some(next_pid) {
        return;
    }

    let old_rsp_ptr: *mut u64 = match old_pid.and_then(|pid| sched.get_process_mut(pid)) {
        Some(p) => &mut p.context.kernel_rsp as *mut u64,
        None => &raw mut DUMMY_RSP,
    };

    let new_rsp = match sched.get_process(next_pid) {
        Some(p) => p.context.kernel_rsp,
        None => return,
    };

    // Larga o lock ANTES de trocar de pilha: esta chamada só "regressa"
    // quando a tarefa actual for escolhida de novo — e nesse meio
    // tempo outras tarefas vão chamar preempt()/yield_now() e precisam
    // de conseguir bloquear o scheduler.
    drop(sched);

    unsafe {
        crate::arch::context_switch::switch_context(old_rsp_ptr, new_rsp);
    }
}

/// Cedência voluntária de CPU (syscall `yield`): igual a `preempt()`
/// mas sem depender do quantum ter expirado — troca sempre que há
/// outra tarefa pronta.
pub fn yield_now() {
    let mut sched = SCHEDULER.lock();

    let old_pid = sched.current_pid;

    let next_pid = match sched.schedule() {
        Some(pid) => pid,
        None => return,
    };

    if old_pid == Some(next_pid) {
        return;
    }

    let old_rsp_ptr: *mut u64 = match old_pid.and_then(|pid| sched.get_process_mut(pid)) {
        Some(p) => &mut p.context.kernel_rsp as *mut u64,
        None => &raw mut DUMMY_RSP,
    };

    let new_rsp = match sched.get_process(next_pid) {
        Some(p) => p.context.kernel_rsp,
        None => return,
    };

    drop(sched);

    unsafe {
        crate::arch::context_switch::switch_context(old_rsp_ptr, new_rsp);
    }
}

/// Termina a tarefa actualmente em execução (chamado pelo trampolim de
/// arranque se a função de entrada de uma tarefa alguma vez retornar).
pub fn exit_current(exit_code: i32) {
    let mut sched = SCHEDULER.lock();
    if let Some(pid) = sched.current_pid {
        sched.exit_process(pid, exit_code);
    }
}

/// Coloca o processo atual para dormir
pub fn sleep(ticks: u64) {
    SCHEDULER.lock().sleep_current(ticks);
}

/// Estatísticas globais
pub fn get_stats() -> SchedulerStats {
    SCHEDULER.lock().stats()
}

/// Lista todos os processos
pub fn list_processes() -> Vec<ProcessInfo> {
    SCHEDULER.lock().list_processes()
}

/// Termina um processo pelo PID (Fase 2)
pub fn kill(pid: Pid, exit_code: i32) -> bool {
    let mut sched = SCHEDULER.lock();
    if sched.get_process(pid).is_some() {
        sched.exit_process(pid, exit_code);
        true
    } else {
        false
    }
}



/// Tarefa idle — executada quando nenhum outro processo está pronto.
/// Usa 'hlt' para pausar a CPU até a próxima interrupção,
/// economizando energia (essencial para dispositivos móveis).
fn idle_task() {
    loop {
        x86_64::instructions::hlt();
    }
}
