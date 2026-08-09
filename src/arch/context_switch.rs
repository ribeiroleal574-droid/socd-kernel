// ============================================================
// SOC-D Kernel — Troca de Contexto Real (x86_64)
// ============================================================
//
// Implementa a técnica clássica de "stack switching" usada em kernels
// didáticos (xv6, e várias forks da comunidade "Writing an OS in Rust").
// Cada tarefa do scheduler tem a sua própria pilha do kernel. Trocar de
// tarefa resume-se a:
//
//   1. Guardar os registos "callee-saved" (rbp, rbx, r12-r15) da tarefa
//      actual na SUA PRÓPRIA pilha, e guardar o RSP resultante no PCB.
//   2. Carregar o RSP previamente guardado da PRÓXIMA tarefa.
//   3. Restaurar os registos callee-saved dessa tarefa a partir da SUA
//      pilha.
//   4. `ret` — como o topo dessa pilha contém um endereço de retorno
//      (colocado lá pelo trampolim de arranque, na primeira vez que a
//      tarefa corre, ou pela troca anterior, nas seguintes), a CPU
//      salta para lá e a tarefa continua exactamente de onde parou.
//
// Isto é chamado a partir do handler do timer (IRQ0) ou de uma syscall
// de yield voluntário, SEMPRE fora do Spinlock do scheduler — nunca se
// pode reter esse lock durante uma troca de pilha (a tarefa só "volta"
// desta chamada quando for escolhida de novo, o que podia nunca
// acontecer se o lock ficasse preso).
//
// LIMITAÇÃO CONHECIDA: esta implementação assume um único core (sem
// SMP). Em multi-core, current_pid/ponteiros teriam de ser per-CPU.
// ============================================================

use core::arch::naked_asm;

/// Troca a pilha do kernel da tarefa actual para a nova.
///
/// # Parâmetros
/// - `old_rsp_out`: onde guardar o RSP da tarefa que está a sair.
/// - `new_rsp`: RSP da tarefa para onde vamos (produzido por uma troca
///   anterior, ou por [`prepare_initial_stack`] para uma tarefa nova).
///
/// # Segurança
/// - `old_rsp_out` tem de ser um ponteiro válido para escrita.
/// - `new_rsp` tem de apontar para uma pilha construída por este mesmo
///   mecanismo.
/// - NUNCA chamar com qualquer Spinlock do scheduler bloqueado.
/// - Só é seguro em contexto single-core / secção não reentrante.
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(old_rsp_out: *mut u64, new_rsp: u64) {
    naked_asm!(
        // Guarda os registos callee-saved da tarefa actual
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Guarda o RSP actual em *old_rsp_out (rdi = 1º arg, System V)
        "mov [rdi], rsp",
        // Muda para a pilha da nova tarefa (rsi = 2º arg)
        "mov rsp, rsi",
        // Restaura os registos callee-saved da nova tarefa
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        // Salta para o endereço de retorno no topo da nova pilha:
        // o trampolim de arranque (1ª execução) ou o ponto exacto
        // onde essa tarefa foi trocada anteriormente (retomas).
        "ret",
    );
}

/// Trampolim de arranque: ponto de entrada da PRIMEIRA vez que uma
/// tarefa corre. `prepare_initial_stack` deixa o ponteiro da função de
/// entrada em R15, que `switch_context` restaura antes do `ret` que
/// nos traz aqui.
///
/// Activa interrupções (`sti`) antes de chamar a tarefa: ao entrarmos
/// aqui vindos de dentro do handler do timer, IF estava a 0 (a CPU
/// desliga IF automaticamente ao entrar numa interrupção de hardware).
/// Uma tarefa nova tem de arrancar com interrupções ligadas, tal como
/// qualquer tarefa normal do kernel — caso contrário `hlt` no idle
/// nunca mais acordava.
#[unsafe(naked)]
unsafe extern "C" fn task_trampoline() -> ! {
    naked_asm!(
        "sti",
        "call r15",
        "call {task_exit}",
        task_exit = sym task_exit_trampoline,
    );
}

/// Chamada se a função de entrada de uma tarefa alguma vez retornar
/// (tarefas do kernel normalmente correm em loop e nunca retornam,
/// mas isto evita executar código indefinido se acontecer).
extern "C" fn task_exit_trampoline() -> ! {
    crate::modules::scheduler::exit_current(0);
    // Cede a CPU imediatamente em vez de esperar pelo próximo tick do
    // timer — mais rápido e não depende de nenhuma lógica de
    // preempção externa. `yield_now()` já trata correctamente o caso
    // de não haver nenhuma tarefa "actual" (current_pid=None, que
    // `exit_current` acabou de definir).
    loop {
        crate::modules::scheduler::yield_now();
        // Se por alguma razão yield_now() devolver o controlo (não
        // devia, a não ser que não haja mesmo mais nenhuma tarefa
        // pronta), hlt até à próxima interrupção e tenta de novo.
        x86_64::instructions::hlt();
    }
}

/// Constrói a pilha inicial de uma tarefa nova, de forma a que a
/// primeira `switch_context` para ela "aterre" em `task_trampoline`,
/// com R15 = `entry_point`.
///
/// Retorna o valor de RSP a guardar em `CpuContext::kernel_rsp`.
///
/// # Segurança
/// `stack_top` tem de ser o topo (endereço mais alto) de uma região de
/// memória válida e exclusiva desta tarefa, com pelo menos 56 bytes
/// livres imediatamente abaixo dele.
pub unsafe fn prepare_initial_stack(stack_top: u64, entry_point: u64) -> u64 {
    // Alinha a 16 bytes (convenção System V) antes de reservar espaço.
    let aligned_top = stack_top & !0xF;

    let write = |addr: u64, val: u64| unsafe {
        (addr as *mut u64).write(val);
    };

    // Layout, de cima (endereço mais alto) para baixo — tem de
    // corresponder exactamente à ordem push/pop em `switch_context`:
    write(aligned_top - 8, task_trampoline as *const () as u64); // retorno
    write(aligned_top - 16, 0); // rbp
    write(aligned_top - 24, 0); // rbx
    write(aligned_top - 32, 0); // r12
    write(aligned_top - 40, 0); // r13
    write(aligned_top - 48, 0); // r14
    write(aligned_top - 56, entry_point); // r15 → lido pelo trampolim

    aligned_top - 56
}
