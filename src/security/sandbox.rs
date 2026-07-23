extern crate alloc;
// ============================================================
// SOC-D Kernel — Sandbox de Processos
// ============================================================
//
// O sandbox é o primeiro pilar de segurança do SOC-D.
// Todo processo roda em ambiente isolado por padrão.
//
// Fase 1 (atual): Sandbox conceitual com rastreamento de estado
// Fase 2: Integração com tabelas de páginas para isolamento real
// Fase 3: Seccomp-BPF para filtro de syscalls por processo
// Fase 4: IA defensiva detectando anomalias comportamentais
// ============================================================

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;
use super::TrustLevel;

/// Identificador único de processo
pub type ProcessId = u64;

/// Contexto de sandbox de um processo
#[derive(Debug, Clone)]
pub struct SandboxContext {
    /// ID do processo
    pub pid: ProcessId,
    /// Nome do processo (para diagnóstico)
    pub name: String,
    /// Nível de confiança
    pub trust_level: TrustLevel,
    /// Permissões concedidas
    pub permissions: SandboxPermissions,
    /// Contagem de violações detectadas
    pub violation_count: u32,
    /// Score de anomalia (0–100, calculado pela IA na Fase 2)
    pub anomaly_score: u32,
}

/// Permissões granulares do sandbox
#[derive(Debug, Clone)]
pub struct SandboxPermissions {
    /// Pode ler do sistema de arquivos
    pub fs_read: bool,
    /// Pode escrever no sistema de arquivos
    pub fs_write: bool,
    /// Pode abrir conexões de rede
    pub network: bool,
    /// Pode comunicar com outros processos (IPC)
    pub ipc: bool,
    /// Pode acessar hardware diretamente
    pub hardware_access: bool,
    /// Pode carregar módulos adicionais
    pub load_modules: bool,
}

impl SandboxPermissions {
    /// Permissões mínimas — praticamente nada
    pub fn minimal() -> Self {
        Self {
            fs_read: false,
            fs_write: false,
            network: false,
            ipc: false,
            hardware_access: false,
            load_modules: false,
        }
    }

    /// Permissões padrão para apps de usuário
    pub fn user_default() -> Self {
        Self {
            fs_read: true,
            fs_write: true,  // Apenas no diretório do app
            network: true,
            ipc: true,
            hardware_access: false,
            load_modules: false,
        }
    }

    /// Permissões para serviços do sistema
    pub fn system() -> Self {
        Self {
            fs_read: true,
            fs_write: true,
            network: true,
            ipc: true,
            hardware_access: true,
            load_modules: true,
        }
    }
}

/// Gerenciador de sandboxes
pub struct SandboxManager {
    contexts: Vec<SandboxContext>,
    next_pid: ProcessId,
    initialized: bool,
}

impl SandboxManager {
    const fn new() -> Self {
        Self {
            contexts: Vec::new(),
            next_pid: 1,
            initialized: false,
        }
    }

    /// Cria um novo contexto de sandbox para um processo
    pub fn create_sandbox(
        &mut self,
        name: &str,
        trust_level: TrustLevel,
    ) -> ProcessId {
        let pid = self.next_pid;
        self.next_pid += 1;

        let permissions = match trust_level {
            TrustLevel::Kernel | TrustLevel::System => SandboxPermissions::system(),
            TrustLevel::User => SandboxPermissions::user_default(),
            TrustLevel::Untrusted => SandboxPermissions::minimal(),
        };

        self.contexts.push(SandboxContext {
            pid,
            name: alloc::string::ToString::to_string(name),
            trust_level,
            permissions,
            violation_count: 0,
            anomaly_score: 0,
        });

        pid
    }

    /// Verifica se um processo tem permissão para uma ação.
    /// Registra violações quando negado.
    pub fn check_permission(&mut self, pid: ProcessId, permission: &str) -> bool {
        if let Some(ctx) = self.contexts.iter_mut().find(|c| c.pid == pid) {
            let allowed = match permission {
                "fs_read"         => ctx.permissions.fs_read,
                "fs_write"        => ctx.permissions.fs_write,
                "network"         => ctx.permissions.network,
                "ipc"             => ctx.permissions.ipc,
                "hardware_access" => ctx.permissions.hardware_access,
                "load_modules"    => ctx.permissions.load_modules,
                _ => false,
            };

            if !allowed {
                ctx.violation_count += 1;
                crate::serial_println!(
                    "[SECURITY] PID {} ({}) violacao: {} | total: {}",
                    pid, ctx.name, permission, ctx.violation_count
                );
            }

            allowed
        } else {
            // Processo desconhecido — negar por padrão
            crate::serial_println!("[SECURITY] PID desconhecido: {} | negado", pid);
            false
        }
    }

    /// Remove sandbox quando processo termina
    pub fn destroy_sandbox(&mut self, pid: ProcessId) {
        self.contexts.retain(|c| c.pid != pid);
    }

    /// Retorna estatísticas de segurança
    pub fn stats(&self) -> SandboxStats {
        SandboxStats {
            active_sandboxes: self.contexts.len(),
            total_violations: self.contexts.iter().map(|c| c.violation_count as u64).sum(),
            high_risk_processes: self.contexts.iter()
                .filter(|c| c.anomaly_score > 70 || c.violation_count > 5)
                .count(),
        }
    }
}

#[derive(Debug)]
pub struct SandboxStats {
    pub active_sandboxes: usize,
    pub total_violations: u64,
    pub high_risk_processes: usize,
}

/// Gerenciador global de sandbox
static SANDBOX_MANAGER: Spinlock<SandboxManager> = Spinlock::new(SandboxManager::new());

/// Inicializa o subsistema de segurança.
pub fn init() {
    let mut mgr = SANDBOX_MANAGER.lock();
    if mgr.initialized {
        return;
    }

    // Cria sandbox para o processo kernel (confiança total)
    let kernel_pid = mgr.create_sandbox("kernel", TrustLevel::Kernel);
    mgr.initialized = true;

    crate::serial_println!("[SECURITY] Sandbox iniciado. Kernel PID: {}", kernel_pid);
}

/// API pública para criar sandbox para novos processos
pub fn create_process_sandbox(name: &str, trust_level: TrustLevel) -> ProcessId {
    SANDBOX_MANAGER.lock().create_sandbox(name, trust_level)
}

/// API pública para verificar permissões
pub fn check_permission(pid: ProcessId, permission: &str) -> bool {
    SANDBOX_MANAGER.lock().check_permission(pid, permission)
}

/// API pública para estatísticas
pub fn get_stats() -> SandboxStats {
    SANDBOX_MANAGER.lock().stats()
}
