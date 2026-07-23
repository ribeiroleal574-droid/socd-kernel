extern crate alloc;
// ============================================================
// SOC-D Kernel — Registro de Módulos
// ============================================================
//
// O registro centraliza informações sobre todos os módulos
// conhecidos pelo kernel. É a base para:
//   - Boot ordenado (respeita dependências e prioridades)
//   - Hot-loading: carregar módulos sem reiniciar
//   - Motor de IA: saber quais módulos estão ativos e seu consumo
//   - Diagnóstico: listar estado de todos os módulos
// ============================================================

use super::{KernelModule, ModuleState, ModuleStatusInfo};
use alloc::{string::ToString, vec::Vec};
use spinning_top::Spinlock;

/// Entrada no registro para um módulo
pub struct ModuleEntry {
    pub name: &'static str,
    pub state: ModuleState,
    pub status: ModuleStatusInfo,
}

/// Registro global de módulos do SOC-D
pub struct ModuleRegistry {
    entries: Vec<ModuleEntry>,
}

impl ModuleRegistry {
    const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Registra um módulo (sem inicializar ainda)
    pub fn register(&mut self, module: &dyn KernelModule) {
        // Evita duplicatas
        if self.entries.iter().any(|e| e.name == module.name()) {
            return;
        }
        self.entries.push(ModuleEntry {
            name: module.name(),
            state: ModuleState::Registered,
            status: module.status_info(),
        });
    }

    /// Atualiza o estado de um módulo
    pub fn set_state(&mut self, name: &str, state: ModuleState) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.state = state;
        }
    }

    /// Retorna o estado atual de um módulo
    pub fn get_state(&self, name: &str) -> Option<&ModuleState> {
        self.entries.iter().find(|e| e.name == name).map(|e| &e.state)
    }

    /// Retorna todos os módulos ativos
    pub fn active_modules(&self) -> Vec<&ModuleEntry> {
        self.entries
            .iter()
            .filter(|e| e.state == ModuleState::Active)
            .collect()
    }

    /// Retorna estatísticas do sistema de módulos
    pub fn stats(&self) -> RegistryStats {
        RegistryStats {
            total: self.entries.len(),
            active: self.entries.iter().filter(|e| e.state == ModuleState::Active).count(),
            failed: self.entries.iter().filter(|e| matches!(e.state, ModuleState::Failed(_))).count(),
        }
    }
}

#[derive(Debug)]
pub struct RegistryStats {
    pub total: usize,
    pub active: usize,
    pub failed: usize,
}

/// Registro global (protegido por spinlock)
pub static REGISTRY: Spinlock<ModuleRegistry> = Spinlock::new(ModuleRegistry::new());

/// Inicializa o sistema de registro.
pub fn init() {
    // Atualmente não precisa de init especial
    // Futuro: carregar módulos persistidos em storage
}

/// Registra e inicializa um módulo, atualizando o estado no registro.
pub fn register_and_init(module: &'static dyn KernelModule) {
    {
        let mut reg = REGISTRY.lock();
        reg.register(module);
        reg.set_state(module.name(), ModuleState::Loading);
    }

    match module.init() {
        Ok(()) => {
            let mut reg = REGISTRY.lock();
            reg.set_state(module.name(), ModuleState::Active);
            crate::serial_println!("[MOD] {} v{} — ativo", module.name(), module.version());
        }
        Err(e) => {
            let mut reg = REGISTRY.lock();
            reg.set_state(module.name(), ModuleState::Failed(e.to_string()));
            crate::serial_println!("[MOD][ERRO] {} — falha: {}", module.name(), e);
        }
    }
}
