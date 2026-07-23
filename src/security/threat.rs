// ============================================================
// SOC-D Kernel — IA Defensiva + Segurança Avançada (Fase 3.3)
// ============================================================
//
// Liga o motor de IA ao subsistema de segurança para:
//   - Deteção de anomalias comportamentais em tempo real
//   - Análise de padrões de syscalls por processo
//   - Isolamento automático de processos suspeitos
//   - Políticas dinâmicas de privacidade por utilizador
//   - Deteção de malware zero-day por heurística
//
// Arquitectura:
//
//   ┌──────────────────────────────────────────────────────┐
//   │                  ThreatEngine                        │
//   ├──────────────┬───────────────┬──────────────────────┤
//   │ BehaviorWatch│  AnomalyScore │  ThreatResponse      │
//   │ (por proc)   │  (IA model)   │  (quarantine/kill)   │
//   └──────────────┴───────────────┴──────────────────────┘
//           ↓                ↓               ↓
//   ┌──────────────────────────────────────────────────────┐
//   │   Sandbox (sandbox.rs) + Policy (policy.rs) + IA    │
//   └──────────────────────────────────────────────────────┘
//
// Score de ameaça (0–100):
//   0–20:  Normal
//   21–50: Suspeito — log + alerta
//   51–80: Perigoso — throttle de CPU + alerta crítico
//   81–100: Crítico — quarentena + kill automático
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;
use crate::modules::scheduler::Pid;
use crate::security::TrustLevel;

// ─── Tipos de Ameaça ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ThreatKind {
    /// Excesso de syscalls numa janela de tempo
    SyscallFlood,
    /// Tentativa de acesso a recurso proibido
    UnauthorizedAccess { resource: String },
    /// Padrão de escrita anómalo (possível ransomware)
    AbnormalFileWrite,
    /// Tentativa de escalada de privilégios
    PrivilegeEscalation,
    /// Tráfego de rede anómalo
    NetworkAnomaly,
    /// Consumo excessivo de heap
    MemoryExhaustion,
    /// Padrão de execução desconhecido
    UnknownBehavior,
}

impl ThreatKind {
    pub fn base_score(&self) -> u32 {
        match self {
            ThreatKind::SyscallFlood              => 30,
            ThreatKind::UnauthorizedAccess { .. } => 60,
            ThreatKind::AbnormalFileWrite         => 50,
            ThreatKind::PrivilegeEscalation       => 90,
            ThreatKind::NetworkAnomaly            => 40,
            ThreatKind::MemoryExhaustion          => 35,
            ThreatKind::UnknownBehavior           => 25,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ThreatKind::SyscallFlood              => "syscall-flood",
            ThreatKind::UnauthorizedAccess { .. } => "unauthorized-access",
            ThreatKind::AbnormalFileWrite         => "abnormal-file-write",
            ThreatKind::PrivilegeEscalation       => "privilege-escalation",
            ThreatKind::NetworkAnomaly            => "network-anomaly",
            ThreatKind::MemoryExhaustion          => "memory-exhaustion",
            ThreatKind::UnknownBehavior           => "unknown-behavior",
        }
    }
}

// ─── Evento de Ameaça ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ThreatEvent {
    pub pid:       Pid,
    pub proc_name: String,
    pub kind:      ThreatKind,
    pub score:     u32,
    pub tick:      u64,
    pub action:    ResponseAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResponseAction {
    Log,
    Alert,
    Throttle,
    Quarantine,
    Terminate,
}

impl ResponseAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResponseAction::Log        => "log",
            ResponseAction::Alert      => "alert",
            ResponseAction::Throttle   => "throttle",
            ResponseAction::Quarantine => "quarantine",
            ResponseAction::Terminate  => "terminate",
        }
    }
}

// ─── Perfil Comportamental por Processo ──────────────────────

#[derive(Debug, Clone)]
pub struct ProcessProfile {
    pub pid:            Pid,
    pub name:           String,
    pub trust:          TrustLevel,
    /// Contagem de syscalls nesta janela
    pub syscall_count:  u64,
    /// Syscalls na janela anterior (para deteção de flood)
    pub prev_syscall_count: u64,
    /// Número de escritas de ficheiros
    pub file_writes:    u64,
    /// Número de acessos negados
    pub denied_access:  u64,
    /// Score de ameaça acumulado (decai com o tempo)
    pub threat_score:   u32,
    /// Estado de quarentena
    pub quarantined:    bool,
    /// Tick da última janela de análise
    pub last_window_tick: u64,
    /// Histórico de ameaças (últimas 10)
    pub threat_history: Vec<(u64, ThreatKind)>,
}

impl ProcessProfile {
    pub fn new(pid: Pid, name: String, trust: TrustLevel) -> Self {
        Self {
            pid, name, trust,
            syscall_count: 0,
            prev_syscall_count: 0,
            file_writes: 0,
            denied_access: 0,
            threat_score: 0,
            quarantined: false,
            last_window_tick: 0,
            threat_history: Vec::new(),
        }
    }

    /// Regista uma syscall
    pub fn record_syscall(&mut self) {
        self.syscall_count += 1;
    }

    /// Regista uma escrita de ficheiro
    pub fn record_file_write(&mut self) {
        self.file_writes += 1;
    }

    /// Regista um acesso negado
    pub fn record_denied(&mut self, resource: &str) {
        self.denied_access += 1;
    }

    /// Decaimento do score de ameaça (comportamento normal reduz suspeita)
    pub fn decay_score(&mut self) {
        self.threat_score = self.threat_score.saturating_sub(2);
    }

    /// Adiciona score de ameaça (capped a 100)
    pub fn add_threat(&mut self, score: u32, kind: ThreatKind, tick: u64) {
        self.threat_score = (self.threat_score + score).min(100);
        if self.threat_history.len() >= 10 { self.threat_history.remove(0); }
        self.threat_history.push((tick, kind));
    }
}

// ─── Motor de Ameaças ────────────────────────────────────────

pub struct ThreatEngine {
    /// Perfis por PID
    profiles:       BTreeMap<Pid, ProcessProfile>,
    /// Log de eventos de ameaça
    events:         Vec<ThreatEvent>,
    /// Processos em quarentena
    quarantined:    Vec<Pid>,
    /// Janela de análise em ticks
    window_ticks:   u64,
    /// Estatísticas
    pub stats:      ThreatStats,
}

#[derive(Debug, Clone, Default)]
pub struct ThreatStats {
    pub total_events:     usize,
    pub terminated_procs: usize,
    pub quarantined_procs: usize,
    pub alerts_fired:     usize,
    pub scans_run:        usize,
}

impl ThreatEngine {
    pub const fn new() -> Self {
        Self {
            profiles:     BTreeMap::new(),
            events:       Vec::new(),
            quarantined:  Vec::new(),
            window_ticks: 120, // ~2 segundos a 60Hz
            stats:        ThreatStats {
                total_events: 0,
                terminated_procs: 0,
                quarantined_procs: 0,
                alerts_fired: 0,
                scans_run: 0,
            },
        }
    }

    /// Regista um novo processo para monitorização
    pub fn register_process(&mut self, pid: Pid, name: &str, trust: TrustLevel) {
        self.profiles.insert(pid, ProcessProfile::new(pid, name.to_string(), trust));
    }

    /// Remove um processo (terminou)
    pub fn unregister_process(&mut self, pid: Pid) {
        self.profiles.remove(&pid);
        self.quarantined.retain(|&p| p != pid);
    }

    /// Analisa todos os processos na janela atual
    pub fn analyze(&mut self, tick: u64) {
        self.stats.scans_run += 1;
        let mut threats: Vec<ThreatEvent> = Vec::new();

        for (pid, profile) in self.profiles.iter_mut() {
            // Pula processos do kernel
            if profile.trust == TrustLevel::Kernel { continue; }

            // Janela de análise
            if tick.saturating_sub(profile.last_window_tick) < self.window_ticks { continue; }
            profile.last_window_tick = tick;

            // ── Heurística 1: Syscall Flood ───────────────────
            let syscall_delta = profile.syscall_count
                .saturating_sub(profile.prev_syscall_count);
            if syscall_delta > 500 {
                let score = (syscall_delta / 10).min(60) as u32;
                let action = Self::score_to_action(profile.threat_score + score, &profile.trust);
                threats.push(ThreatEvent {
                    pid: *pid,
                    proc_name: profile.name.clone(),
                    kind: ThreatKind::SyscallFlood,
                    score,
                    tick,
                    action,
                });
                profile.add_threat(score, ThreatKind::SyscallFlood, tick);
            }
            profile.prev_syscall_count = profile.syscall_count;

            // ── Heurística 2: Escrita excessiva de ficheiros ──
            if profile.file_writes > 100 {
                let score = 50u32;
                let action = Self::score_to_action(profile.threat_score + score, &profile.trust);
                threats.push(ThreatEvent {
                    pid: *pid,
                    proc_name: profile.name.clone(),
                    kind: ThreatKind::AbnormalFileWrite,
                    score,
                    tick,
                    action,
                });
                profile.add_threat(score, ThreatKind::AbnormalFileWrite, tick);
                profile.file_writes = 0; // reset após deteção
            }

            // ── Heurística 3: Acessos negados repetidos ───────
            if profile.denied_access > 5 {
                let score = (profile.denied_access * 8).min(70) as u32;
                let action = Self::score_to_action(profile.threat_score + score, &profile.trust);
                threats.push(ThreatEvent {
                    pid: *pid,
                    proc_name: profile.name.clone(),
                    kind: ThreatKind::UnauthorizedAccess {
                        resource: "multiple".to_string()
                    },
                    score,
                    tick,
                    action,
                });
                profile.add_threat(score, ThreatKind::UnauthorizedAccess {
                    resource: "multiple".to_string() }, tick);
                profile.denied_access = 0;
            }

            // Decaimento natural do score
            profile.decay_score();
        }

        // Aplica respostas às ameaças detetadas
        for event in threats {
            self.respond(&event);
            self.events.push(event);
            if self.events.len() > 100 { self.events.remove(0); }
        }
    }

    fn score_to_action(total_score: u32, trust: &TrustLevel) -> ResponseAction {
        // Processos de sistema têm tolerância maior
        let threshold = match trust {
            TrustLevel::Kernel   => return ResponseAction::Log,
            TrustLevel::System   => 15,
            TrustLevel::User     => 0,
            TrustLevel::Untrusted => -20i32 as u32,
        };
        let adjusted = total_score.saturating_add(threshold);
        match adjusted {
            0..=20  => ResponseAction::Log,
            21..=50 => ResponseAction::Alert,
            51..=80 => ResponseAction::Throttle,
            81..=95 => ResponseAction::Quarantine,
            _       => ResponseAction::Terminate,
        }
    }

    fn respond(&mut self, event: &ThreatEvent) {
        self.stats.total_events += 1;
        match event.action {
            ResponseAction::Log => {
                crate::serial_println!("[THREAT][LOG] PID={} '{}' {} score={}",
                    event.pid, event.proc_name, event.kind.as_str(), event.score);
            }
            ResponseAction::Alert => {
                self.stats.alerts_fired += 1;
                crate::serial_println!("[THREAT][ALERT] ⚠ PID={} '{}' {} score={}",
                    event.pid, event.proc_name, event.kind.as_str(), event.score);
            }
            ResponseAction::Throttle => {
                self.stats.alerts_fired += 1;
                crate::serial_println!("[THREAT][THROTTLE] ⚠ PID={} '{}' CPU limitado score={}",
                    event.pid, event.proc_name, event.score);
                // Em Fase 4: reduz slice de CPU no scheduler
            }
            ResponseAction::Quarantine => {
                if !self.quarantined.contains(&event.pid) {
                    self.quarantined.push(event.pid);
                    self.stats.quarantined_procs += 1;
                    if let Some(p) = self.profiles.get_mut(&event.pid) {
                        p.quarantined = true;
                    }
                    crate::serial_println!("[THREAT][QUARANTINE] 🔒 PID={} '{}' isolado score={}",
                        event.pid, event.proc_name, event.score);
                }
            }
            ResponseAction::Terminate => {
                self.stats.terminated_procs += 1;
                crate::serial_println!("[THREAT][TERMINATE] 🚨 PID={} '{}' TERMINADO score={}",
                    event.pid, event.proc_name, event.score);
                crate::modules::scheduler::kill(event.pid, -1);
                self.unregister_process(event.pid);
            }
        }
    }

    pub fn recent_events(&self, n: usize) -> Vec<&ThreatEvent> {
        let start = self.events.len().saturating_sub(n);
        self.events[start..].iter().collect()
    }

    pub fn is_quarantined(&self, pid: Pid) -> bool {
        self.quarantined.contains(&pid)
    }

    pub fn release_quarantine(&mut self, pid: Pid) -> bool {
        let before = self.quarantined.len();
        self.quarantined.retain(|&p| p != pid);
        if let Some(p) = self.profiles.get_mut(&pid) { p.quarantined = false; }
        self.quarantined.len() < before
    }
}

// ─── Políticas Dinâmicas de Privacidade ──────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PrivacyLevel {
    /// Máxima abertura — tudo partilhado
    Open,
    /// Padrão — partilha controlada
    Balanced,
    /// Privado — mínimo de dados enviados
    Private,
    /// Lockdown — sem comunicação externa
    Lockdown,
}

impl PrivacyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrivacyLevel::Open     => "open",
            PrivacyLevel::Balanced => "balanced",
            PrivacyLevel::Private  => "private",
            PrivacyLevel::Lockdown => "lockdown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrivacyPolicy {
    pub level:          PrivacyLevel,
    /// Permite telemetria de diagnóstico
    pub telemetry:      bool,
    /// Permite sync P2P automático
    pub p2p_sync:       bool,
    /// Permite acesso de rede de apps de terceiros
    pub third_party_net:bool,
    /// Cifra todos os ficheiros em repouso
    pub encrypt_at_rest:bool,
    /// Limpa logs automaticamente após N ticks
    pub log_retention:  Option<u64>,
}

impl PrivacyPolicy {
    pub fn balanced() -> Self {
        Self {
            level: PrivacyLevel::Balanced,
            telemetry: true,
            p2p_sync: true,
            third_party_net: true,
            encrypt_at_rest: false,
            log_retention: Some(86400), // ~1 dia
        }
    }

    pub fn private() -> Self {
        Self {
            level: PrivacyLevel::Private,
            telemetry: false,
            p2p_sync: true,
            third_party_net: false,
            encrypt_at_rest: true,
            log_retention: Some(3600),
        }
    }

    pub fn lockdown() -> Self {
        Self {
            level: PrivacyLevel::Lockdown,
            telemetry: false,
            p2p_sync: false,
            third_party_net: false,
            encrypt_at_rest: true,
            log_retention: Some(0),
        }
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static THREAT_ENGINE: Spinlock<ThreatEngine> =
    Spinlock::new(ThreatEngine::new());

pub static PRIVACY_POLICY: Spinlock<PrivacyPolicy> =
    Spinlock::new(PrivacyPolicy { // const init
        level: PrivacyLevel::Balanced,
        telemetry: true,
        p2p_sync: true,
        third_party_net: true,
        encrypt_at_rest: false,
        log_retention: Some(86400),
    });

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    // Regista processos do kernel como confiáveis
    THREAT_ENGINE.lock().register_process(1, "kernel", TrustLevel::Kernel);
    THREAT_ENGINE.lock().register_process(2, "scheduler", TrustLevel::Kernel);
    crate::serial_println!("[THREAT] Motor de IA defensiva inicializado");
    crate::serial_println!("[THREAT] Heuristicas: syscall-flood | file-write | unauth-access");
    crate::serial_println!("[THREAT] Politica de privacidade: {}",
        PRIVACY_POLICY.lock().level.as_str());
}

pub fn register(pid: Pid, name: &str, trust: TrustLevel) {
    THREAT_ENGINE.lock().register_process(pid, name, trust);
}

pub fn unregister(pid: Pid) {
    THREAT_ENGINE.lock().unregister_process(pid);
}

pub fn record_syscall(pid: Pid) {
    if let Some(p) = THREAT_ENGINE.lock().profiles.get_mut(&pid) {
        p.record_syscall();
    }
}

pub fn record_file_write(pid: Pid) {
    if let Some(p) = THREAT_ENGINE.lock().profiles.get_mut(&pid) {
        p.record_file_write();
    }
}

pub fn record_denied(pid: Pid, resource: &str) {
    if let Some(p) = THREAT_ENGINE.lock().profiles.get_mut(&pid) {
        p.record_denied(resource);
    }
}

pub fn is_quarantined(pid: Pid) -> bool {
    THREAT_ENGINE.lock().is_quarantined(pid)
}

pub fn threat_tick(current_tick: u64) {
    THREAT_ENGINE.lock().analyze(current_tick);
}

pub fn set_privacy(level: PrivacyLevel) {
    let policy = match level {
        PrivacyLevel::Balanced => PrivacyPolicy::balanced(),
        PrivacyLevel::Private  => PrivacyPolicy::private(),
        PrivacyLevel::Lockdown => PrivacyPolicy::lockdown(),
        PrivacyLevel::Open     => PrivacyPolicy::balanced(),
    };
    crate::serial_println!("[THREAT] Politica de privacidade alterada: {}",
        policy.level.as_str());
    *PRIVACY_POLICY.lock() = policy;
}

pub fn stats() -> ThreatStats {
    THREAT_ENGINE.lock().stats.clone()
}

// ─── Demonstração Fase 3.3 ───────────────────────────────────

pub fn run_demo() {
    crate::serial_println!("\n[FASE3.3] === IA Defensiva + Seguranca Avancada ===");

    // Regista processos de utilizador para monitorização
    register(10, "user-app-1",  TrustLevel::User);
    register(11, "user-app-2",  TrustLevel::User);
    register(12, "untrusted",   TrustLevel::Untrusted);

    // Simula comportamento normal
    for _ in 0..50  { record_syscall(10); }
    for _ in 0..200 { record_syscall(11); }   // app 11 mais activa

    // Simula comportamento suspeito no processo 12
    for _ in 0..600 { record_syscall(12); }   // flood de syscalls
    for _ in 0..120 { record_file_write(12); } // escrita excessiva
    for _ in 0..8   { record_denied(12, "/sys/kernel"); } // acessos negados

    // Executa análise imediata (simula janela completa)
    {
        let mut engine = THREAT_ENGINE.lock();
        // Forçar análise ignorando janela de tempo
        if let Some(p) = engine.profiles.get_mut(&12) {
            p.last_window_tick = 0;
        }
        if let Some(p) = engine.profiles.get_mut(&11) {
            p.last_window_tick = 0;
        }
    }
    threat_tick(1000);

    let s = stats();
    crate::serial_println!("[FASE3.3] Ameacas: {} eventos | {} alertas | {} quarentenas | {} terminados",
        s.total_events, s.alerts_fired, s.quarantined_procs, s.terminated_procs);

    let engine = THREAT_ENGINE.lock();
    let recent = engine.recent_events(5);
    if !recent.is_empty() {
        crate::serial_println!("[FASE3.3] Ultimos eventos:");
        for e in recent {
            crate::serial_println!("[FASE3.3]   PID={} '{}' {} → {}",
                e.pid, e.proc_name, e.kind.as_str(), e.action.as_str());
        }
    }
    drop(engine);

    crate::serial_println!("[FASE3.3] Use 'threat' no shell para ver ameacas");
    crate::serial_println!("[FASE3.3] Use 'privacy <level>' para mudar politica");
    crate::serial_println!("[FASE3.3] ==========================================\n");
}
