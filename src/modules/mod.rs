extern crate alloc;
// ============================================================
// SOC-D Kernel — Sistema de Módulos Dinâmicos
// ============================================================
//
// O sistema de módulos é o coração do SOC-D.
// Diferente de kernels monolíticos (Linux) onde tudo é
// compilado junto, aqui módulos podem ser:
//   - Registrados em tempo de compilação (built-in)
//   - Carregados em runtime (TODO: ELF loader na Fase 2)
//   - Descarregados quando não usados (economia de memória)
//
// Cada módulo declara:
//   - Nome e versão
//   - Dependências (outros módulos necessários)
//   - Funções init() e cleanup()
//   - Capacidades que oferece (para o sistema de IA na Fase 2)
// ============================================================

pub mod registry;    // Registro global de módulos
pub mod elf_loader;  // Carregamento de módulos ELF em runtime
pub mod scheduler;   // Scheduler preemptivo de processos
pub mod tmpfs;       // Sistema de arquivos em RAM
pub mod process;     // Gestor de processos dinâmicos (Fase 2)
pub mod virt;        // Virtualização leve / containers (Fase 3.2)
pub mod xdev;        // Interface cross-device PC↔mobile↔AR (Fase 3.4)
pub mod monitor;     // Monitor de recursos em tempo real (Fase 6.1)
pub mod tests;       // Suite de testes automatizados (Fase 6.3)

use alloc::string::String;

/// Estado atual de um módulo no sistema
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleState {
    /// Registrado mas não inicializado
    Registered,
    /// Sendo inicializado
    Loading,
    /// Ativo e funcionando
    Active,
    /// Temporariamente suspenso (liberou recursos)
    Suspended,
    /// Ocorreu um erro durante init
    Failed(String),
    /// Descarregado (cleanup executado)
    Unloaded,
}

/// Prioridade de carregamento do módulo
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModulePriority {
    /// Essencial — deve carregar antes de tudo
    Critical = 0,
    /// Alta — serviços essenciais do kernel
    High = 1,
    /// Normal — serviços padrão
    Normal = 2,
    /// Baixa — serviços opcionais
    Low = 3,
}

/// Interface que todo módulo SOC-D deve implementar.
/// Define o contrato entre o kernel e os módulos.
pub trait KernelModule: Send + Sync {
    /// Nome único do módulo (ex: "security.sandbox")
    fn name(&self) -> &'static str;

    /// Versão semântica (ex: "0.1.0")
    fn version(&self) -> &'static str;

    /// Descrição humana do módulo
    fn description(&self) -> &'static str;

    /// Lista de módulos dos quais este depende
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    /// Prioridade de carregamento
    fn priority(&self) -> ModulePriority {
        ModulePriority::Normal
    }

    /// Inicializa o módulo. Retorna Ok(()) ou Err com mensagem.
    /// Chamado pelo kernel ao carregar o módulo.
    fn init(&self) -> Result<(), &'static str>;

    /// Libera recursos do módulo.
    /// Chamado antes de descarregar (ou ao suspender).
    fn cleanup(&self);

    /// Retorna informações de status (para diagnóstico e IA)
    fn status_info(&self) -> ModuleStatusInfo {
        ModuleStatusInfo {
            name: self.name(),
            version: self.version(),
            priority: self.priority(),
            memory_usage_kb: 0, // Override para reportar uso real
        }
    }
}

/// Informações de status de um módulo (usadas pelo motor de IA na Fase 2)
#[derive(Debug, Clone)]
pub struct ModuleStatusInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub priority: ModulePriority,
    pub memory_usage_kb: usize,
}

// ─── Módulos Built-in da Fase 1 ────────────────────────────────────────────

/// Módulo de Segurança — Sandbox e controle de acesso
struct SecurityModule;
impl KernelModule for SecurityModule {
    fn name(&self) -> &'static str { "security.sandbox" }
    fn version(&self) -> &'static str { "0.1.0" }
    fn description(&self) -> &'static str { "Sandbox de processos e controle de acesso basico" }
    fn priority(&self) -> ModulePriority { ModulePriority::Critical }
    fn init(&self) -> Result<(), &'static str> {
        crate::security::sandbox::init();
        Ok(())
    }
    fn cleanup(&self) {
        // Segurança nunca é desativada em runtime
    }
}

/// Módulo de Driver de Teclado
struct KeyboardModule;
impl KernelModule for KeyboardModule {
    fn name(&self) -> &'static str { "driver.keyboard" }
    fn version(&self) -> &'static str { "0.1.0" }
    fn description(&self) -> &'static str { "Driver PS/2 de teclado" }
    fn priority(&self) -> ModulePriority { ModulePriority::High }
    fn init(&self) -> Result<(), &'static str> {
        crate::drivers::keyboard::init();
        Ok(())
    }
    fn cleanup(&self) {}
}

/// Módulo de VGA / Display
struct VgaModule;
impl KernelModule for VgaModule {
    fn name(&self) -> &'static str { "driver.vga" }
    fn version(&self) -> &'static str { "0.1.0" }
    fn description(&self) -> &'static str { "Driver de texto VGA 80x25" }
    fn priority(&self) -> ModulePriority { ModulePriority::Critical }
    fn init(&self) -> Result<(), &'static str> {
        crate::drivers::vga::init();
        Ok(())
    }
    fn cleanup(&self) {}
}

/// Carrega todos os módulos built-in da Fase 1.
/// Chamado durante o boot pelo kernel_main.
pub fn load_builtin_modules() {
    let reg = registry::REGISTRY.lock();

    // Módulos são carregados em ordem de prioridade
    let modules: &[&dyn KernelModule] = &[
        &VgaModule,
        &SecurityModule,
        &KeyboardModule,
    ];

    drop(reg); // Libera o lock antes de chamar init()

    for module in modules {
        registry::register_and_init(*module);
    }
}
