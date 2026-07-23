// SOC-D Kernel — Driver PS/2 + Shell de Debug
extern crate alloc;
use alloc::{string::{String, ToString}, vec::Vec};
use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spinning_top::Spinlock;

// pc-keyboard 0.5: Keyboard::new(layout, scancodeSet, HandleControl)
lazy_static! {
    static ref KEYBOARD: Spinlock<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Spinlock::new(Keyboard::new(
            layouts::Us104Key,
            ScancodeSet1,
            HandleControl::Ignore,
        ));
}

static CMD_BUFFER: Spinlock<String> = Spinlock::new(String::new());

pub fn init() {
    crate::serial_println!("[KB] Driver PS/2 pronto");
}

pub fn handle_scancode(scancode: u8) {
    let mut keyboard = KEYBOARD.lock();
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(c) => handle_character(c),
                DecodedKey::RawKey(_)  => {}
            }
        }
    }
}

fn handle_character(c: char) {
    match c {
        '\n' => {
            crate::print!("\n");
            let cmd = CMD_BUFFER.lock().clone();
            CMD_BUFFER.lock().clear();
            execute_command(cmd.trim());
            crate::print!("> ");
        }
        '\x08' => {
            let mut buf = CMD_BUFFER.lock();
            if buf.pop().is_some() {
                crate::print!("\x08 \x08");
            }
        }
        c => {
            let mut buf = CMD_BUFFER.lock();
            if buf.len() < 128 {
                buf.push(c);
                crate::print!("{}", c);
            }
        }
    }
}

/// Executa um comando vindo do teclado PS/2 (output via println!)
pub fn execute_command_serial(cmd: &str) {
    execute_command(cmd);
}

fn execute_command(cmd: &str) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() { return; }
    match parts[0] {
        "help"            => cmd_help(),
        "version"         => cmd_version(),
        "status"          => cmd_status(),
        "mem"             => cmd_memory(),
        "sandbox" | "sec" => cmd_sandbox(),
        "ps"              => cmd_ps(),
        "sched"           => cmd_sched(),
        "ls"              => cmd_ls(parts.get(1).copied()),
        "cat"             => cmd_cat(parts.get(1).copied()),
        "exec"            => cmd_exec(parts.get(1).copied()),
        "kill"            => cmd_kill(parts.get(1).copied()),
        "dag"             => cmd_dag(parts.get(1).copied()),
        "sync"            => cmd_sync(),
        "ct"              => cmd_ct(parts.get(1).copied(), parts.get(2).copied()),
        "threat"          => cmd_threat(),
        "privacy"         => cmd_privacy(parts.get(1).copied()),
        "devices"         => cmd_devices(),
        "handoff"         => cmd_handoff(parts.get(1).copied(), parts.get(2).copied()),
        "clipboard"       => cmd_clipboard(parts.get(1).copied()),
        "mobile"          => cmd_mobile(parts.get(1).copied()),
        "theme"           => cmd_theme(parts.get(1).copied()),
        "ar"              => cmd_ar(),
        "cogn"            => cmd_cogn(parts.get(1).copied(), parts.get(2).copied()),
        "monitor"         => cmd_monitor(),
        "top"             => cmd_top(),
        "test"            => cmd_test(parts.get(1).copied()),
        "pci"             => cmd_pci(),
        "p2p"             => cmd_p2p(),
        "peers"           => cmd_peers(),
        "ia"              => cmd_ia(),
        "suggest"         => cmd_suggest(),
        "edge"            => cmd_edge(),
        "wasm"            => cmd_wasm(),
        "xr"              => cmd_xr(),
        "quantum"         => cmd_quantum(),
        "net"             => cmd_net(),
        "syscall"         => cmd_syscall(),
        "ui"              => cmd_ui(),
        "arm"             => cmd_arm(),
        "clear"           => cmd_clear(),
        "reboot"          => cmd_reboot(),
        "" => {}
        u  => { crate::println!("[ERRO] '{}' desconhecido. Digite 'help'", u); }
    }
}

fn cmd_help() {
    crate::println!("┌──────────────────────────────────────────────────┐");
    crate::println!("│           SOC-D Shell — Comandos                 │");
    crate::println!("├──────────────┬───────────────────────────────────┤");
    crate::println!("│ Sistema      │ help version status mem ps        │");
    crate::println!("│              │ sched ls cat modules sandbox      │");
    crate::println!("├──────────────┼───────────────────────────────────┤");
    crate::println!("│ Processos    │ exec [demo|<elf>]  kill <pid>     │");
    crate::println!("│ Containers   │ ct [ls|stop|rm|stats] [id]        │");
    crate::println!("├──────────────┼───────────────────────────────────┤");
    crate::println!("│ DAG/Sync     │ dag [ls|stats|/path]  sync        │");
    crate::println!("│ Cross-Device │ devices  handoff <s> <d>          │");
    crate::println!("│              │ clipboard [texto]                  │");
    crate::println!("├──────────────┼───────────────────────────────────┤");
    crate::println!("│ Seguranca    │ threat  privacy <nivel>           │");
    crate::println!("│ Rede         │ net  p2p  peers                   │");
    crate::println!("├──────────────┼───────────────────────────────────┤");
    crate::println!("│ Subsistemas  │ ia suggest edge wasm xr           │");
    crate::println!("│              │ quantum syscall ui arm            │");
    crate::println!("├──────────────┼───────────────────────────────────┤");
    crate::println!("│ Terminal     │ clear  reboot                     │");
    crate::println!("└──────────────┴───────────────────────────────────┘");
}
fn cmd_version() {
    crate::println!("SOC-D Kernel v{}", env!("CARGO_PKG_VERSION"));
    crate::println!("Sistema Operacional Cognitivo Distribuido");
}
fn cmd_status() {
    use crate::modules::registry::REGISTRY;
    let reg = REGISTRY.lock();
    let s = reg.stats();
    crate::println!("Modulos: {} total | {} ativos | {} falhos", s.total, s.active, s.failed);
    for m in reg.active_modules() {
        crate::println!("  [OK] {} v{}", m.name, m.status.version);
    }
}
fn cmd_memory() {
    use crate::memory::heap::{heap_stats, HEAP_SIZE};
    let (used, free) = heap_stats();
    crate::println!("Heap: {} KB total | {} KB usado | {} KB livre",
        HEAP_SIZE/1024, used/1024, free/1024);
}
fn cmd_sandbox() {
    let s = crate::security::sandbox::get_stats();
    crate::println!("Sandbox: {} ativos | {} violacoes | {} risco",
        s.active_sandboxes, s.total_violations, s.high_risk_processes);
}
fn cmd_ps() {
    let procs = crate::modules::scheduler::list_processes();
    crate::println!("PID  NOME              ESTADO          PRIO");
    crate::println!("────────────────────────────────────────────");
    for p in &procs {
        crate::println!("{:<5}{:<18}{:<16}{:?}", p.pid, p.name, p.state, p.priority);
    }
    crate::println!("{} processo(s)", procs.len());
}
fn cmd_sched() {
    let s = crate::modules::scheduler::get_stats();
    crate::println!("Tick: {} | CTX switches: {} | Procs: {}",
        s.current_tick, s.context_switches, s.total_processes);
    crate::println!("  Rodando:{} Prontos:{} Bloqueados:{} Dormindo:{}",
        s.running, s.ready, s.blocked, s.sleeping);
}
fn cmd_ls(path: Option<&str>) {
    match crate::modules::tmpfs::ls(path.unwrap_or("/")) {
        Ok(entries) => {
            crate::println!("{}:", path.unwrap_or("/"));
            for (name, id) in &entries {
                crate::println!("  [{}] {}", id, name);
            }
        }
        Err(e) => crate::println!("[ERRO] {}", e),
    }
}
fn cmd_cat(path: Option<&str>) {
    let p = match path { Some(p) => p, None => { crate::println!("Uso: cat <path>"); return; } };
    match crate::modules::tmpfs::read(p) {
        Ok(data) => {
            if let Ok(s) = core::str::from_utf8(&data) { crate::print!("{}", s); }
            else { crate::println!("[BINARIO {} bytes]", data.len()); }
        }
        Err(e) => crate::println!("[ERRO] {}", e),
    }
}
fn cmd_modules() {
    use crate::modules::elf_loader::ELF_MANAGER;
    let m = ELF_MANAGER.lock();
    let list = m.list();
    if list.is_empty() { crate::println!("Nenhum modulo ELF externo carregado."); }
    else {
        for (name, base, size) in &list {
            crate::println!("  0x{:016x}  {} KB  {}", base, size/1024, name);
        }
    }
}
fn cmd_exec(arg: Option<&str>) {
    use crate::modules::process;
    match arg {
        None => {
            crate::println!("Uso: exec <nome>");
            crate::println!("     exec demo   -- lanca tarefas de demonstracao");
            crate::println!("Nota: para ELF externo, carregue via TmpFS e use exec <path>");
        }
        Some("demo") => {
            process::exec_demo();
            let procs = process::list_dynamic();
            crate::println!("Processos dinamicos ativos: {}", procs.len());
            for p in &procs {
                crate::println!("  PID={} '{}' entry=0x{:x}",
                    p.pid, p.name, p.entry);
            }
        }
        Some(name) => {
            // Tenta carregar do TmpFS
            match crate::modules::tmpfs::read(name) {
                Ok(data) => {
                    match process::exec_elf(name, &data) {
                        Ok(pid) => crate::println!("[OK] '{}' carregado PID={}", name, pid),
                        Err(e)  => crate::println!("[ERRO] exec '{}': {}", name, e),
                    }
                }
                Err(_) => {
                    crate::println!("[ERRO] '{}' nao encontrado no TmpFS.", name);
                    crate::println!("Copie o ELF para o TmpFS primeiro.");
                }
            }
        }
    }
}
fn cmd_kill(arg: Option<&str>) {
    match arg {
        None => { crate::println!("Uso: kill <pid>"); }
        Some(s) => {
            match s.parse::<u64>() {
                Ok(pid) => {
                    if crate::modules::process::kill(pid) {
                        crate::println!("[OK] PID={} terminado.", pid);
                    } else {
                        crate::println!("[ERRO] PID={} nao encontrado.", pid);
                    }
                }
                Err(_) => crate::println!("[ERRO] PID invalido: '{}'", s),
            }
        }
    }
}
fn cmd_dag(arg: Option<&str>) {
    use crate::p2p::dag;
    match arg {
        None | Some("ls") => {
            let paths = dag::list();
            let s = dag::stats();
            crate::println!("DAG: {} blocos | {} ficheiros | {} merges",
                s.total_blocks, s.file_blocks, s.merge_count);
            if paths.is_empty() {
                crate::println!("  (vazio)");
            } else {
                for p in &paths {
                    crate::println!("  {}", p);
                }
            }
        }
        Some("stats") => {
            let s = dag::stats();
            crate::println!("Blocos totais:      {}", s.total_blocks);
            crate::println!("Blocos ficheiro:    {}", s.file_blocks);
            crate::println!("Blocos sync:        {}", s.sync_blocks);
            crate::println!("Merges:             {}", s.merge_count);
            crate::println!("Conflitos resolvidos:{}", s.conflicts_resolved);
        }
        Some("verify") => {
            use crate::p2p::dag_sig;
            let (ok, fail, untrusted) = dag_sig::stats();
            crate::println!("DAG Cadeia de Confianca:");
            crate::println!("  Blocos verificados OK:    {}", ok);
            crate::println!("  Blocos rejeitados:        {}", fail);
            crate::println!("  Autores nao verificados:  {}", untrusted);
            let chain = dag_sig::TRUST_CHAIN.lock();
            crate::println!("  Chaves confiadas:         {}", chain.trusted_key_count());
        }
        Some(path) if path.starts_with('/') => {
            let hist = dag::history(path);
            if hist.is_empty() {
                crate::println!("Sem historico para '{}'", path);
            } else {
                crate::println!("Historico '{}': {} versoes", path, hist.len());
                for (seq, hash) in &hist {
                    crate::println!("  v{} hash={}", seq, hash);
                }
            }
        }
        Some(_) => {
            crate::println!("dag ls          -- lista paths");
            crate::println!("dag stats       -- estatisticas");
            crate::println!("dag verify      -- cadeia de confianca");
            crate::println!("dag /path       -- historico de versoes");
        }
    }
}
fn cmd_sync() {
    use crate::p2p::dag;
    let tick = crate::modules::scheduler::get_stats().current_tick;
    dag::sync_tick(tick);
    let s = dag::stats();
    crate::println!("[SYNC] DAG: {} blocos | {} peers conectados",
        s.total_blocks,
        crate::p2p::get_stats().peers_active);
    crate::println!("[SYNC] Blocos propagados via Gossip P2P");
}
fn cmd_ct(sub: Option<&str>, arg: Option<&str>) {
    use crate::modules::virt;
    match sub {
        None | Some("ls") => {
            let s = virt::stats();
            crate::println!("Containers: {} total | {} running | {} paused | {} stopped",
                s.total, s.running, s.paused, s.stopped);
            for c in virt::list() {
                crate::println!("  [{}] {} | {} | {} | pids={:?}",
                    c.id, c.name, c.runtime, c.state, c.pids);
            }
        }
        Some("stop") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if virt::stop(id) { crate::println!("[OK] Container {} parado.", id); }
                    else { crate::println!("[ERRO] Container {} nao encontrado.", id); }
                }
                None => crate::println!("Uso: ct stop <id>"),
            }
        }
        Some("rm") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if virt::remove(id) { crate::println!("[OK] Container {} removido.", id); }
                    else { crate::println!("[ERRO] Container {} nao encontrado ou ainda a correr.", id); }
                }
                None => crate::println!("Uso: ct rm <id>"),
            }
        }
        Some("stats") => {
            let s = virt::stats();
            crate::println!("Total:   {}", s.total);
            crate::println!("Running: {}", s.running);
            crate::println!("Paused:  {}", s.paused);
            crate::println!("Stopped: {}", s.stopped);
        }
        Some(_) => {
            crate::println!("ct ls           -- lista containers");
            crate::println!("ct stats        -- estatisticas");
            crate::println!("ct stop <id>    -- para container");
            crate::println!("ct rm <id>      -- remove container parado");
        }
    }
}
fn cmd_pci() {
    use crate::net::virtio_real;
    crate::println!("PCI Bus Scan:");
    crate::println!("{:<8} {:<8} {:<8} {:<8} {:<6}", "Bus:Dev", "Vendor", "Device", "Class", "BAR0");
    crate::println!("{}", "─".repeat(44));
    let devices = virtio_real::list_pci_devices();
    if devices.is_empty() {
        crate::println!("  Nenhum device PCI encontrado.");
    }
    for d in &devices {
        let name = match (d.vendor, d.device) {
            (0x1AF4, 0x1000) => "virtio-net",
            (0x1AF4, 0x1001) => "virtio-blk",
            (0x1AF4, 0x1050) => "virtio-gpu",
            (0x8086, _)      => "Intel",
            (0x10DE, _)      => "NVIDIA",
            (0x1234, 0x1111) => "QEMU VGA",
            _                => "?",
        };
        crate::println!("{:02x}:{:02x}     {:04x}    {:04x}    {:02x}:{:02x}  0x{:08x}  {}",
            d.bus, d.dev, d.vendor, d.device,
            d.class, d.subclass, d.bar0, name);
    }
    crate::println!("");
    // virtio-net real status
    let real = crate::net::virtio_real::VIRTIO_REAL.lock();
    if real.initialized {
        crate::println!("virtio-net PCI real: ATIVO");
        crate::println!("  MAC:  {}", real.mac_string());
        crate::println!("  Link: {}", if real.link_up { "UP" } else { "DOWN" });
        let (tx_p, rx_p, tx_b, rx_b) = (real.tx_packets, real.rx_packets,
                                          real.tx_bytes,   real.rx_bytes);
        crate::println!("  TX:   {} pkts / {} bytes", tx_p, tx_b);
        crate::println!("  RX:   {} pkts / {} bytes", rx_p, rx_b);
    } else {
        crate::println!("virtio-net PCI real: NAO DISPONIVEL");
        crate::println!("  Adicionar ao QEMU:");
        crate::println!("  -netdev user,id=net0 -device virtio-net-pci,netdev=net0");
    }
}
fn cmd_test(arg: Option<&str>) {
    use crate::modules::tests;
    match arg {
        None | Some("run") => {
            crate::println!("A executar suite de testes...");
            tests::run_all();
            let (pass, fail, skip) = tests::get_summary();
            crate::println!("Resultado: {} pass | {} fail | {} skip",
                pass, fail, skip);
        }
        Some("status") => {
            let (pass, fail, skip) = tests::get_summary();
            if pass == 0 && fail == 0 {
                crate::println!("Testes ainda nao executados. Use 'test run'.");
            } else {
                crate::println!("Ultimo resultado: {} pass | {} fail | {} skip",
                    pass, fail, skip);
                if fail == 0 {
                    crate::println!("TODOS OS TESTES PASSARAM.");
                } else {
                    crate::println!("{} TESTES FALHARAM.", fail);
                }
            }
        }
        Some(_) => {
            crate::println!("test run    -- executa todos os 46 testes");
            crate::println!("test status -- mostra resultado do ultimo run");
        }
    }
}
fn cmd_monitor() {
    use crate::modules::monitor;
    // Força captura imediata
    let tick = crate::modules::scheduler::get_stats().current_tick;
    {
        let mut m = monitor::MONITOR.lock();
        m.last_capture = 0;
        m.tick(tick);
    }
    let rep = monitor::report();
    crate::println!("{}", rep);
    let alerts = monitor::active_alerts();
    if !alerts.is_empty() {
        crate::println!("ALERTAS ATIVOS: {}", alerts.len());
        for a in &alerts {
            crate::println!("  [{}] {}", a.kind.as_str(), a.message);
        }
    }
}
fn cmd_top() {
    use crate::modules::scheduler;
    let procs = scheduler::list_processes();
    let stats = scheduler::get_stats();
    crate::println!("SOC-D top — {} processos | tick={}", procs.len(), stats.current_tick);
    crate::println!("{:<6} {:<18} {:<10} {:<8} {:<10}",
        "PID", "Nome", "Estado", "Prioridade", "CPU ticks");
    crate::println!("{}", "─".repeat(56));
    // Ordena por cpu_ticks decrescente
    let mut sorted = procs.clone();
    sorted.sort_by(|a, b| b.cpu_ticks.cmp(&a.cpu_ticks));
    for p in sorted.iter().take(16) {
        crate::println!("{:<6} {:<18} {:<10} {:<8} {:<10}",
            p.pid, p.name, p.state, alloc::format!("{:?}", p.priority), p.cpu_ticks);
    }
    if sorted.len() > 16 {
        crate::println!("  ... e mais {} processos", sorted.len() - 16);
    }
    // Resumo heap
    let (used, free) = crate::memory::heap::heap_stats();
    crate::println!("");
    crate::println!("Heap: {} usado / {} livre / {} total",
        crate::modules::monitor::ResourceSnapshot::fmt_bytes(used),
        crate::modules::monitor::ResourceSnapshot::fmt_bytes(free),
        crate::modules::monitor::ResourceSnapshot::fmt_bytes(crate::memory::heap::HEAP_SIZE));
}
fn cmd_cogn(sub: Option<&str>, arg: Option<&str>) {
    use crate::ia::cognitive;
    match sub {
        None | Some("status") => {
            let s = cognitive::stats();
            let engine = cognitive::COGNITIVE.lock();
            crate::println!("┌──────────────────────────────────────────────┐");
            crate::println!("│         Motor Cognitivo SOC-D                │");
            crate::println!("├──────────────────────────────────────────────┤");
            crate::println!("│ Ciclos executados:  {:>8}                │", s.cycles_run);
            crate::println!("│ Padroes match:      {:>8}                │", s.patterns_matched);
            crate::println!("│ Acoes executadas:   {:>8}                │", s.actions_executed);
            crate::println!("│ Sugestoes feitas:   {:>8}                │", s.suggestions_made);
            crate::println!("│ Episodios memoria:  {:>8}                │", s.episodes_stored);
            crate::println!("├──────────────────────────────────────────────┤");
            crate::println!("│ Knowledge Graph: {} nos / {} arestas         │",
                engine.knowledge.node_count(),
                engine.knowledge.edge_count());
            crate::println!("├──────────────────────────────────────────────┤");
            crate::println!("│ Padroes registados:                          │");
            for p in &engine.patterns {
                crate::println!("│  [{}] '{}' conf={:.0}% {}             │",
                    p.id, p.name,
                    p.confidence * 100.0,
                    if p.approved { "AUTO" } else { "PENDENTE" });
            }
            crate::println!("└──────────────────────────────────────────────┘");
        }
        Some("approve") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if cognitive::approve(id) {
                        crate::println!("[OK] Padrao {} aprovado para auto-execucao.", id);
                    } else {
                        crate::println!("[ERRO] Padrao {} nao encontrado.", id);
                    }
                }
                None => crate::println!("Uso: cogn approve <id>"),
            }
        }
        Some("log") => {
            let engine = cognitive::COGNITIVE.lock();
            let recent = engine.recent_actions(10);
            if recent.is_empty() {
                crate::println!("Nenhuma acao executada ainda.");
            } else {
                crate::println!("Ultimas acoes do motor cognitivo:");
                for (tick, pattern, action) in &recent {
                    crate::println!("  tick={} '{}' → {}", tick, pattern, action);
                }
            }
        }
        Some("tick") => {
            let tick = crate::modules::scheduler::get_stats().current_tick;
            cognitive::cognitive_tick(tick);
            crate::println!("[OK] Ciclo cognitivo executado (tick={}).", tick);
        }
        Some(_) => {
            crate::println!("cogn              -- estado do motor cognitivo");
            crate::println!("cogn approve <id> -- aprova padrao para auto-execucao");
            crate::println!("cogn log          -- historico de acoes");
            crate::println!("cogn tick         -- executa ciclo imediatamente");
        }
    }
}
fn cmd_mobile(arg: Option<&str>) {
    use crate::ui::mobile::{self, FormFactor, Theme};
    match arg {
        None => {
            let s = mobile::stats();
            let ui = mobile::MOBILE_UI.lock();
            crate::println!("UI Mobile Adaptativa:");
            crate::println!("  Form factor:  {}", ui.form_factor.as_str());
            let (w, h) = ui.form_factor.dimensions();
            crate::println!("  Resolucao:    {}x{}", w, h);
            crate::println!("  Touch:        {}", if ui.form_factor.is_touch() {"sim"} else {"nao"});
            crate::println!("  Tema:         {:?}", ui.theme);
            crate::println!("  Layouts:      {}", s.layouts_computed);
            crate::println!("  Gestos:       {}", s.gestures_handled);
        }
        Some("desktop") => mobile::adapt(FormFactor::Desktop { width: 1024, height: 768 }),
        Some("mobile")  => mobile::adapt(FormFactor::Mobile  { width: 1080, height: 2340, portrait: true }),
        Some("tablet")  => mobile::adapt(FormFactor::Tablet  { width: 2048, height: 1536, portrait: false }),
        Some("tv")      => mobile::adapt(FormFactor::Tv      { width: 3840, height: 2160 }),
        Some("ar")      => mobile::adapt(FormFactor::Ar),
        Some("vr")      => mobile::adapt(FormFactor::Vr),
        Some(_) => {
            crate::println!("mobile [desktop|mobile|tablet|tv|ar|vr]");
            crate::println!("  Adapta a UI ao form factor especificado");
        }
    }
}
fn cmd_theme(arg: Option<&str>) {
    use crate::ui::mobile::{self, Theme};
    match arg {
        None => {
            let ui = mobile::MOBILE_UI.lock();
            crate::println!("Tema atual: {:?}", ui.theme);
            crate::println!("Disponiveis: dark | light | oled | ar");
        }
        Some("dark")  => { mobile::set_theme(Theme::Dark);          crate::println!("[OK] Tema: dark"); }
        Some("light") => { mobile::set_theme(Theme::Light);         crate::println!("[OK] Tema: light"); }
        Some("oled")  => { mobile::set_theme(Theme::Oled);          crate::println!("[OK] Tema: oled (preto puro)"); }
        Some("ar")    => { mobile::set_theme(Theme::ArTransparent); crate::println!("[OK] Tema: AR transparente"); }
        Some(t)       => crate::println!("[ERRO] Tema '{}' desconhecido", t),
    }
}
fn cmd_ar() {
    use crate::ui::ar;
    let s = ar::stats();
    let scene = ar::SPATIAL.lock();
    crate::println!("┌─────────────────────────────────────────┐");
    crate::println!("│       Interface Holografica AR           │");
    crate::println!("├─────────────────────────────────────────┤");
    crate::println!("│ Anchors criados:   {:>6}               │", s.anchors_created);
    crate::println!("│ Holograms ativos:  {:>6}               │", s.holograms_active);
    crate::println!("│ Activacoes gaze:   {:>6}               │", s.gaze_activations);
    crate::println!("│ Gestos mao:        {:>6}               │", s.gestures_processed);
    crate::println!("│ Frames renderiz.:  {:>6}               │", s.frames_rendered);
    crate::println!("├─────────────────────────────────────────┤");
    let focused = scene.gaze.focused_hologram;
    crate::println!("│ Foco gaze:  {:>28} │",
        focused.map(|id| alloc::format!("hologram id={}", id))
               .unwrap_or_else(|| "nenhum".to_string()));
    crate::println!("│ Dwell:      {:>4} / {:>4} ticks          │",
        scene.gaze.dwell_ticks, scene.gaze.dwell_threshold);
    crate::println!("└─────────────────────────────────────────┘");
    crate::println!("Holograms:");
    for h in &scene.holograms {
        let p = &h.local_pose.position;
        let focus = if h.gaze_focused { " [FOCO]" } else { "" };
        crate::println!("  [{}] ({:.1},{:.1},{:.1}) op={:.1}{}",
            h.id, p.x, p.y, p.z, h.opacity, focus);
    }
}
fn cmd_devices() {
    use crate::modules::xdev;
    let devs = xdev::online_devices();
    let s = xdev::stats();
    crate::println!("Cluster cross-device: {} dispositivos online", devs.len());
    crate::println!("{:<4} {:<14} {:<12} {:<18} {}", "ID", "Nome", "Tipo", "Resolucao", "IP");
    crate::println!("{}", "─".repeat(60));
    for (i, d) in devs.iter().enumerate() {
        let (rx, ry) = d.kind.default_resolution();
        let res = if rx > 0 { alloc::format!("{}x{}", rx, ry) }
                  else { "n/a".to_string() };
        let ip = alloc::format!("{}.{}.{}.{}",
            d.local_ip[0], d.local_ip[1], d.local_ip[2], d.local_ip[3]);
        crate::println!("{:<4} {:<14} {:<12} {:<18} {}", i+1, d.name, d.kind.as_str(), res, ip);
    }
    crate::println!("Sessoes: {} | Handoffs: {} | Clipboard syncs: {}",
        s.sessions_created, s.handoffs_done, s.clipboard_syncs);
}
fn cmd_handoff(sid_arg: Option<&str>, dev_arg: Option<&str>) {
    use crate::modules::xdev::{self, ClipboardContent};
    match (sid_arg, dev_arg) {
        (None, _) | (_, None) => {
            crate::println!("Uso: handoff <session_id> <device_index>");
            crate::println!("  Use 'devices' para ver dispositivos e IDs de sessao");
        }
        (Some(sid_str), Some(dev_str)) => {
            let sid = match sid_str.parse::<u64>() {
                Ok(v) => v,
                Err(_) => { crate::println!("[ERRO] session_id invalido"); return; }
            };
            let devs = xdev::online_devices();
            let idx: usize = match dev_str.parse::<usize>() {
                Ok(v) if v >= 1 && v <= devs.len() => v - 1,
                _ => { crate::println!("[ERRO] device_index invalido (use 'devices')"); return; }
            };
            let target = devs[idx].node_id;
            match xdev::handoff(sid, target) {
                Ok(()) => crate::println!("[OK] Sessao {} transferida para '{}'",
                    sid, devs[idx].name),
                Err(_) => crate::println!("[ERRO] Falha no handoff"),
            }
        }
    }
}
fn cmd_clipboard(arg: Option<&str>) {
    use crate::modules::xdev::{self, ClipboardContent};
    match arg {
        None => {
            let bus = xdev::XDEV.lock();
            crate::println!("Clipboard: {}", bus.clipboard.as_str());
            if let ClipboardContent::Text(ref t) = bus.clipboard {
                crate::println!("  \"{}\"", t);
            }
        }
        Some(text) => {
            xdev::clipboard_copy(ClipboardContent::Text(text.to_string()));
            crate::println!("[OK] Clipboard copiado e sincronizado via P2P");
        }
    }
}
fn cmd_threat() {
    use crate::security::threat;
    let s = threat::stats();
    crate::println!("┌─────────────────────────────────────────┐");
    crate::println!("│      IA Defensiva — Estado              │");
    crate::println!("├─────────────────────────────────────────┤");
    crate::println!("│ Eventos totais:    {:>6}               │", s.total_events);
    crate::println!("│ Alertas:           {:>6}               │", s.alerts_fired);
    crate::println!("│ Em quarentena:     {:>6}               │", s.quarantined_procs);
    crate::println!("│ Processos mortos:  {:>6}               │", s.terminated_procs);
    crate::println!("│ Scans executados:  {:>6}               │", s.scans_run);
    crate::println!("├─────────────────────────────────────────┤");
    let privacy = threat::PRIVACY_POLICY.lock();
    crate::println!("│ Privacidade: {:<27} │", privacy.level.as_str());
    crate::println!("│ Telemetria:  {:<27} │",
        if privacy.telemetry { "ativa" } else { "desativada" });
    crate::println!("│ Sync P2P:    {:<27} │",
        if privacy.p2p_sync { "permitido" } else { "bloqueado" });
    crate::println!("│ Cifra disco: {:<27} │",
        if privacy.encrypt_at_rest { "ativa" } else { "desativada" });
    crate::println!("└─────────────────────────────────────────┘");
}
fn cmd_privacy(level: Option<&str>) {
    use crate::security::threat::{self, PrivacyLevel};
    match level {
        None => {
            crate::println!("Uso: privacy <open|balanced|private|lockdown>");
            let p = threat::PRIVACY_POLICY.lock();
            crate::println!("Nivel atual: {}", p.level.as_str());
        }
        Some("open")     => threat::set_privacy(PrivacyLevel::Open),
        Some("balanced") => threat::set_privacy(PrivacyLevel::Balanced),
        Some("private")  => threat::set_privacy(PrivacyLevel::Private),
        Some("lockdown") => threat::set_privacy(PrivacyLevel::Lockdown),
        Some(other)      => crate::println!("[ERRO] Nivel invalido: '{}'", other),
    }
}
fn cmd_p2p() {
    let s = crate::p2p::get_stats();
    crate::println!("P2P: {} | Node: {}...", if s.online {"ONLINE"} else {"OFFLINE"}, s.node_id_short);
    crate::println!("Peers: {} conhecidos / {} ativos", s.peers_known, s.peers_active);
    let c = crate::p2p::crypto::get_stats();
    crate::println!("Cripto: {} sessoes | {} msgs", c.active_sessions, c.total_messages);
}
fn cmd_peers() {
    let peers = crate::p2p::peer::get_all_peers();
    crate::println!("ID        NOME          ESTADO       SCORE");
    crate::println!("──────────────────────────────────────────");
    for p in &peers {
        crate::println!("{}  {:<14}{:<13}{}",
            p.short_id(), p.name,
            alloc::format!("{:?}", p.state), p.trust_score);
    }
    crate::println!("{} peer(s)", peers.len());
}
fn cmd_ia() {
    let s = crate::ia::get_stats();
    crate::println!("IA: {} | Inferences: {} | Acc: {}%",
        if s.initialized {"ATIVO"} else {"INATIVO"},
        s.inferences_total, s.model_accuracy);
    crate::println!("Amostras: {} | Otimizacoes: {} | Latencia: {}us",
        s.metrics_collected, s.optimizations_applied,
        crate::ia::model::avg_latency_us());
}
fn cmd_suggest() {
    let suggestions = crate::ia::suggest::get_suggestions();
    if suggestions.is_empty() { crate::println!("Sem sugestoes no momento."); return; }
    for s in &suggestions {
        crate::println!("[{}] {} ({}%)", s.id, s.title, s.confidence);
        crate::println!("  {}", s.description);
    }
}
fn cmd_edge() {
    let s = crate::edge::get_stats();
    crate::println!("Edge: {} nos | {} submetidas | {} concluidas",
        s.active_nodes, s.tasks_submitted, s.tasks_completed);
    crate::println!("Bytes offloaded: {} KB", s.bytes_offloaded / 1024);
    for node in crate::edge::node::get_all() {
        crate::println!("  {:?} | {} | {} MIPS",
            node.state, node.name, node.profile.cpu_mips);
    }
}
fn cmd_wasm() {
    let (loaded, active, calls, traps) = crate::wasm::get_stats();
    crate::println!("WASM: {} modulos | {} instancias | {} calls | {} traps",
        loaded, active, calls, traps);
    crate::println!("Mem max/inst: {} MB", crate::wasm::MAX_LINEAR_MEMORY / 1024 / 1024);
}
fn cmd_xr() {
    let s = crate::xr::get_stats();
    crate::println!("XR: {} | {}", if s.initialized {"ATIVO"} else {"INATIVO"}, s.session_state);
    crate::println!("Sistema: {}", s.system_name.unwrap_or_else(|| "N/A".into()));
    crate::println!("Frames: {} | HMD: ({:.2},{:.2},{:.2}) yaw:{:.1}deg",
        s.frame_count, s.hmd_pos.x, s.hmd_pos.y, s.hmd_pos.z, s.hmd_yaw_deg);
}
fn cmd_quantum() {
    let stats = crate::quantum::get_stats();
    crate::println!("Quantum: {} jobs | {} concluidos | {} shots",
        stats.jobs_total, stats.jobs_completed, stats.total_shots);
    crate::println!("Executando Bell State demo...");
    let jid = crate::quantum::run_demo_bell_state();
    let q = crate::quantum::QUANTUM.lock();
    if let Some(job) = q.get_job(jid) {
        if let Some(results) = &job.results {
            for (state, count) in results {
                crate::println!("  |{}> : {} ({:.1}%)",
                    state, count, *count as f32 / job.shots as f32 * 100.0);
            }
        }
    }
}
fn cmd_net() {
    let s = crate::net::get_stats();
    crate::println!("Net: {} | Hostname: {}",
        if s.initialized {"ATIVO"} else {"INATIVO"}, s.hostname);
    crate::println!("IP: {} | {} interfaces ({} up)",
        s.primary_ip.unwrap_or_else(|| "N/A".into()), s.interfaces, s.link_up);
    let (tx_p, rx_p, tx_b, rx_b) = crate::net::virtio::get_stats();
    crate::println!("virtio-net TX:{} pkts/{} B | RX:{} pkts/{} B",
        tx_p, tx_b, rx_p, rx_b);
    crate::println!("MAC: {} | Link: {}",
        crate::net::virtio::get_mac().to_string(),
        if crate::net::virtio::is_up() {"UP"} else {"DOWN"});
}
fn cmd_syscall() {
    let (total, errors) = crate::syscall::get_stats();
    crate::println!("Syscall: {} chamadas | {} erros", total, errors);
    crate::println!("POSIX: open/close/read/write/socket/...");
    crate::println!("SOC-D: p2p/ia/edge/wasm/xr/quantum/ui/sec");
    // Test: write to stdout
    let args = crate::syscall::SyscallArgs {
        nr: 3, a0: 1,
        a1: b"[SYSCALL TEST] ok\n".as_ptr() as u64,
        a2: 18, ..Default::default()
    };
    let r = crate::syscall::dispatch(&args);
    crate::println!("test write: {} bytes", r);
}
fn cmd_ui() {
    let state = crate::ui::UI_STATE.lock();
    let comp = crate::ui::compositor::stats();
    crate::println!("UI: {} | {:?} | {} frames",
        if state.initialized {"ATIVO"} else {"INATIVO"},
        state.mode, state.frames_rendered);
    crate::println!("Surfaces: {} ({} mapeadas) | Compositor: {} frames",
        comp.total_surfaces, comp.mapped, comp.frames_composed);
    crate::println!("Resolucao: {}x{} 32bpp",
        crate::ui::SCREEN_WIDTH, crate::ui::SCREEN_HEIGHT);
}
fn cmd_arm() {
    let info = crate::arch::arm::ArmCpuInfo::read();
    crate::println!("Arch: {} | {} {} ARMv{}",
        crate::arch::arm::ARCH,
        info.implementer_name(), info.part_name(), info.architecture);
    crate::println!("Cores: {} | SIMD: {} | Crypto: {}",
        info.core_count, info.has_simd, info.has_crypto);
}
fn cmd_clear() {
    for _ in 0..30 { crate::println!(""); }
}
fn cmd_reboot() {
    crate::println!("Reiniciando...");
    unsafe {
        let mut port = x86_64::instructions::port::Port::<u8>::new(0xCF9);
        port.write(0x06u8);
    }
}
