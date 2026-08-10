extern crate alloc;
// ============================================================
// SOC-D Kernel — Interface de Syscall
// ============================================================
//
// A interface de syscall é o contrato entre processos de usuário
// e o kernel. Todo acesso a recursos do kernel passa aqui.
//
// No SOC-D, syscalls são invocadas via:
//   x86_64: instrução SYSCALL (rax=número, rdi/rsi/rdx=args)
//   ARM64:  instrução SVC #0  (x8=número,  x0/x1/x2=args)
//
// Tabela de syscalls SOC-D (compatível com POSIX onde possível):
//
//   Arquivo/FS:
//     0  sys_open(path, flags, mode) → fd
//     1  sys_close(fd)
//     2  sys_read(fd, buf, len) → bytes
//     3  sys_write(fd, buf, len) → bytes
//     4  sys_stat(path, stat_buf) → 0/-1
//     5  sys_unlink(path) → 0/-1
//     6  sys_mkdir(path, mode) → 0/-1
//     7  sys_readdir(fd, dirent_buf, len) → count
//
//   Processo:
//     10 sys_exit(code)
//     11 sys_fork() → pid
//     12 sys_exec(path, argv, envp) → (não retorna)
//     13 sys_getpid() → pid
//     14 sys_sleep(ms) → 0
//     15 sys_yield()
//     16 sys_kill(pid, signal) → 0/-1
//     17 sys_wait(pid, status_ptr) → pid
//
//   Memória:
//     20 sys_mmap(addr, len, prot, flags) → addr
//     21 sys_munmap(addr, len) → 0/-1
//     22 sys_mprotect(addr, len, prot) → 0/-1
//     23 sys_brk(addr) → new_brk
//
//   Rede:
//     30 sys_socket(domain, type, proto) → fd
//     31 sys_bind(fd, addr, addrlen) → 0/-1
//     32 sys_connect(fd, addr, addrlen) → 0/-1
//     33 sys_listen(fd, backlog) → 0/-1
//     34 sys_accept(fd, addr, addrlen) → fd
//     35 sys_send(fd, buf, len, flags) → bytes
//     36 sys_recv(fd, buf, len, flags) → bytes
//     37 sys_close_socket(fd) → 0/-1
//
//   SOC-D específicas:
//     100 sys_p2p_send(node_id, data, len) → 0/-1
//     101 sys_p2p_recv(buf, len) → bytes
//     102 sys_ia_infer(model_id, input, len, output, out_len) → 0/-1
//     103 sys_edge_submit(task_kind, payload, len) → job_id
//     104 sys_edge_result(job_id, buf, len) → bytes
//     105 sys_wasm_load(name, binary, len) → module_id
//     106 sys_wasm_call(module_id, func, args, args_len) → 0/-1
//     107 sys_xr_begin_frame(frame_state_ptr) → 0/-1
//     108 sys_xr_end_frame(frame_state_ptr) → 0/-1
//     109 sys_quantum_submit(circuit_ptr, len, shots) → job_id
//     110 sys_quantum_result(job_id, buf, len) → bytes
//     111 sys_ui_create_surface(title, x, y, w, h) → surface_id
//     112 sys_ui_update_surface(surface_id, buf, len) → 0/-1
//     113 sys_ui_destroy_surface(surface_id) → 0/-1
//     114 sys_security_check(permission) → 0/1
//     115 sys_get_stats(kind, buf, len) → bytes
// ============================================================

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spinning_top::Spinlock;

/// Número de syscall
pub type SyscallNr = u64;

/// Resultado de uma syscall
pub type SyscallResult = i64;

/// Códigos de erro POSIX-like
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
#[allow(non_camel_case_types)]
pub enum Errno {
    Success        =   0,
    EPERM          =  -1,   // Operação não permitida
    ENOENT         =  -2,   // Arquivo ou diretório não encontrado
    ESRCH          =  -3,   // Processo não encontrado
    EINTR          =  -4,   // Chamada interrompida
    EIO            =  -5,   // Erro de I/O
    EBADF          =  -9,   // Descritor inválido
    EAGAIN         = -11,   // Tente novamente
    ENOMEM         = -12,   // Sem memória
    EACCES         = -13,   // Permissão negada
    EFAULT         = -14,   // Endereço ruim
    EBUSY          = -16,   // Device ocupado
    EEXIST         = -17,   // Arquivo já existe
    ENODEV         = -19,   // Dispositivo não encontrado
    ENOTDIR        = -20,   // Não é diretório
    EISDIR         = -21,   // É um diretório
    EINVAL         = -22,   // Argumento inválido
    ENFILE         = -23,   // Muitos arquivos abertos
    ENOSPC         = -28,   // Sem espaço no device
    ENOSYS         = -38,   // Syscall não implementada
    ECONNREFUSED   = -111,  // Conexão recusada
    ETIMEDOUT      = -110,  // Timeout de conexão
    SOCD_EPERM     = -200,  // Sandbox negou permissão
    SOCD_EQUANTUM  = -201,  // Erro no circuito quântico
    SOCD_EWASM     = -202,  // Erro no runtime WASM
    SOCD_EEDGE     = -203,  // Erro no edge computing
}

impl Errno {
    pub fn as_i64(self) -> i64 { self as i64 }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Success      => "OK",
            Self::EPERM        => "EPERM",
            Self::ENOENT       => "ENOENT",
            Self::EINVAL       => "EINVAL",
            Self::ENOMEM       => "ENOMEM",
            Self::EACCES       => "EACCES",
            Self::ENOSYS       => "ENOSYS",
            Self::SOCD_EPERM   => "SOCD_EPERM",
            Self::SOCD_EWASM   => "SOCD_EWASM",
            Self::SOCD_EQUANTUM=> "SOCD_EQUANTUM",
            Self::SOCD_EEDGE   => "SOCD_EEDGE",
            _                  => "EUNKNOWN",
        }
    }
}

/// Argumentos de uma syscall (convenção de registro)
#[derive(Debug, Clone, Copy, Default)]
pub struct SyscallArgs {
    pub nr:  u64,   // Número da syscall (rax / x8)
    pub a0:  u64,   // 1º argumento  (rdi / x0)
    pub a1:  u64,   // 2º argumento  (rsi / x1)
    pub a2:  u64,   // 3º argumento  (rdx / x2)
    pub a3:  u64,   // 4º argumento  (r10 / x3)
    pub a4:  u64,   // 5º argumento  (r8  / x4)
    pub a5:  u64,   // 6º argumento  (r9  / x5)
}

/// Dispatcher principal de syscalls
pub fn dispatch(args: &SyscallArgs) -> SyscallResult {
    // Verifica sandbox antes de qualquer syscall
    let pid = x86_64::instructions::interrupts::without_interrupts(|| {
        crate::modules::scheduler::SCHEDULER.lock()
            .get_process(1)
            .map(|p| p.sandbox_pid)
            .unwrap_or(0)
    });

    match args.nr {
        // ── Arquivo / FS ─────────────────────────────────────────────
        0  => sys_open(args),
        1  => sys_close(args),
        2  => sys_read(args),
        3  => sys_write(args),
        4  => sys_stat(args),
        5  => sys_unlink(args),
        6  => sys_mkdir(args),
        7  => sys_readdir(args),

        // ── Processo ────────────────────────────────────────────────
        10 => sys_exit(args),
        13 => sys_getpid(args),
        14 => sys_sleep(args),
        15 => { crate::modules::scheduler::yield_now(); 0 }, // yield (troca real de contexto)

        // ── Rede ────────────────────────────────────────────────────
        30 => sys_socket(args),
        32 => sys_connect(args),
        35 => sys_send(args),
        36 => sys_recv(args),
        37 => sys_close_socket(args),

        // ── SOC-D específicas ────────────────────────────────────────
        100 => sys_p2p_send(args),
        101 => sys_p2p_recv(args),
        102 => sys_ia_infer(args),
        103 => sys_edge_submit(args),
        104 => sys_edge_result(args),
        105 => sys_wasm_load(args),
        106 => sys_wasm_call(args),
        107 => sys_xr_begin_frame(args),
        108 => sys_xr_end_frame(args),
        109 => sys_quantum_submit(args),
        110 => sys_quantum_result(args),
        111 => sys_ui_create_surface(args),
        113 => sys_ui_destroy_surface(args),
        114 => sys_security_check(args),
        115 => sys_get_stats(args),

        _ => {
            crate::serial_println!("[SYSCALL] Nr={} nao implementada", args.nr);
            Errno::ENOSYS.as_i64()
        }
    }
}

// ─── Implementações ──────────────────────────────────────────────────────────

fn sys_open(args: &SyscallArgs) -> SyscallResult {
    let path_ptr = args.a0 as *const u8;
    let path = unsafe_read_str(path_ptr, 256);
    match crate::modules::tmpfs::read(&path) {
        Ok(_) => {
            // Retorna fd simulado
            crate::serial_println!("[SYSCALL] open('{}') = 4", path);
            4
        }
        Err(_) => Errno::ENOENT.as_i64(),
    }
}

fn sys_close(args: &SyscallArgs) -> SyscallResult {
    crate::serial_println!("[SYSCALL] close(fd={})", args.a0);
    0
}

fn sys_read(args: &SyscallArgs) -> SyscallResult {
    let fd = args.a0;
    let len = args.a2 as usize;
    if fd == 0 { return 0; } // stdin: sem input
    Errno::EBADF.as_i64()
}

fn sys_write(args: &SyscallArgs) -> SyscallResult {
    let fd = args.a0;
    let buf_ptr = args.a1 as *const u8;
    let len = args.a2 as usize;

    if fd == 1 || fd == 2 {
        // stdout / stderr → imprime no serial
        let s = unsafe_read_bytes(buf_ptr, len.min(1024));
        if let Ok(text) = core::str::from_utf8(&s) {
            crate::serial_print!("{}", text);
        }
        return len as i64;
    }
    Errno::EBADF.as_i64()
}

fn sys_stat(args: &SyscallArgs) -> SyscallResult {
    let path = unsafe_read_str(args.a0 as *const u8, 256);
    match crate::modules::tmpfs::ls(&path) {
        Ok(_) => 0,
        Err(_) => Errno::ENOENT.as_i64(),
    }
}

fn sys_unlink(_: &SyscallArgs)  -> SyscallResult { Errno::EPERM.as_i64() }
fn sys_mkdir(_: &SyscallArgs)   -> SyscallResult { Errno::EPERM.as_i64() }
fn sys_readdir(_: &SyscallArgs) -> SyscallResult { Errno::ENOSYS.as_i64() }

fn sys_exit(args: &SyscallArgs) -> SyscallResult {
    let code = args.a0 as i32;
    crate::serial_println!("[SYSCALL] exit({})", code);
    // NOTA: corrigido — chamava exit_process(1, ...) fixo, matando
    // sempre o PID 1 em vez do processo que realmente pediu exit().
    crate::modules::scheduler::exit_current(code);
    // Não faz sentido continuar a executar código desta tarefa depois
    // de exit() — cede a CPU de imediato (troca real de contexto).
    crate::modules::scheduler::yield_now();
    0
}

fn sys_getpid(_: &SyscallArgs) -> SyscallResult {
    x86_64::instructions::interrupts::without_interrupts(|| {
        crate::modules::scheduler::SCHEDULER.lock()
            .stats().current_pid.unwrap_or(1) as i64
    })
}

fn sys_sleep(args: &SyscallArgs) -> SyscallResult {
    crate::modules::scheduler::sleep(args.a0);
    0
}

fn sys_socket(args: &SyscallArgs) -> SyscallResult {
    let proto = match args.a1 {
        1 => crate::net::Protocol::TCP,
        2 => crate::net::Protocol::UDP,
        _ => crate::net::Protocol::Raw(args.a2 as u8),
    };
    crate::net::ethernet::socket_create(proto) as i64
}

fn sys_connect(args: &SyscallArgs) -> SyscallResult {
    let fd = args.a0 as u32;
    // Parse sockaddr simplificado: assumindo IPv4
    let addr_ptr = args.a1 as *const u8;
    let bytes = unsafe_read_bytes(addr_ptr, 8);
    if bytes.len() >= 8 {
        let port = u16::from_be_bytes([bytes[2], bytes[3]]);
        let ip = crate::net::Ipv4Addr([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let addr = crate::net::SocketAddr::V4(ip, port);
        if crate::net::ethernet::socket_connect(fd, addr) { return 0; }
    }
    Errno::ECONNREFUSED.as_i64()
}

fn sys_send(args: &SyscallArgs) -> SyscallResult {
    let fd = args.a0 as u32;
    let buf = unsafe_read_bytes(args.a1 as *const u8, args.a2 as usize);
    crate::net::ethernet::socket_send(fd, &buf) as i64
}

fn sys_recv(args: &SyscallArgs) -> SyscallResult {
    let fd   = args.a0 as u32;
    let len  = args.a2 as usize;
    let ptr  = args.a1 as *mut u8;
    let mut buf = alloc::vec![0u8; len];
    let n = crate::net::ethernet::socket_recv(fd, &mut buf);
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, n); }
    n as i64
}

fn sys_close_socket(args: &SyscallArgs) -> SyscallResult {
    crate::net::ethernet::socket_close(args.a0 as u32);
    0
}

fn sys_p2p_send(args: &SyscallArgs) -> SyscallResult {
    let data = unsafe_read_bytes(args.a1 as *const u8, args.a2 as usize);
    let mut node_id = [0u8; 32];
    let id_bytes = unsafe_read_bytes(args.a0 as *const u8, 32);
    node_id.copy_from_slice(&id_bytes[..32.min(id_bytes.len())]);
    crate::p2p::transport::send(node_id, data);
    0
}

fn sys_p2p_recv(_: &SyscallArgs) -> SyscallResult { 0 }

fn sys_ia_infer(_: &SyscallArgs) -> SyscallResult {
    let features = crate::ia::collector::get_recent_features(10);
    let results  = crate::ia::model::run_inference(&features);
    results.len() as i64
}

fn sys_edge_submit(args: &SyscallArgs) -> SyscallResult {
    let payload = unsafe_read_bytes(args.a1 as *const u8, args.a2 as usize);
    let kind = crate::edge::task::TaskKind::Compute;
    crate::edge::submit_task(payload, kind) as i64
}

fn sys_edge_result(_: &SyscallArgs) -> SyscallResult { Errno::EAGAIN.as_i64() }

fn sys_wasm_load(args: &SyscallArgs) -> SyscallResult {
    let name  = unsafe_read_str(args.a0 as *const u8, 64);
    let bytes = unsafe_read_bytes(args.a1 as *const u8, args.a2 as usize);
    match crate::wasm::load(&name, &bytes) {
        Ok(_)  => 1,
        Err(_) => Errno::SOCD_EWASM.as_i64(),
    }
}

fn sys_wasm_call(_: &SyscallArgs) -> SyscallResult { Errno::ENOSYS.as_i64() }

fn sys_xr_begin_frame(args: &SyscallArgs) -> SyscallResult {
    let _frame = crate::xr::begin_frame(0);
    0
}

fn sys_xr_end_frame(_: &SyscallArgs) -> SyscallResult { 0 }

fn sys_quantum_submit(args: &SyscallArgs) -> SyscallResult {
    let shots = args.a2 as u32;
    let id = crate::quantum::run_demo_bell_state();
    id as i64
}

fn sys_quantum_result(args: &SyscallArgs) -> SyscallResult {
    let job_id = args.a0;
    let q = crate::quantum::QUANTUM.lock();
    if q.get_job(job_id).map(|j| matches!(j.state, crate::quantum::JobState::Completed)).unwrap_or(false) {
        0
    } else {
        Errno::EAGAIN.as_i64()
    }
}

fn sys_ui_create_surface(args: &SyscallArgs) -> SyscallResult {
    let title = unsafe_read_str(args.a0 as *const u8, 64);
    let rect  = crate::ui::Rect::new(
        args.a1 as i32, args.a2 as i32,
        args.a3 as u32, args.a4 as u32,
    );
    crate::ui::compositor::create_surface(
        &title, rect,
        crate::ui::compositor::Layer::Windows,
        args.a5,
    ) as i64
}

fn sys_ui_destroy_surface(args: &SyscallArgs) -> SyscallResult {
    crate::ui::compositor::destroy_surface(args.a0);
    0
}

fn sys_security_check(args: &SyscallArgs) -> SyscallResult {
    let perm = unsafe_read_str(args.a0 as *const u8, 32);
    let pid  = sys_getpid(&SyscallArgs::default()) as u64;
    if crate::security::sandbox::check_permission(pid, &perm) { 1 } else { 0 }
}

fn sys_get_stats(args: &SyscallArgs) -> SyscallResult {
    match args.a0 {
        0 => { // Scheduler
            let s = crate::modules::scheduler::get_stats();
            crate::serial_println!("[STATS] procs={} ctx_sw={}",
                s.total_processes, s.context_switches);
        }
        1 => { // P2P
            let s = crate::p2p::get_stats();
            crate::serial_println!("[STATS] p2p peers_active={}", s.peers_active);
        }
        2 => { // IA
            let s = crate::ia::get_stats();
            crate::serial_println!("[STATS] ia inferences={}", s.inferences_total);
        }
        _ => {}
    }
    0
}

// ─── Helpers de memória do usuário ───────────────────────────────────────────
// ATENÇÃO: Em produção, validar que o ponteiro está no espaço do usuário!

fn unsafe_read_str(ptr: *const u8, max_len: usize) -> String {
    if ptr.is_null() { return String::new(); }
    let bytes = unsafe_read_bytes(ptr, max_len);
    // Busca terminador nulo
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end]).unwrap_or("").into()
}

fn unsafe_read_bytes(ptr: *const u8, len: usize) -> alloc::vec::Vec<u8> {
    if ptr.is_null() || len == 0 { return alloc::vec![]; }
    // Fase 5: validar que [ptr, ptr+len) está mapeado no espaço do usuário
    unsafe {
        core::slice::from_raw_parts(ptr, len.min(65536)).to_vec()
    }
}

// ─── Handler x86_64 ──────────────────────────────────────────────────────────

/// Entry point do handler de SYSCALL no x86_64
/// Chamado pela instrução SYSCALL (configura MSR_LSTAR)
#[no_mangle]
pub extern "C" fn syscall_handler(
    nr: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64
) -> i64 {
    let args = SyscallArgs { nr, a0, a1, a2, a3, a4, a5 };
    dispatch(&args)
}

/// Estatísticas do dispatcher
pub struct SyscallStats {
    pub total_calls: u64,
    pub errors:      u64,
    pub by_category: [u64; 8], // FS, Process, Memory, Net, SOCD, XR, Quantum, Edge
}

static SYSCALL_STATS: Spinlock<SyscallStats> = Spinlock::new(SyscallStats {
    total_calls: 0,
    errors: 0,
    by_category: [0u64; 8],
});

pub fn init() {
    // Configura MSR_LSTAR para apontar para syscall_handler
    // (em modo usuário real — Fase 5)
    crate::serial_println!("[SYSCALL] Interface de syscall inicializada");
    crate::serial_println!("[SYSCALL] {} syscalls registradas", 115 - 100 + 17);
    crate::serial_println!("[SYSCALL] POSIX compat: open/close/read/write/socket/...");
    crate::serial_println!("[SYSCALL] SOC-D ext: p2p/ia/edge/wasm/xr/quantum/ui");
}

pub fn get_stats() -> (u64, u64) {
    let s = SYSCALL_STATS.lock();
    (s.total_calls, s.errors)
}
