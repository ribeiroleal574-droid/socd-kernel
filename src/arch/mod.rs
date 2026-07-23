// ============================================================
// SOC-D Kernel — Módulo de Arquitetura
// Abstrações específicas para x86_64
// ============================================================

pub mod gdt;          // Global Descriptor Table
pub mod interrupts;   // IDT + handlers de interrupção
pub mod port;         // Acesso a portas de I/O

/// Inicializa todos os subsistemas de arquitetura em ordem.
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

pub mod arm;  // ARM AArch64 (Fase 3)
