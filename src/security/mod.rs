// ============================================================
// SOC-D Kernel — Módulo de Segurança
// ============================================================

pub mod sandbox;    // Isolamento de processos
pub mod policy;     // Políticas de acesso
pub mod threat;     // IA defensiva + deteção de ameaças (Fase 3.3)

/// Nível de confiança de um processo/módulo
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    /// Kernel — acesso total
    Kernel = 0,
    /// Sistema — serviços essenciais, acesso controlado
    System = 1,
    /// Usuário — apps normais, ambiente restrito
    User = 2,
    /// Não confiável — máxima restrição
    Untrusted = 3,
}

/// Evento de segurança registrado pelo subsistema
#[derive(Debug, Clone)]
pub struct SecurityEvent {
    pub kind: SecurityEventKind,
    pub source: &'static str,
    pub tick: u64,
}

#[derive(Debug, Clone)]
pub enum SecurityEventKind {
    SandboxInit,
    PolicyViolation { resource: &'static str },
    AnomalyDetected { score: u32 },
}
