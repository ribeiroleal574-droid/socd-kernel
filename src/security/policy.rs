// ============================================================
// SOC-D Kernel — Políticas de Segurança
// ============================================================
//
// Define regras de acesso globais do sistema.
// Na Fase 2, essas políticas serão dinâmicas e configuráveis
// pelo usuário via interface gráfica.
// ============================================================

/// Nível de abertura do sistema definido pelo usuário
/// (aparece na interface de privacidade do SOC-D)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrivacyLevel {
    /// Máxima segurança — apenas o essencial funciona
    Locked = 0,
    /// Balanceado — apps de confiança têm acesso normal
    Balanced = 1,
    /// Aberto — mais permissivo para desenvolvedores
    Open = 2,
}

impl Default for PrivacyLevel {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Política global do sistema
pub struct SystemPolicy {
    pub privacy_level: PrivacyLevel,
    pub allow_unsigned_modules: bool,
    pub require_sandbox_for_all: bool,
    pub log_all_violations: bool,
    pub auto_isolate_anomalies: bool,
}

impl Default for SystemPolicy {
    fn default() -> Self {
        Self {
            privacy_level: PrivacyLevel::Balanced,
            allow_unsigned_modules: false,  // Fase 2: verificação de assinatura
            require_sandbox_for_all: true,  // Sempre sandbox
            log_all_violations: true,       // Log completo
            auto_isolate_anomalies: true,   // IA isola processos suspeitos
        }
    }
}
