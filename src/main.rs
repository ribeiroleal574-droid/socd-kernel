#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
// ============================================================
// SOC-D Kernel — Entry Point
// Sistema Operacional Cognitivo Distribuído
// ============================================================

#![no_std]
#![no_main]

#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

// ─── Módulos do Kernel ────────────────────────────────────────
mod arch;
mod memory;
mod modules;
mod security;
mod drivers;
mod p2p;
mod ia;
mod ui;
mod edge;
mod wasm;
mod xr;
mod quantum;
mod net;
mod crypto;  // Criptografia real: SHA-256 + Ed25519 + HMAC-SHA256
mod getrandom_impl; // Entropia para ed25519-dalek em no_std
mod syscall;
// NOTE: Removidas as declarações duplicadas de `mod p2p`, `mod ia` e
//       `mod syscall` que existiam no final do ficheiro original.

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    // ── 1: Serial (debug) ─────────────────────────────────────
    drivers::serial::init();
    serial_println!("[SOC-D] Boot iniciado...");

    // ── 2: VGA + Banner ───────────────────────────────────────
    drivers::vga::init();
    print_banner();

    // ── 3: GDT ────────────────────────────────────────────────
    arch::gdt::init();
    serial_println!("[OK] GDT inicializada");

    // ── 4: IDT ────────────────────────────────────────────────
    arch::interrupts::init_idt();
    serial_println!("[OK] IDT configurada");

    // ── 5: PIC + interrupts ───────────────────────────────────
    unsafe { arch::interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
    serial_println!("[OK] Interrupcoes habilitadas");

    // ── 6: Memory / Heap ──────────────────────────────────────
    let phys_mem_offset = x86_64::VirtAddr::new(boot_info.physical_memory_offset);
    memory::set_phys_mem_offset(phys_mem_offset.as_u64());
    let mut mapper = unsafe { memory::paging::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        memory::frame_allocator::BootInfoFrameAllocator::init(&boot_info.memory_map)
    };
    memory::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("[ERRO] Falha ao inicializar heap do kernel");
    serial_println!("[OK] Memoria e heap inicializados ({} KB heap)",
        memory::heap::HEAP_SIZE / 1024);

    // ── 6b: Criptografia Real (precisa do heap para o backend
    //        `alloc` do curve25519-dalek/ed25519-dalek) ────────
    crypto::init();
    serial_println!("[OK] Criptografia real: SHA-256 + Ed25519 + HMAC");

    // ── 7: Modules ────────────────────────────────────────────
    modules::registry::init();
    serial_println!("[OK] Registro de modulos ativo");
    modules::load_builtin_modules();
    serial_println!("[OK] Modulos essenciais carregados");

    // ── 8: Security ───────────────────────────────────────────
    security::sandbox::init();
    serial_println!("[OK] Sandbox de seguranca ativo");

    // ── 8b: IA Defensiva (Fase 3.3) ──────────────────────────
    security::threat::init();
    serial_println!("[OK] IA defensiva ativa");

    // ── 8c: Disco virtio-blk (Fase 8 — TmpFS em disco) ───────
    // Tem de vir ANTES do TmpFS, para o snapshot poder ser
    // carregado do disco já na inicialização. Se não houver disco
    // associado no QEMU (-drive + -device virtio-blk-pci), fica
    // graciosamente sem persistência — TmpFS funciona só em RAM
    // como antes.
    drivers::virtio_blk::init_physical();

    // ── 9: TmpFS ──────────────────────────────────────────────
    modules::tmpfs::init();
    serial_println!("[OK] TmpFS inicializado");

    // ── 10: Scheduler ─────────────────────────────────────────
    modules::scheduler::init();
    // Regista esta própria execução (o resto do boot + kernel_loop)
    // como uma tarefa normal do scheduler, para que a troca de
    // contexto real possa preemptá-la e retomá-la tal como a qualquer
    // outra tarefa (ver arch::context_switch e o comando 'ps').
    modules::scheduler::register_current_as_task("kernel_main", modules::scheduler::Priority::Normal);
    serial_println!("[OK] Scheduler preemptivo ativo (troca de contexto real)");

    // ── 11: Keyboard ──────────────────────────────────────────
    drivers::keyboard::init();
    serial_println!("[OK] Driver de teclado pronto");

    // ── 12: P2P ───────────────────────────────────────────────
    p2p::init();
    serial_println!("[OK] Rede P2P inicializada");

    // ── 12b: DAG + Sync P2P (Fase 3) ─────────────────────────
    p2p::dag::init();
    p2p::dag::run_demo();
    serial_println!("[OK] DAG + Sync P2P ativo");

    // ── 13: IA ────────────────────────────────────────────────
    ia::init();
    serial_println!("[OK] Motor de IA ativo");

    // ── 14: UI ────────────────────────────────────────────────
    ui::init();
    serial_println!("[OK] Interface grafica ativa");

    // ── 15: Edge Computing ────────────────────────────────────
    edge::init();
    serial_println!("[OK] Edge computing ativo");

    // ── 16: WASM Runtime ──────────────────────────────────────
    wasm::init();
    serial_println!("[OK] WASM runtime ativo");

    // ── 17: OpenXR ────────────────────────────────────────────
    xr::init();
    serial_println!("[OK] OpenXR AR/VR ativo");

    // ── 18: Quantum ───────────────────────────────────────────
    quantum::init();
    quantum::run_demo_bell_state();
    serial_println!("[OK] Motor quantico ativo");

    // ── 19: Net stack ─────────────────────────────────────────
    net::init();
    // NOTA: net::virtio::init() removido — inicializava um driver
    // virtio-net paralelo 100% simulado (nunca ligado a hardware
    // real), que só confundia o boot com uma segunda mensagem
    // "virtio-net inicializado" antes da Fase 7 (driver real, ver
    // net::virtio_real) mais abaixo.
    net::ethernet::init();
    serial_println!("[OK] Stack de rede ativa");

    // ── 20: Syscall ───────────────────────────────────────────
    syscall::init();
    serial_println!("[OK] Syscall interface ativa");

    // ── 21: Fase 2 — Gestor de processos dinâmicos ───────────
    modules::process::init();
    modules::process::exec_demo();
    serial_println!("[OK] Fase 2: processos dinamicos ativos");

    // ── 22: Fase 3.2 — Virtualização leve ────────────────────
    modules::virt::init();
    modules::virt::run_demo();
    serial_println!("[OK] Fase 3.2: containers ativos");

    // ── 23: Fase 3.3 — IA Defensiva ──────────────────────────
    security::threat::run_demo();
    serial_println!("[OK] Fase 3.3: IA defensiva operacional");

    // ── 24: Fase 3.4 — Cross-Device ──────────────────────────
    modules::xdev::init();
    modules::xdev::run_demo();
    serial_println!("[OK] Fase 3.4: cluster cross-device ativo");

    // ── 25: Fase 4.1 — UI Mobile Adaptativa ──────────────────
    ui::mobile::init();
    ui::mobile::run_demo();
    serial_println!("[OK] Fase 4.1: UI mobile adaptativa ativa");

    // ── 26: Fase 4.2 — Interface Holográfica AR ───────────────
    ui::ar::init();
    ui::ar::run_demo();
    serial_println!("[OK] Fase 4.2: interface holografica AR ativa");

    // ── 27: Fase 5 — Motor Cognitivo ─────────────────────────
    ia::cognitive::init();
    ia::cognitive::run_demo();
    serial_println!("[OK] Fase 5: motor cognitivo ativo");

    // ── 28: Fase 6.1 — Monitor de Recursos ───────────────────
    modules::monitor::init();
    modules::monitor::run_demo();
    serial_println!("[OK] Fase 6.1: monitor de recursos ativo");

    // ── 29: Fase 6.2 — Assinaturas DAG ───────────────────────
    p2p::dag_sig::init();
    p2p::dag_sig::run_demo();
    serial_println!("[OK] Fase 6.2: DAG criptografico ativo");

    // ── 30: Fase 6.3 — Suite de Testes ───────────────────────
    modules::tests::init();
    modules::tests::run_all();
    let (pass, fail, skip) = modules::tests::get_summary();
    serial_println!("[OK] Fase 6.3: testes {} pass / {} fail / {} skip",
        pass, fail, skip);

    // ── 31: Fase 7 — Driver de Rede Físico ───────────────────
    net::virtio_real::run_demo();
    serial_println!("[OK] Fase 7: driver de rede fisico inicializado");

    // ── Boot completo! ────────────────────────────────────────
    serial_println!("\n[SOC-D] Kernel v{} pronto.\n", env!("CARGO_PKG_VERSION"));

    // ── Dashboard VGA (janela QEMU) ───────────────────────────
    drivers::vga_dashboard::draw();

    // ── Shell serial interativo ───────────────────────────────
    drivers::serial_shell::print_welcome();

    kernel_loop()
}

fn kernel_loop() -> ! {
    // Começa o tick a partir do estado atual do scheduler
    // para evitar overflow quando cognitive.last_tick foi definido
    // durante o boot com valores altos
    let mut tick: u64 = crate::modules::scheduler::get_stats().current_tick;
    let mut last_bg_tick: u64 = 0;
    loop {
        drivers::serial_shell::tick();
        // Motores de background chamados a cada iteração do loop
        ia::cognitive::cognitive_tick(tick);   // Cognitivo: a cada 60 ticks
        p2p::dag::sync_tick(tick);             // DAG sync: a cada 300 ticks
        security::threat::threat_tick(tick);   // Threat: a cada 120 ticks
        ui::ar::ar_tick(tick);                 // AR: expira holograms
        modules::virt::virt_tick(tick);        // Containers: limites recursos
        modules::monitor::monitor_tick(tick);  // Monitor: captura a cada 60 ticks

        // Trabalho periódico que ANTES corria dentro do handler do
        // timer (interrupts.rs). Movido para aqui — contexto de
        // tarefa normal — porque correr código com Spinlocks próprios
        // directamente numa interrupção de hardware é perigoso assim
        // que há troca de contexto real: se uma tarefa estiver a
        // meio de segurar um desses locks quando o timer dispara, a
        // interrupção fica presa à espera de um lock que nunca mais
        // é libertado (deadlock). Usa o tick real do hardware
        // (arch::interrupts::current_tick), não o contador local do
        // loop, para manter a mesma cadência de antes.
        let real_tick = arch::interrupts::current_tick();
        if real_tick != last_bg_tick {
            last_bg_tick = real_tick;
            if real_tick % 1000 == 0 {
                p2p::discovery::tick(real_tick);
                p2p::gossip::tick(real_tick);
                ia::tick(real_tick);
                edge::tick(real_tick);
            }
            if real_tick % 16 == 0 {
                ui::shell::tick(real_tick);
            }
            if real_tick % 20 == 0 {
                // Ponto único de sondagem/distribuição da rede real —
                // ver net::poll_and_dispatch (não pode haver mais do
                // que um sítio a drenar virtio_real::receive()).
                net::poll_and_dispatch();
            }
        }

        tick = tick.saturating_add(1);
        x86_64::instructions::hlt();
    }
}

fn print_banner() {
    println!("╔══════════════════════════════════════════════════╗");
    println!("║         SOC-D — Kernel v{}                    ║", env!("CARGO_PKG_VERSION"));
    println!("║  Sistema Operacional Cognitivo Distribuido       ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Desativa interrupções imediatamente — impede que o scheduler,
    // optimizer e outros handlers continuem a executar após o panic
    x86_64::instructions::interrupts::disable();
    println!("\n[KERNEL PANIC] {}", info);
    serial_println!("[KERNEL PANIC] {}", info);
    serial_println!("[KERNEL PANIC] Sistema parado. Reinicie o QEMU.");
    loop {
        x86_64::instructions::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("Falha de alocacao: {:?}", layout)
}
