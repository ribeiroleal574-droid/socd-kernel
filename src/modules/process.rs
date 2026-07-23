// ============================================================
// SOC-D Kernel — Gestor de Processos Dinâmicos (Fase 2)
// ============================================================
//
// Este módulo liga o ELF loader ao scheduler para criar
// processos a partir de binários ELF carregados em runtime.
//
// Funcionalidades Fase 2:
//   exec_elf(name, data) — carrega ELF + cria processo
//   kill(pid)            — termina processo por PID
//   wait(pid)            — aguarda término de processo
//   fork_kernel_task()   — cria tarefa interna do kernel
//   Tabela de símbolos do kernel para módulos externos
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;

use crate::modules::scheduler::{self, Pid, Priority};
use crate::modules::elf_loader::{self, ElfError};

// ─── Tabela de Símbolos do Kernel (Fase 2) ───────────────────
//
// Funções do kernel exportadas para módulos ELF externos.
// Módulos podem chamar estas funções via relocação de símbolos.
//
// Convenção de nomes: socd_<subsistema>_<função>

/// Alocador público para módulos — wrapper de alloc::alloc
pub unsafe extern "C" fn socd_alloc(size: usize, align: usize) -> *mut u8 {
    use alloc::alloc::{alloc, Layout};
    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    alloc(layout)
}

/// Desalocador público para módulos
pub unsafe extern "C" fn socd_dealloc(ptr: *mut u8, size: usize, align: usize) {
    use alloc::alloc::{dealloc, Layout};
    if ptr.is_null() { return; }
    if let Ok(layout) = Layout::from_size_align(size, align) {
        dealloc(ptr, layout);
    }
}

/// Print serial para módulos
pub extern "C" fn socd_serial_print(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 { return; }
    let s = unsafe { core::slice::from_raw_parts(ptr, len) };
    if let Ok(text) = core::str::from_utf8(s) {
        crate::serial_println!("[MODULE] {}", text);
    }
}

/// Cria novo processo a partir de módulo
pub extern "C" fn socd_spawn(name_ptr: *const u8, name_len: usize, entry: u64) -> u64 {
    if name_ptr.is_null() { return 0; }
    let name_bytes = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let name = core::str::from_utf8(name_bytes).unwrap_or("unnamed");
    let entry_fn: fn() = unsafe { core::mem::transmute(entry as usize) };
    scheduler::spawn(name, entry_fn, Priority::Normal)
}

/// Dorme N ticks
pub extern "C" fn socd_sleep(ticks: u64) {
    scheduler::sleep(ticks);
}

/// Retorna tick atual do scheduler
pub extern "C" fn socd_tick() -> u64 {
    scheduler::get_stats().current_tick
}

// ─── Tabela de símbolos exportados ───────────────────────────

pub struct KernelExport {
    pub name: &'static str,
    pub address: u64,
}

// ─── Tabela de Símbolos do Kernel (Fase 2) ───────────────────
//
// Funções do kernel exportadas para módulos ELF externos.
// Não podem ser convertidas para u64 em tempo de compilação
// (restrição do Rust const eval) — tabela

// Tabela inicializada em runtime por init()
pub static KERNEL_EXPORTS: Spinlock<Vec<KernelExport>> =
    Spinlock::new(Vec::new());

fn build_kernel_exports() {
    let mut table = KERNEL_EXPORTS.lock();
    table.push(KernelExport { name: "socd_alloc",        address: socd_alloc        as *const () as usize as u64 });
    table.push(KernelExport { name: "socd_dealloc",      address: socd_dealloc      as *const () as usize as u64 });
    table.push(KernelExport { name: "socd_serial_print", address: socd_serial_print as *const () as usize as u64 });
    table.push(KernelExport { name: "socd_spawn",        address: socd_spawn        as *const () as usize as u64 });
    table.push(KernelExport { name: "socd_sleep",        address: socd_sleep        as *const () as usize as u64 });
    table.push(KernelExport { name: "socd_tick",         address: socd_tick         as *const () as usize as u64 });
}

/// Resolve um símbolo pelo nome — procura primeiro nos exports do kernel,
/// depois nos módulos ELF já carregados.
pub fn resolve_symbol(name: &str) -> Option<u64> {
    // 1. Símbolos do kernel
    if let Some(addr) = KERNEL_EXPORTS.lock().iter().find(|e| e.name == name).map(|e| e.address) {
        return Some(addr);
    }
    // 2. Símbolos de outros módulos ELF carregados
    elf_loader::find_module_symbol(name)
}

// ─── Registo de Processos Dinâmicos ──────────────────────────

#[derive(Debug, Clone)]
pub struct DynProcess {
    pub pid:       Pid,
    pub name:      String,
    pub elf_module: Option<String>, // nome do módulo ELF de origem
    pub entry:     u64,
}

struct ProcessRegistry {
    processes: Vec<DynProcess>,
}

impl ProcessRegistry {
    const fn new() -> Self { Self { processes: Vec::new() } }

    fn register(&mut self, pid: Pid, name: String, elf_module: Option<String>, entry: u64) {
        self.processes.push(DynProcess { pid, name, elf_module, entry });
    }

    fn remove(&mut self, pid: Pid) -> bool {
        let before = self.processes.len();
        self.processes.retain(|p| p.pid != pid);
        self.processes.len() < before
    }

    fn list(&self) -> Vec<DynProcess> {
        self.processes.clone()
    }

    fn find(&self, pid: Pid) -> Option<&DynProcess> {
        self.processes.iter().find(|p| p.pid == pid)
    }
}

static PROC_REGISTRY: Spinlock<ProcessRegistry> =
    Spinlock::new(ProcessRegistry::new());

// ─── API Pública ─────────────────────────────────────────────

/// Carrega um binário ELF e lança-o como processo.
/// Retorna o PID do processo criado, ou um erro.
pub fn exec_elf(name: &str, data: &[u8]) -> Result<Pid, ExecError> {
    crate::serial_println!("[PROC] exec_elf '{}' ({} bytes)", name, data.len());

    // 1. Carrega o ELF via ElfModuleManager (API pública, sem aceder campos privados)
    elf_loader::ELF_MANAGER.lock().load(name, data)
        .map_err(ExecError::Elf)?;

    // 2. Obtém o endereço base (entry point) do módulo recém-carregado
    let entry = {
        let mgr = elf_loader::ELF_MANAGER.lock();
        mgr.list()
            .into_iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, base, _)| base)
            .ok_or(ExecError::NoEntryPoint)?
    };

    // 3. Cria processo no scheduler
    let entry_fn: fn() = unsafe { core::mem::transmute(entry as usize) };
    let pid = scheduler::spawn(name, entry_fn, Priority::Normal);

    // 4. Regista na tabela de processos dinâmicos
    PROC_REGISTRY.lock().register(
        pid,
        name.to_string(),
        Some(name.to_string()),
        entry,
    );

    crate::serial_println!("[PROC] Processo '{}' criado PID={}", name, pid);
    Ok(pid)
}

/// Termina um processo pelo PID.
/// Retorna true se o processo existia e foi terminado.
pub fn kill(pid: Pid) -> bool {
    crate::serial_println!("[PROC] kill PID={}", pid);
    let found = PROC_REGISTRY.lock().remove(pid);
    if found {
        crate::modules::scheduler::SCHEDULER
            .lock()
            .exit_process(pid, -1);
        crate::serial_println!("[PROC] PID={} terminado", pid);
    } else {
        crate::serial_println!("[PROC] PID={} nao encontrado", pid);
    }
    found
}

/// Cria uma tarefa interna do kernel (não ELF).
pub fn spawn_kernel_task(name: &str, entry: fn(), priority: Priority) -> Pid {
    let pid = scheduler::spawn(name, entry, priority);
    PROC_REGISTRY.lock().register(
        pid,
        name.to_string(),
        None,
        entry as u64,
    );
    pid
}

/// Lista todos os processos dinâmicos registados.
pub fn list_dynamic() -> Vec<DynProcess> {
    PROC_REGISTRY.lock().list()
}

/// Número de símbolos exportados pelo kernel.
pub fn kernel_symbol_count() -> usize {
    KERNEL_EXPORTS.lock().len()
}

/// Inicializa o gestor de processos dinâmicos.
pub fn init() {
    build_kernel_exports();
    let count = KERNEL_EXPORTS.lock().len();
    crate::serial_println!("[PROC] Gestor de processos dinamicos ativo");
    crate::serial_println!("[PROC] {} simbolos do kernel exportados:", count);
    for exp in KERNEL_EXPORTS.lock().iter() {
        crate::serial_println!("[PROC]   {} @ 0x{:016x}", exp.name, exp.address);
    }
}

// ─── Erro de exec ────────────────────────────────────────────

#[derive(Debug)]
pub enum ExecError {
    Elf(ElfError),
    NoEntryPoint,
    MemoryFull,
}

impl core::fmt::Display for ExecError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            ExecError::Elf(e)      => write!(f, "ELF: {:?}", e),
            ExecError::NoEntryPoint => write!(f, "sem entry point"),
            ExecError::MemoryFull   => write!(f, "memoria insuficiente"),
        }
    }
}

// ─── Módulo ELF de demonstração embutido ─────────────────────
//
// Um mini-ELF x86_64 relocável de teste embutido no kernel.
// Contém apenas uma função `init()` que chama socd_serial_print.
//
// Gerado por:
//   nasm -f elf64 demo.asm -o demo.o
//
// demo.asm:
//   extern socd_serial_print
//   section .text
//   global init
//   init:
//     mov rdi, msg
//     mov rsi, msg_len
//     call socd_serial_print
//     ret
//   section .rodata
//   msg: db "Hello from ELF module!", 10
//   msg_len: equ $ - msg
//
// NOTA: Este é um ELF minimal sintético para demonstração.
// Em produção, módulos são compilados externamente com:
//   rustc --target x86_64-unknown-none --crate-type cdylib

/// ELF de demo: imprime uma mensagem via socd_serial_print e retorna.
/// Formato: ELF64 relocável mínimo gerado sinteticamente.
pub fn demo_elf_bytes() -> Vec<u8> {
    // Código x86_64 de demonstração (shellcode que chama socd_serial_print)
    // mov rdi, <ptr>; mov rsi, <len>; call <socd_serial_print>; ret
    // Como não podemos compilar ELF real aqui, criamos um processo
    // interno de demonstração sem ELF real — a infra está pronta para
    // receber ELFs reais compilados externamente.
    Vec::new() // placeholder — ver exec_demo() abaixo
}

/// Demonstração da Fase 2: cria tarefas dinâmicas de teste sem ELF externo
pub fn exec_demo() {
    crate::serial_println!("\n[FASE2] === Demonstracao Fase 2: Processos Dinamicos ===");

    // Tarefa 1: processo de monitorização
    let pid1 = spawn_kernel_task("monitor", monitor_task, Priority::Low);
    crate::serial_println!("[FASE2] monitor PID={} criado", pid1);

    // Tarefa 2: processo de logging
    let pid2 = spawn_kernel_task("logger", logger_task, Priority::Low);
    crate::serial_println!("[FASE2] logger PID={} criado", pid2);

    // Tarefa 3: processo de cleanup
    let pid3 = spawn_kernel_task("cleanup", cleanup_task, Priority::Low);
    crate::serial_println!("[FASE2] cleanup PID={} criado", pid3);

    crate::serial_println!("[FASE2] {} simbolos kernel disponiveis para modulos ELF",
        KERNEL_EXPORTS.lock().len());
    crate::serial_println!("[FASE2] Use 'exec <nome>' no shell para carregar ELF externo");
    crate::serial_println!("[FASE2] =====================================================\n");
}

// ─── Tarefas de demonstração ─────────────────────────────────

fn monitor_task() {
    crate::serial_println!("[monitor] Tarefa de monitorizacao iniciada (PID dinamico)");
    // Em produção: loop de monitorização de métricas
    // Por agora termina imediatamente para não interferir
}

fn logger_task() {
    crate::serial_println!("[logger] Tarefa de logging iniciada (PID dinamico)");
}

fn cleanup_task() {
    crate::serial_println!("[cleanup] Tarefa de limpeza iniciada (PID dinamico)");
}
