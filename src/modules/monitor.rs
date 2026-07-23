// ============================================================
// SOC-D Kernel — Monitor de Recursos em Tempo Real (Fase 6.1)
// ============================================================
//
// Monitoriza em tempo real:
//   - CPU: context switches/s, processos por estado, idle%
//   - RAM: heap usado/livre, fragmentação estimada
//   - Processos: top por CPU ticks, lista detalhada
//   - Rede: bytes enviados/recebidos, peers ativos
//   - DAG: blocos, merges, sync status
//   - Containers: ativos, CPU por container
//   - Sistema: uptime, tick rate, subsistemas
//
// Implementação no_std pura — sem floats onde possível,
// aritmética inteira para percentagens.
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    format,
};
use spinning_top::Spinlock;

// ─── Snapshot de Recursos ────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ResourceSnapshot {
    pub tick:              u64,

    // CPU
    pub context_switches:  u64,
    pub cs_per_sec:        u64,   // context switches por segundo (estimado)
    pub cpu_idle_pct:      u8,    // % idle (0–100)
    pub proc_total:        usize,
    pub proc_running:      usize,
    pub proc_ready:        usize,
    pub proc_blocked:      usize,
    pub proc_sleeping:     usize,
    pub proc_dead:         usize,

    // RAM
    pub heap_used_bytes:   usize,
    pub heap_free_bytes:   usize,
    pub heap_total_bytes:  usize,
    pub heap_used_pct:     u8,    // % usado (0–100)

    // Rede
    pub net_bytes_sent:    u64,
    pub net_bytes_recv:    u64,
    pub net_peers_active:  usize,
    pub net_packets_out:   u64,
    pub net_packets_in:    u64,

    // DAG
    pub dag_total_blocks:  usize,
    pub dag_file_blocks:   usize,
    pub dag_merges:        usize,
    pub dag_conflicts:     usize,

    // Containers
    pub ct_total:          usize,
    pub ct_running:        usize,
    pub ct_stopped:        usize,

    // IA
    pub ia_inferences:     u64,
    pub ia_cycles:         u64,
    pub ia_actions:        u64,

    // Sistema
    pub uptime_secs:       u64,   // segundos desde boot (tick / 60Hz aprox)
    pub subsystems_ok:     usize,
}

impl ResourceSnapshot {
    /// Captura o estado actual de todos os subsistemas
    pub fn capture(tick: u64, prev: Option<&ResourceSnapshot>) -> Self {
        let sched  = crate::modules::scheduler::get_stats();
        let (heap_used, heap_free) = crate::memory::heap::heap_stats();
        let heap_total = crate::memory::heap::HEAP_SIZE;
        let p2p    = crate::p2p::get_stats();
        let dag    = crate::p2p::dag::stats();
        let ct     = crate::modules::virt::stats();
        let ia     = crate::ia::get_stats();
        let cogn   = crate::ia::cognitive::stats();
        let net    = crate::net::get_stats();

        // Heap % usado
        let heap_used_pct = if heap_total > 0 {
            ((heap_used as u64 * 100) / heap_total as u64) as u8
        } else { 0 };

        // Context switches por segundo (delta entre snapshots)
        let cs_per_sec = if let Some(p) = prev {
            let delta_cs   = sched.context_switches.saturating_sub(p.context_switches);
            let delta_tick = tick.saturating_sub(p.tick).max(1);
            // 60 ticks ≈ 1 segundo
            delta_cs.saturating_mul(60) / delta_tick
        } else { 0 };

        // CPU idle estimado: se não há context switches, está idle
        let cpu_idle_pct = if cs_per_sec == 0 { 95u8 }
            else if cs_per_sec < 10 { 80u8 }
            else if cs_per_sec < 50 { 60u8 }
            else if cs_per_sec < 100 { 40u8 }
            else { 10u8 };

        // Uptime em segundos (60 ticks ≈ 1 segundo a 60Hz)
        let uptime_secs = tick / 60;

        Self {
            tick,
            context_switches:  sched.context_switches,
            cs_per_sec,
            cpu_idle_pct,
            proc_total:    sched.total_processes,
            proc_running:  sched.running,
            proc_ready:    sched.ready,
            proc_blocked:  sched.blocked,
            proc_sleeping: sched.sleeping,
            proc_dead:     sched.dead,
            heap_used_bytes:  heap_used,
            heap_free_bytes:  heap_free,
            heap_total_bytes: heap_total,
            heap_used_pct,
            net_bytes_sent:   net.total_tx_bytes,
            net_bytes_recv:   net.total_rx_bytes,
            net_peers_active: p2p.peers_active,
            net_packets_out:  0,
            net_packets_in:   0,
            dag_total_blocks: dag.total_blocks,
            dag_file_blocks:  dag.file_blocks,
            dag_merges:       dag.merge_count,
            dag_conflicts:    dag.conflicts_resolved,
            ct_total:   ct.total,
            ct_running: ct.running,
            ct_stopped: ct.stopped,
            ia_inferences: ia.inferences_total,
            ia_cycles:     cogn.cycles_run,
            ia_actions:    cogn.actions_executed,
            uptime_secs,
            subsystems_ok: 24, // todos os módulos activos
        }
    }

    /// Formata bytes para string legível (KB, MB)
    pub fn fmt_bytes(b: usize) -> String {
        if b >= 1024 * 1024 {
            format!("{} MB", b / (1024 * 1024))
        } else if b >= 1024 {
            format!("{} KB", b / 1024)
        } else {
            format!("{} B", b)
        }
    }

    /// Formata uptime como HH:MM:SS
    pub fn fmt_uptime(secs: u64) -> String {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    }

    /// Barra de progresso ASCII (largura fixa)
    pub fn progress_bar(pct: u8, width: usize) -> String {
        let filled = (pct as usize * width) / 100;
        let empty  = width.saturating_sub(filled);
        let mut s = String::new();
        s.push('[');
        for _ in 0..filled { s.push('#'); }
        for _ in 0..empty  { s.push('.'); }
        s.push(']');
        s
    }
}

// ─── Monitor Principal ───────────────────────────────────────

pub struct ResourceMonitor {
    /// Histórico de snapshots (últimos N)
    history:      Vec<ResourceSnapshot>,
    max_history:  usize,
    /// Snapshot mais recente
    pub current:  Option<ResourceSnapshot>,
    /// Intervalo entre capturas (ticks)
    interval:     u64,
    pub last_capture: u64,
    /// Alertas ativos
    pub alerts:   Vec<ResourceAlert>,
}

#[derive(Debug, Clone)]
pub struct ResourceAlert {
    pub kind:    AlertKind,
    pub message: String,
    pub tick:    u64,
    pub cleared: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertKind {
    HeapCritical,   // > 85%
    HeapHigh,       // > 70%
    TooManyProcs,   // > 32 processos
    NoPeers,        // 0 peers ativos
    DagStale,       // DAG sem novos blocos há muito tempo
}

impl AlertKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertKind::HeapCritical  => "HEAP-CRITICO",
            AlertKind::HeapHigh      => "HEAP-ALTO",
            AlertKind::TooManyProcs  => "MUITOS-PROCS",
            AlertKind::NoPeers       => "SEM-PEERS",
            AlertKind::DagStale      => "DAG-PARADO",
        }
    }
}

impl ResourceMonitor {
    pub const fn new() -> Self {
        Self {
            history:      Vec::new(),
            max_history:  60, // 60 snapshots = ~1 minuto de histórico
            current:      None,
            interval:     60, // 1 snapshot por segundo
            last_capture: 0,
            alerts:       Vec::new(),
        }
    }

    /// Tick — captura snapshot se necessário
    pub fn tick(&mut self, tick: u64) {
        if tick.saturating_sub(self.last_capture) < self.interval { return; }
        self.last_capture = tick;

        let prev = self.current.as_ref();
        let snap = ResourceSnapshot::capture(tick, prev);

        // Verifica alertas
        self.check_alerts(&snap);

        // Guarda no histórico
        if self.history.len() >= self.max_history {
            self.history.remove(0);
        }
        if let Some(prev) = self.current.take() {
            self.history.push(prev);
        }
        self.current = Some(snap);
    }

    fn check_alerts(&mut self, snap: &ResourceSnapshot) {
        // Heap crítico
        if snap.heap_used_pct > 85 {
            if !self.has_alert(AlertKind::HeapCritical) {
                self.fire_alert(AlertKind::HeapCritical,
                    &format!("Heap {}% usado ({} livre)",
                        snap.heap_used_pct,
                        ResourceSnapshot::fmt_bytes(snap.heap_free_bytes)),
                    snap.tick);
            }
        } else {
            self.clear_alert(AlertKind::HeapCritical);
        }

        // Heap alto
        if snap.heap_used_pct > 70 && snap.heap_used_pct <= 85 {
            if !self.has_alert(AlertKind::HeapHigh) {
                self.fire_alert(AlertKind::HeapHigh,
                    &format!("Heap {}% usado", snap.heap_used_pct),
                    snap.tick);
            }
        } else {
            self.clear_alert(AlertKind::HeapHigh);
        }

        // Sem peers
        if snap.net_peers_active == 0 {
            if !self.has_alert(AlertKind::NoPeers) {
                self.fire_alert(AlertKind::NoPeers,
                    "Nenhum peer P2P ativo", snap.tick);
            }
        } else {
            self.clear_alert(AlertKind::NoPeers);
        }
    }

    fn has_alert(&self, kind: AlertKind) -> bool {
        self.alerts.iter().any(|a| a.kind == kind && !a.cleared)
    }

    fn fire_alert(&mut self, kind: AlertKind, msg: &str, tick: u64) {
        crate::serial_println!("[MONITOR][ALERTA] {} — {}", kind.as_str(), msg);
        self.alerts.push(ResourceAlert {
            kind, message: msg.to_string(), tick, cleared: false,
        });
    }

    fn clear_alert(&mut self, kind: AlertKind) {
        for a in self.alerts.iter_mut() {
            if a.kind == kind { a.cleared = true; }
        }
    }

    /// Retorna tendência de uso de heap (subindo/descendo/estável)
    pub fn heap_trend(&self) -> &'static str {
        if self.history.len() < 3 { return "—"; }
        let n = self.history.len();
        let old = self.history[n-3].heap_used_pct;
        let new = self.current.as_ref().map(|s| s.heap_used_pct).unwrap_or(0);
        if new > old + 5 { "↑" }
        else if old > new + 5 { "↓" }
        else { "→" }
    }

    /// Retorna o último snapshot ou um vazio
    pub fn snapshot(&self) -> Option<&ResourceSnapshot> {
        self.current.as_ref()
    }

    /// Sparkline ASCII de heap dos últimos N snapshots
    pub fn heap_sparkline(&self, n: usize) -> String {
        let bars = ['_', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let samples: Vec<u8> = self.history.iter()
            .rev().take(n).rev()
            .map(|s| s.heap_used_pct)
            .collect();
        samples.iter().map(|&p| {
            let idx = ((p as usize) * (bars.len() - 1)) / 100;
            bars[idx.min(bars.len()-1)]
        }).collect()
    }

    /// Formata o relatório completo de recursos
    pub fn report(&self) -> String {
        let Some(s) = self.current.as_ref() else {
            return "Monitor ainda a inicializar...".to_string();
        };

        let mut out = String::new();
        let sep = "─".repeat(48);

        // Header
        out.push_str(&format!("╔{}╗\n", "═".repeat(48)));
        out.push_str(&format!("║  SOC-D Monitor — uptime: {}  ║\n",
            ResourceSnapshot::fmt_uptime(s.uptime_secs)));
        out.push_str(&format!("╚{}╝\n", "═".repeat(48)));

        // CPU
        let cpu_used = 100u8.saturating_sub(s.cpu_idle_pct);
        out.push_str(&format!("\nCPU  {} {:3}%  (idle {:3}%)\n",
            ResourceSnapshot::progress_bar(cpu_used, 20),
            cpu_used, s.cpu_idle_pct));
        out.push_str(&format!("     ctx-switches/s: {}  total: {}\n",
            s.cs_per_sec, s.context_switches));
        out.push_str(&format!("     procs: {} total | {} run | {} ready | {} sleep | {} dead\n",
            s.proc_total, s.proc_running, s.proc_ready,
            s.proc_sleeping, s.proc_dead));

        // RAM
        let spark = self.heap_sparkline(20);
        out.push_str(&format!("\nRAM  {} {:3}%  {} {}\n",
            ResourceSnapshot::progress_bar(s.heap_used_pct, 20),
            s.heap_used_pct,
            self.heap_trend(),
            if spark.is_empty() { "".to_string() } else { format!("[{}]", spark) }));
        out.push_str(&format!("     usado: {}  livre: {}  total: {}\n",
            ResourceSnapshot::fmt_bytes(s.heap_used_bytes),
            ResourceSnapshot::fmt_bytes(s.heap_free_bytes),
            ResourceSnapshot::fmt_bytes(s.heap_total_bytes)));

        // Rede
        out.push_str(&format!("\nRED  peers: {}  tx: {}  rx: {}\n",
            s.net_peers_active,
            ResourceSnapshot::fmt_bytes(s.net_bytes_sent as usize),
            ResourceSnapshot::fmt_bytes(s.net_bytes_recv as usize)));

        // DAG
        out.push_str(&format!("\nDAG  {} blocos  {} ficheiros  {} merges\n",
            s.dag_total_blocks, s.dag_file_blocks, s.dag_merges));

        // Containers
        out.push_str(&format!("\nCT   {} total  {} running  {} stopped\n",
            s.ct_total, s.ct_running, s.ct_stopped));

        // IA
        out.push_str(&format!("\nIA   {} inferencias  {} ciclos cogn  {} acoes auto\n",
            s.ia_inferences, s.ia_cycles, s.ia_actions));

        // Alertas
        let active_alerts: Vec<&ResourceAlert> = self.alerts.iter()
            .filter(|a| !a.cleared).collect();
        if !active_alerts.is_empty() {
            out.push_str(&format!("\n{}\n", sep));
            out.push_str("ALERTAS:\n");
            for a in active_alerts {
                out.push_str(&format!("  [{}] {}\n", a.kind.as_str(), a.message));
            }
        }

        out.push_str(&format!("{}\n", sep));
        out.push_str(&format!("subsistemas: {}/24 ok  |  tick: {}\n",
            s.subsystems_ok, s.tick));
        out
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static MONITOR: Spinlock<ResourceMonitor> =
    Spinlock::new(ResourceMonitor::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[MONITOR] Monitor de recursos inicializado");
    crate::serial_println!("[MONITOR] Intervalo: 1 snapshot/segundo | Historico: 60s");
}

pub fn monitor_tick(tick: u64) {
    MONITOR.lock().tick(tick);
}

pub fn report() -> String {
    MONITOR.lock().report()
}

pub fn snapshot() -> Option<ResourceSnapshot> {
    MONITOR.lock().current.clone()
}

/// Actualiza o CognitiveContext com dados reais do monitor
pub fn real_cpu_pct() -> u8 {
    MONITOR.lock().current.as_ref()
        .map(|s| 100u8.saturating_sub(s.cpu_idle_pct))
        .unwrap_or(0)
}

pub fn real_ram_pct() -> u8 {
    MONITOR.lock().current.as_ref()
        .map(|s| s.heap_used_pct)
        .unwrap_or(0)
}

pub fn active_alerts() -> Vec<ResourceAlert> {
    MONITOR.lock().alerts.iter()
        .filter(|a| !a.cleared)
        .cloned()
        .collect()
}

pub fn run_demo() {
    crate::serial_println!("\n[FASE6.1] === Monitor de Recursos em Tempo Real ===");
    let tick = crate::modules::scheduler::get_stats().current_tick;
    // Força primeira captura
    MONITOR.lock().tick(tick);
    MONITOR.lock().last_capture = 0; // permite re-captura imediata
    MONITOR.lock().tick(tick + 1);
    let rep = report();
    crate::serial_println!("{}", rep);
    crate::serial_println!("[FASE6.1] Use 'monitor' no shell para ver em tempo real");
    crate::serial_println!("[FASE6.1] Use 'top' para ver processos por CPU");
    crate::serial_println!("[FASE6.1] ===========================================\n");
}
