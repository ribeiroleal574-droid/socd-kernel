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
        u  => { crate::serial_println!("[ERRO] '{}' desconhecido. Digite 'help'", u); }
    }
}

fn cmd_help() {
    crate::serial_println!("┌──────────────────────────────────────────────────┐");
    crate::serial_println!("│           SOC-D Shell — Comandos                 │");
    crate::serial_println!("├──────────────┬───────────────────────────────────┤");
    crate::serial_println!("│ Sistema      │ help version status mem ps        │");
    crate::serial_println!("│              │ sched ls cat modules sandbox      │");
    crate::serial_println!("├──────────────┼───────────────────────────────────┤");
    crate::serial_println!("│ Processos    │ exec [demo|<elf>]  kill <pid>     │");
    crate::serial_println!("│ Containers   │ ct [ls|stop|rm|stats] [id]        │");
    crate::serial_println!("├──────────────┼───────────────────────────────────┤");
    crate::serial_println!("│ DAG/Sync     │ dag [ls|stats|/path]  sync        │");
    crate::serial_println!("│ Cross-Device │ devices  handoff <s> <d>          │");
    crate::serial_println!("│              │ clipboard [texto]                  │");
    crate::serial_println!("├──────────────┼───────────────────────────────────┤");
    crate::serial_println!("│ Seguranca    │ threat  privacy <nivel>           │");
    crate::serial_println!("│ Rede         │ net  p2p  peers                   │");
    crate::serial_println!("├──────────────┼───────────────────────────────────┤");
    crate::serial_println!("│ Subsistemas  │ ia suggest edge wasm xr           │");
    crate::serial_println!("│              │ quantum syscall ui arm            │");
    crate::serial_println!("├──────────────┼───────────────────────────────────┤");
    crate::serial_println!("│ Terminal     │ clear  reboot                     │");
    crate::serial_println!("└──────────────┴───────────────────────────────────┘");
}
fn cmd_version() {
    crate::serial_println!("SOC-D Kernel v{}", env!("CARGO_PKG_VERSION"));
    crate::serial_println!("Sistema Operacional Cognitivo Distribuido");
}
fn cmd_status() {
    use crate::modules::registry::REGISTRY;
    let reg = REGISTRY.lock();
    let s = reg.stats();
    crate::serial_println!("Modulos: {} total | {} ativos | {} falhos", s.total, s.active, s.failed);
    for m in reg.active_modules() {
        crate::serial_println!("  [OK] {} v{}", m.name, m.status.version);
    }
}
fn cmd_memory() {
    use crate::memory::heap::{heap_stats, HEAP_SIZE};
    let (used, free) = heap_stats();
    crate::serial_println!("Heap: {} KB total | {} KB usado | {} KB livre",
        HEAP_SIZE/1024, used/1024, free/1024);
}
fn cmd_sandbox() {
    let s = crate::security::sandbox::get_stats();
    crate::serial_println!("Sandbox: {} ativos | {} violacoes | {} risco",
        s.active_sandboxes, s.total_violations, s.high_risk_processes);
}
fn cmd_ps() {
    let procs = crate::modules::scheduler::list_processes();
    crate::serial_println!("PID  NOME              ESTADO          PRIO");
    crate::serial_println!("────────────────────────────────────────────");
    for p in &procs {
        crate::serial_println!("{:<5}{:<18}{:<16}{:?}", p.pid, p.name, p.state, p.priority);
    }
    crate::serial_println!("{} processo(s)", procs.len());
}
fn cmd_sched() {
    let s = crate::modules::scheduler::get_stats();
    crate::serial_println!("Tick: {} | CTX switches: {} | Procs: {}",
        s.current_tick, s.context_switches, s.total_processes);
    crate::serial_println!("  Rodando:{} Prontos:{} Bloqueados:{} Dormindo:{}",
        s.running, s.ready, s.blocked, s.sleeping);
}
fn cmd_ls(path: Option<&str>) {
    match crate::modules::tmpfs::ls(path.unwrap_or("/")) {
        Ok(entries) => {
            crate::serial_println!("{}:", path.unwrap_or("/"));
            for (name, id) in &entries {
                crate::serial_println!("  [{}] {}", id, name);
            }
        }
        Err(e) => { crate::serial_println!("[ERRO] {}", e); },
    }
}
fn cmd_cat(path: Option<&str>) {
    let p = match path { Some(p) => p, None => { crate::serial_println!("Uso: cat <path>"); return; } };
    match crate::modules::tmpfs::read(p) {
        Ok(data) => {
            if let Ok(s) = core::str::from_utf8(&data) { crate::print!("{}", s); }
            else { crate::serial_println!("[BINARIO {} bytes]", data.len()); }
        }
        Err(e) => { crate::serial_println!("[ERRO] {}", e); },
    }
}
fn cmd_modules() {
    use crate::modules::elf_loader::ELF_MANAGER;
    let m = ELF_MANAGER.lock();
    let list = m.list();
    if list.is_empty() { crate::serial_println!("Nenhum modulo ELF externo carregado."); }
    else {
        for (name, base, size) in &list {
            crate::serial_println!("  0x{:016x}  {} KB  {}", base, size/1024, name);
        }
    }
}
fn cmd_exec(arg: Option<&str>) {
    use crate::modules::process;
    match arg {
        None => {
            crate::serial_println!("Uso: exec <nome>");
            crate::serial_println!("     exec demo   -- lanca tarefas de demonstracao");
            crate::serial_println!("Nota: para ELF externo, carregue via TmpFS e use exec <path>");
        }
        Some("demo") => {
            process::exec_demo();
            let procs = process::list_dynamic();
            crate::serial_println!("Processos dinamicos ativos: {}", procs.len());
            for p in &procs {
                crate::serial_println!("  PID={} '{}' entry=0x{:x}",
                    p.pid, p.name, p.entry);
            }
        }
        Some(name) => {
            // Tenta carregar do TmpFS
            match crate::modules::tmpfs::read(name) {
                Ok(data) => {
                    match process::exec_elf(name, &data) {
                        Ok(pid) => { crate::serial_println!("[OK] '{}' carregado PID={}", name, pid); },
                        Err(e)  => { crate::serial_println!("[ERRO] exec '{}': {}", name, e); },
                    }
                }
                Err(_) => {
                    crate::serial_println!("[ERRO] '{}' nao encontrado no TmpFS.", name);
                    crate::serial_println!("Copie o ELF para o TmpFS primeiro.");
                }
            }
        }
    }
}
fn cmd_kill(arg: Option<&str>) {
    match arg {
        None => { crate::serial_println!("Uso: kill <pid>"); }
        Some(s) => {
            match s.parse::<u64>() {
                Ok(pid) => {
                    if crate::modules::process::kill(pid) {
                        crate::serial_println!("[OK] PID={} terminado.", pid);
                    } else {
                        crate::serial_println!("[ERRO] PID={} nao encontrado.", pid);
                    }
                }
                Err(_) => { crate::serial_println!("[ERRO] PID invalido: '{}'", s); },
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
            crate::serial_println!("DAG: {} blocos | {} ficheiros | {} merges",
                s.total_blocks, s.file_blocks, s.merge_count);
            if paths.is_empty() {
                crate::serial_println!("  (vazio)");
            } else {
                for p in &paths {
                    crate::serial_println!("  {}", p);
                }
            }
        }
        Some("stats") => {
            let s = dag::stats();
            crate::serial_println!("Blocos totais:      {}", s.total_blocks);
            crate::serial_println!("Blocos ficheiro:    {}", s.file_blocks);
            crate::serial_println!("Blocos sync:        {}", s.sync_blocks);
            crate::serial_println!("Merges:             {}", s.merge_count);
            crate::serial_println!("Conflitos resolvidos:{}", s.conflicts_resolved);
        }
        Some("verify") => {
            use crate::p2p::dag_sig;
            let (ok, fail, untrusted) = dag_sig::stats();
            crate::serial_println!("DAG Cadeia de Confianca:");
            crate::serial_println!("  Blocos verificados OK:    {}", ok);
            crate::serial_println!("  Blocos rejeitados:        {}", fail);
            crate::serial_println!("  Autores nao verificados:  {}", untrusted);
            let chain = dag_sig::TRUST_CHAIN.lock();
            crate::serial_println!("  Chaves confiadas:         {}", chain.trusted_key_count());
        }
        Some(path) if path.starts_with('/') => {
            let hist = dag::history(path);
            if hist.is_empty() {
                crate::serial_println!("Sem historico para '{}'", path);
            } else {
                crate::serial_println!("Historico '{}': {} versoes", path, hist.len());
                for (seq, hash) in &hist {
                    crate::serial_println!("  v{} hash={}", seq, hash);
                }
            }
        }
        Some(_) => {
            crate::serial_println!("dag ls          -- lista paths");
            crate::serial_println!("dag stats       -- estatisticas");
            crate::serial_println!("dag verify      -- cadeia de confianca");
            crate::serial_println!("dag /path       -- historico de versoes");
        }
    }
}
fn cmd_sync() {
    use crate::p2p::dag;
    let tick = crate::modules::scheduler::get_stats().current_tick;
    dag::sync_tick(tick);
    let s = dag::stats();
    crate::serial_println!("[SYNC] DAG: {} blocos | {} peers conectados",
        s.total_blocks,
        crate::p2p::get_stats().peers_active);
    crate::serial_println!("[SYNC] Blocos propagados via Gossip P2P");
}
fn cmd_ct(sub: Option<&str>, arg: Option<&str>) {
    use crate::modules::virt;
    match sub {
        None | Some("ls") => {
            let s = virt::stats();
            crate::serial_println!("Containers: {} total | {} running | {} paused | {} stopped",
                s.total, s.running, s.paused, s.stopped);
            for c in virt::list() {
                crate::serial_println!("  [{}] {} | {} | {} | pids={:?}",
                    c.id, c.name, c.runtime, c.state, c.pids);
            }
        }
        Some("stop") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if virt::stop(id) { crate::serial_println!("[OK] Container {} parado.", id); }
                    else { crate::serial_println!("[ERRO] Container {} nao encontrado.", id); }
                }
                None => { crate::serial_println!("Uso: ct stop <id>"); }
            }
        }
        Some("rm") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if virt::remove(id) { crate::serial_println!("[OK] Container {} removido.", id); }
                    else { crate::serial_println!("[ERRO] Container {} nao encontrado ou ainda a correr.", id); }
                }
                None => { crate::serial_println!("Uso: ct rm <id>"); }
            }
        }
        Some("stats") => {
            let s = virt::stats();
            crate::serial_println!("Total:   {}", s.total);
            crate::serial_println!("Running: {}", s.running);
            crate::serial_println!("Paused:  {}", s.paused);
            crate::serial_println!("Stopped: {}", s.stopped);
        }
        Some(_) => {
            crate::serial_println!("ct ls           -- lista containers");
            crate::serial_println!("ct stats        -- estatisticas");
            crate::serial_println!("ct stop <id>    -- para container");
            crate::serial_println!("ct rm <id>      -- remove container parado");
        }
    }
}
fn cmd_pci() {
    use crate::net::virtio_real;
    crate::serial_println!("PCI Bus Scan:");
    crate::serial_println!("{:<8} {:<8} {:<8} {:<8} {:<6}", "Bus:Dev", "Vendor", "Device", "Class", "BAR0");
    crate::serial_println!("{}", "─".repeat(44));
    let devices = virtio_real::list_pci_devices();
    if devices.is_empty() {
        crate::serial_println!("  Nenhum device PCI encontrado.");
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
        crate::serial_println!("{:02x}:{:02x}     {:04x}    {:04x}    {:02x}:{:02x}  0x{:08x}  {}",
            d.bus, d.dev, d.vendor, d.device,
            d.class, d.subclass, d.bar0, name);
    }
    crate::serial_println!("");
    // virtio-net real status
    let real = crate::net::virtio_real::VIRTIO_REAL.lock();
    if real.initialized {
        crate::serial_println!("virtio-net PCI real: ATIVO");
        crate::serial_println!("  MAC:  {}", real.mac_string());
        crate::serial_println!("  Link: {}", if real.link_up { "UP" } else { "DOWN" });
        let (tx_p, rx_p, tx_b, rx_b) = (real.tx_packets, real.rx_packets,
                                          real.tx_bytes,   real.rx_bytes);
        crate::serial_println!("  TX:   {} pkts / {} bytes", tx_p, tx_b);
        crate::serial_println!("  RX:   {} pkts / {} bytes", rx_p, rx_b);
    } else {
        crate::serial_println!("virtio-net PCI real: NAO DISPONIVEL");
        crate::serial_println!("  Adicionar ao QEMU:");
        crate::serial_println!("  -netdev user,id=net0 -device virtio-net-pci,netdev=net0");
    }
}
fn cmd_test(arg: Option<&str>) {
    use crate::modules::tests;
    match arg {
        None | Some("run") => {
            crate::serial_println!("A executar suite de testes...");
            tests::run_all();
            let (pass, fail, skip) = tests::get_summary();
            crate::serial_println!("Resultado: {} pass | {} fail | {} skip",
                pass, fail, skip);
        }
        Some("status") => {
            let (pass, fail, skip) = tests::get_summary();
            if pass == 0 && fail == 0 {
                crate::serial_println!("Testes ainda nao executados. Use 'test run'.");
            } else {
                crate::serial_println!("Ultimo resultado: {} pass | {} fail | {} skip",
                    pass, fail, skip);
                if fail == 0 {
                    crate::serial_println!("TODOS OS TESTES PASSARAM.");
                } else {
                    crate::serial_println!("{} TESTES FALHARAM.", fail);
                }
            }
        }
        Some(_) => {
            crate::serial_println!("test run    -- executa todos os 46 testes");
            crate::serial_println!("test status -- mostra resultado do ultimo run");
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
    crate::serial_println!("{}", rep);
    let alerts = monitor::active_alerts();
    if !alerts.is_empty() {
        crate::serial_println!("ALERTAS ATIVOS: {}", alerts.len());
        for a in &alerts {
            crate::serial_println!("  [{}] {}", a.kind.as_str(), a.message);
        }
    }
}
fn cmd_top() {
    use crate::modules::scheduler;
    let procs = scheduler::list_processes();
    let stats = scheduler::get_stats();
    crate::serial_println!("SOC-D top — {} processos | tick={}", procs.len(), stats.current_tick);
    crate::serial_println!("{:<6} {:<18} {:<10} {:<8} {:<10}",
        "PID", "Nome", "Estado", "Prioridade", "CPU ticks");
    crate::serial_println!("{}", "─".repeat(56));
    // Ordena por cpu_ticks decrescente
    let mut sorted = procs.clone();
    sorted.sort_by(|a, b| b.cpu_ticks.cmp(&a.cpu_ticks));
    for p in sorted.iter().take(16) {
        crate::serial_println!("{:<6} {:<18} {:<10} {:<8} {:<10}",
            p.pid, p.name, p.state, alloc::format!("{:?}", p.priority), p.cpu_ticks);
    }
    if sorted.len() > 16 {
        crate::serial_println!("  ... e mais {} processos", sorted.len() - 16);
    }
    // Resumo heap
    let (used, free) = crate::memory::heap::heap_stats();
    crate::serial_println!("");
    crate::serial_println!("Heap: {} usado / {} livre / {} total",
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
            crate::serial_println!("┌──────────────────────────────────────────────┐");
            crate::serial_println!("│         Motor Cognitivo SOC-D                │");
            crate::serial_println!("├──────────────────────────────────────────────┤");
            crate::serial_println!("│ Ciclos executados:  {:>8}                │", s.cycles_run);
            crate::serial_println!("│ Padroes match:      {:>8}                │", s.patterns_matched);
            crate::serial_println!("│ Acoes executadas:   {:>8}                │", s.actions_executed);
            crate::serial_println!("│ Sugestoes feitas:   {:>8}                │", s.suggestions_made);
            crate::serial_println!("│ Episodios memoria:  {:>8}                │", s.episodes_stored);
            crate::serial_println!("├──────────────────────────────────────────────┤");
            crate::serial_println!("│ Knowledge Graph: {} nos / {} arestas         │",
                engine.knowledge.node_count(),
                engine.knowledge.edge_count());
            crate::serial_println!("├──────────────────────────────────────────────┤");
            crate::serial_println!("│ Padroes registados:                          │");
            for p in &engine.patterns {
                crate::serial_println!("│  [{}] '{}' conf={:.0}% {}             │",
                    p.id, p.name,
                    p.confidence * 100.0,
                    if p.approved { "AUTO" } else { "PENDENTE" });
            }
            crate::serial_println!("└──────────────────────────────────────────────┘");
        }
        Some("approve") => {
            match arg.and_then(|s| s.parse::<u64>().ok()) {
                Some(id) => {
                    if cognitive::approve(id) {
                        crate::serial_println!("[OK] Padrao {} aprovado para auto-execucao.", id);
                    } else {
                        crate::serial_println!("[ERRO] Padrao {} nao encontrado.", id);
                    }
                }
                None => { crate::serial_println!("Uso: cogn approve <id>"); }
            }
        }
        Some("log") => {
            let engine = cognitive::COGNITIVE.lock();
            let recent = engine.recent_actions(10);
            if recent.is_empty() {
                crate::serial_println!("Nenhuma acao executada ainda.");
            } else {
                crate::serial_println!("Ultimas acoes do motor cognitivo:");
                for (tick, pattern, action) in &recent {
                    crate::serial_println!("  tick={} '{}' → {}", tick, pattern, action);
                }
            }
        }
        Some("tick") => {
            let tick = crate::modules::scheduler::get_stats().current_tick;
            cognitive::cognitive_tick(tick);
            crate::serial_println!("[OK] Ciclo cognitivo executado (tick={}).", tick);
        }
        Some(_) => {
            crate::serial_println!("cogn              -- estado do motor cognitivo");
            crate::serial_println!("cogn approve <id> -- aprova padrao para auto-execucao");
            crate::serial_println!("cogn log          -- historico de acoes");
            crate::serial_println!("cogn tick         -- executa ciclo imediatamente");
        }
    }
}
fn cmd_mobile(arg: Option<&str>) {
    use crate::ui::mobile::{self, FormFactor, Theme};
    match arg {
        None => {
            let s = mobile::stats();
            let ui = mobile::MOBILE_UI.lock();
            crate::serial_println!("UI Mobile Adaptativa:");
            crate::serial_println!("  Form factor:  {}", ui.form_factor.as_str());
            let (w, h) = ui.form_factor.dimensions();
            crate::serial_println!("  Resolucao:    {}x{}", w, h);
            crate::serial_println!("  Touch:        {}", if ui.form_factor.is_touch() {"sim"} else {"nao"});
            crate::serial_println!("  Tema:         {:?}", ui.theme);
            crate::serial_println!("  Layouts:      {}", s.layouts_computed);
            crate::serial_println!("  Gestos:       {}", s.gestures_handled);
        }
        Some("desktop") => mobile::adapt(FormFactor::Desktop { width: 1024, height: 768 }),
        Some("mobile")  => mobile::adapt(FormFactor::Mobile  { width: 1080, height: 2340, portrait: true }),
        Some("tablet")  => mobile::adapt(FormFactor::Tablet  { width: 2048, height: 1536, portrait: false }),
        Some("tv")      => mobile::adapt(FormFactor::Tv      { width: 3840, height: 2160 }),
        Some("ar")      => mobile::adapt(FormFactor::Ar),
        Some("vr")      => mobile::adapt(FormFactor::Vr),
        Some(_) => {
            crate::serial_println!("mobile [desktop|mobile|tablet|tv|ar|vr]");
            crate::serial_println!("  Adapta a UI ao form factor especificado");
        }
    }
}
fn cmd_theme(arg: Option<&str>) {
    use crate::ui::mobile::{self, Theme};
    match arg {
        None => {
            let ui = mobile::MOBILE_UI.lock();
            crate::serial_println!("Tema atual: {:?}", ui.theme);
            crate::serial_println!("Disponiveis: dark | light | oled | ar");
        }
        Some("dark")  => { mobile::set_theme(Theme::Dark);          crate::serial_println!("[OK] Tema: dark"); }
        Some("light") => { mobile::set_theme(Theme::Light);         crate::serial_println!("[OK] Tema: light"); }
        Some("oled")  => { mobile::set_theme(Theme::Oled);          crate::serial_println!("[OK] Tema: oled (preto puro)"); }
        Some("ar")    => { mobile::set_theme(Theme::ArTransparent); crate::serial_println!("[OK] Tema: AR transparente"); }
        Some(t)       => { crate::serial_println!("[ERRO] Tema '{}' desconhecido", t); },
    }
}
fn cmd_ar() {
    use crate::ui::ar;
    let s = ar::stats();
    let scene = ar::SPATIAL.lock();
    crate::serial_println!("┌─────────────────────────────────────────┐");
    crate::serial_println!("│       Interface Holografica AR           │");
    crate::serial_println!("├─────────────────────────────────────────┤");
    crate::serial_println!("│ Anchors criados:   {:>6}               │", s.anchors_created);
    crate::serial_println!("│ Holograms ativos:  {:>6}               │", s.holograms_active);
    crate::serial_println!("│ Activacoes gaze:   {:>6}               │", s.gaze_activations);
    crate::serial_println!("│ Gestos mao:        {:>6}               │", s.gestures_processed);
    crate::serial_println!("│ Frames renderiz.:  {:>6}               │", s.frames_rendered);
    crate::serial_println!("├─────────────────────────────────────────┤");
    let focused = scene.gaze.focused_hologram;
    crate::serial_println!("│ Foco gaze:  {:>28} │",
        focused.map(|id| alloc::format!("hologram id={}", id))
               .unwrap_or_else(|| "nenhum".to_string()));
    crate::serial_println!("│ Dwell:      {:>4} / {:>4} ticks          │",
        scene.gaze.dwell_ticks, scene.gaze.dwell_threshold);
    crate::serial_println!("└─────────────────────────────────────────┘");
    crate::serial_println!("Holograms:");
    for h in &scene.holograms {
        let p = &h.local_pose.position;
        let focus = if h.gaze_focused { " [FOCO]" } else { "" };
        crate::serial_println!("  [{}] ({:.1},{:.1},{:.1}) op={:.1}{}",
            h.id, p.x, p.y, p.z, h.opacity, focus);
    }
}
fn cmd_devices() {
    use crate::modules::xdev;
    let devs = xdev::online_devices();
    let s = xdev::stats();
    crate::serial_println!("Cluster cross-device: {} dispositivos online", devs.len());
    crate::serial_println!("{:<4} {:<14} {:<12} {:<18} {}", "ID", "Nome", "Tipo", "Resolucao", "IP");
    crate::serial_println!("{}", "─".repeat(60));
    for (i, d) in devs.iter().enumerate() {
        let (rx, ry) = d.kind.default_resolution();
        let res = if rx > 0 { alloc::format!("{}x{}", rx, ry) }
                  else { "n/a".to_string() };
        let ip = alloc::format!("{}.{}.{}.{}",
            d.local_ip[0], d.local_ip[1], d.local_ip[2], d.local_ip[3]);
        crate::serial_println!("{:<4} {:<14} {:<12} {:<18} {}", i+1, d.name, d.kind.as_str(), res, ip);
    }
    crate::serial_println!("Sessoes: {} | Handoffs: {} | Clipboard syncs: {}",
        s.sessions_created, s.handoffs_done, s.clipboard_syncs);
}
fn cmd_handoff(sid_arg: Option<&str>, dev_arg: Option<&str>) {
    use crate::modules::xdev::{self, ClipboardContent};
    match (sid_arg, dev_arg) {
        (None, _) | (_, None) => {
            crate::serial_println!("Uso: handoff <session_id> <device_index>");
            crate::serial_println!("  Use 'devices' para ver dispositivos e IDs de sessao");
        }
        (Some(sid_str), Some(dev_str)) => {
            let sid = match sid_str.parse::<u64>() {
                Ok(v) => v,
                Err(_) => { crate::serial_println!("[ERRO] session_id invalido"); return; }
            };
            let devs = xdev::online_devices();
            let idx: usize = match dev_str.parse::<usize>() {
                Ok(v) if v >= 1 && v <= devs.len() => v - 1,
                _ => { crate::serial_println!("[ERRO] device_index invalido (use 'devices')"); return; }
            };
            let target = devs[idx].node_id;
            match xdev::handoff(sid, target) {
                Ok(()) => { crate::serial_println!("[OK] Sessao {} transferida para '{}'", sid, devs[idx].name); }
                Err(_) => { crate::serial_println!("[ERRO] Falha no handoff"); }
            }
        }
    }
}
fn cmd_clipboard(arg: Option<&str>) {
    use crate::modules::xdev::{self, ClipboardContent};
    match arg {
        None => {
            let bus = xdev::XDEV.lock();
            crate::serial_println!("Clipboard: {}", bus.clipboard.as_str());
            if let ClipboardContent::Text(ref t) = bus.clipboard {
                crate::serial_println!("  \"{}\"", t);
            }
        }
        Some(text) => {
            xdev::clipboard_copy(ClipboardContent::Text(text.to_string()));
            crate::serial_println!("[OK] Clipboard copiado e sincronizado via P2P");
        }
    }
}
fn cmd_threat() {
    use crate::security::threat;
    let s = threat::stats();
    crate::serial_println!("┌─────────────────────────────────────────┐");
    crate::serial_println!("│      IA Defensiva — Estado              │");
    crate::serial_println!("├─────────────────────────────────────────┤");
    crate::serial_println!("│ Eventos totais:    {:>6}               │", s.total_events);
    crate::serial_println!("│ Alertas:           {:>6}               │", s.alerts_fired);
    crate::serial_println!("│ Em quarentena:     {:>6}               │", s.quarantined_procs);
    crate::serial_println!("│ Processos mortos:  {:>6}               │", s.terminated_procs);
    crate::serial_println!("│ Scans executados:  {:>6}               │", s.scans_run);
    crate::serial_println!("├─────────────────────────────────────────┤");
    let privacy = threat::PRIVACY_POLICY.lock();
    crate::serial_println!("│ Privacidade: {:<27} │", privacy.level.as_str());
    crate::serial_println!("│ Telemetria:  {:<27} │",
        if privacy.telemetry { "ativa" } else { "desativada" });
    crate::serial_println!("│ Sync P2P:    {:<27} │",
        if privacy.p2p_sync { "permitido" } else { "bloqueado" });
    crate::serial_println!("│ Cifra disco: {:<27} │",
        if privacy.encrypt_at_rest { "ativa" } else { "desativada" });
    crate::serial_println!("└─────────────────────────────────────────┘");
}
fn cmd_privacy(level: Option<&str>) {
    use crate::security::threat::{self, PrivacyLevel};
    match level {
        None => {
            crate::serial_println!("Uso: privacy <open|balanced|private|lockdown>");
            let p = threat::PRIVACY_POLICY.lock();
            crate::serial_println!("Nivel atual: {}", p.level.as_str());
        }
        Some("open")     => threat::set_privacy(PrivacyLevel::Open),
        Some("balanced") => threat::set_privacy(PrivacyLevel::Balanced),
        Some("private")  => threat::set_privacy(PrivacyLevel::Private),
        Some("lockdown") => threat::set_privacy(PrivacyLevel::Lockdown),
        Some(other)      => { crate::serial_println!("[ERRO] Nivel invalido: '{}'", other); },
    }
}
fn cmd_p2p() {
    let s = crate::p2p::get_stats();
    crate::serial_println!("P2P: {} | Node: {}...", if s.online {"ONLINE"} else {"OFFLINE"}, s.node_id_short);
    crate::serial_println!("Peers: {} conhecidos / {} ativos", s.peers_known, s.peers_active);
    let c = crate::p2p::crypto::get_stats();
    crate::serial_println!("Cripto: {} sessoes | {} msgs", c.active_sessions, c.total_messages);
}
fn cmd_peers() {
    let peers = crate::p2p::peer::get_all_peers();
    crate::serial_println!("ID        NOME          ESTADO       SCORE");
    crate::serial_println!("──────────────────────────────────────────");
    for p in &peers {
        crate::serial_println!("{}  {:<14}{:<13}{}",
            p.short_id(), p.name,
            alloc::format!("{:?}", p.state), p.trust_score);
    }
    crate::serial_println!("{} peer(s)", peers.len());
}
fn cmd_ia() {
    let s = crate::ia::get_stats();
    crate::serial_println!("IA: {} | Inferences: {} | Acc: {}%",
        if s.initialized {"ATIVO"} else {"INATIVO"},
        s.inferences_total, s.model_accuracy);
    crate::serial_println!("Amostras: {} | Otimizacoes: {} | Latencia: {}us",
        s.metrics_collected, s.optimizations_applied,
        crate::ia::model::avg_latency_us());
}
fn cmd_suggest() {
    let suggestions = crate::ia::suggest::get_suggestions();
    if suggestions.is_empty() { crate::serial_println!("Sem sugestoes no momento."); return; }
    for s in &suggestions {
        crate::serial_println!("[{}] {} ({}%)", s.id, s.title, s.confidence);
        crate::serial_println!("  {}", s.description);
    }
}
fn cmd_edge() {
    let s = crate::edge::get_stats();
    crate::serial_println!("Edge: {} nos | {} submetidas | {} concluidas",
        s.active_nodes, s.tasks_submitted, s.tasks_completed);
    crate::serial_println!("Bytes offloaded: {} KB", s.bytes_offloaded / 1024);
    for node in crate::edge::node::get_all() {
        crate::serial_println!("  {:?} | {} | {} MIPS",
            node.state, node.name, node.profile.cpu_mips);
    }
}
fn cmd_wasm() {
    let (loaded, active, calls, traps) = crate::wasm::get_stats();
    crate::serial_println!("WASM: {} modulos | {} instancias | {} calls | {} traps",
        loaded, active, calls, traps);
    crate::serial_println!("Mem max/inst: {} MB", crate::wasm::MAX_LINEAR_MEMORY / 1024 / 1024);
}
fn cmd_xr() {
    let s = crate::xr::get_stats();
    crate::serial_println!("XR: {} | {}", if s.initialized {"ATIVO"} else {"INATIVO"}, s.session_state);
    crate::serial_println!("Sistema: {}", s.system_name.unwrap_or_else(|| "N/A".into()));
    crate::serial_println!("Frames: {} | HMD: ({:.2},{:.2},{:.2}) yaw:{:.1}deg",
        s.frame_count, s.hmd_pos.x, s.hmd_pos.y, s.hmd_pos.z, s.hmd_yaw_deg);
}
fn cmd_quantum() {
    let stats = crate::quantum::get_stats();
    crate::serial_println!("Quantum: {} jobs | {} concluidos | {} shots",
        stats.jobs_total, stats.jobs_completed, stats.total_shots);
    crate::serial_println!("Executando Bell State demo...");
    let jid = crate::quantum::run_demo_bell_state();
    let q = crate::quantum::QUANTUM.lock();
    if let Some(job) = q.get_job(jid) {
        if let Some(results) = &job.results {
            for (state, count) in results {
                crate::serial_println!("  |{}> : {} ({:.1}%)",
                    state, count, *count as f32 / job.shots as f32 * 100.0);
            }
        }
    }
}
fn cmd_net() {
    let s = crate::net::get_stats();
    crate::serial_println!("Net: {} | Hostname: {}",
        if s.initialized {"ATIVO"} else {"INATIVO"}, s.hostname);
    crate::serial_println!("IP: {} | {} interfaces ({} up)",
        s.primary_ip.unwrap_or_else(|| "N/A".into()), s.interfaces, s.link_up);
    let (tx_p, rx_p, tx_b, rx_b) = crate::net::virtio::get_stats();
    crate::serial_println!("virtio-net TX:{} pkts/{} B | RX:{} pkts/{} B",
        tx_p, tx_b, rx_p, rx_b);
    crate::serial_println!("MAC: {} | Link: {}",
        crate::net::virtio::get_mac().to_string(),
        if crate::net::virtio::is_up() {"UP"} else {"DOWN"});
}
fn cmd_syscall() {
    let (total, errors) = crate::syscall::get_stats();
    crate::serial_println!("Syscall: {} chamadas | {} erros", total, errors);
    crate::serial_println!("POSIX: open/close/read/write/socket/...");
    crate::serial_println!("SOC-D: p2p/ia/edge/wasm/xr/quantum/ui/sec");
    // Test: write to stdout
    let args = crate::syscall::SyscallArgs {
        nr: 3, a0: 1,
        a1: b"[SYSCALL TEST] ok\n".as_ptr() as u64,
        a2: 18, ..Default::default()
    };
    let r = crate::syscall::dispatch(&args);
    crate::serial_println!("test write: {} bytes", r);
}
fn cmd_ui() {
    let state = crate::ui::UI_STATE.lock();
    let comp = crate::ui::compositor::stats();
    crate::serial_println!("UI: {} | {:?} | {} frames",
        if state.initialized {"ATIVO"} else {"INATIVO"},
        state.mode, state.frames_rendered);
    crate::serial_println!("Surfaces: {} ({} mapeadas) | Compositor: {} frames",
        comp.total_surfaces, comp.mapped, comp.frames_composed);
    crate::serial_println!("Resolucao: {}x{} 32bpp",
        crate::ui::SCREEN_WIDTH, crate::ui::SCREEN_HEIGHT);
}
fn cmd_arm() {
    let info = crate::arch::arm::ArmCpuInfo::read();
    crate::serial_println!("Arch: {} | {} {} ARMv{}",
        crate::arch::arm::ARCH,
        info.implementer_name(), info.part_name(), info.architecture);
    crate::serial_println!("Cores: {} | SIMD: {} | Crypto: {}",
        info.core_count, info.has_simd, info.has_crypto);
}
fn cmd_clear() {
    for _ in 0..30 { crate::serial_println!(""); }
}
fn cmd_reboot() {
    crate::serial_println!("Reiniciando...");
    unsafe {
        let mut port = x86_64::instructions::port::Port::<u8>::new(0xCF9);
        port.write(0x06u8);
    }
}
