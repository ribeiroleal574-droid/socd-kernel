#![allow(non_camel_case_types)]
// ============================================================
// SOC-D Kernel — ARM Exception Vectors (AArch64)
// ============================================================
//
// Em ARM64, não existe IDT como no x86_64.
// Em vez disso, existe a VBAR_EL1 (Vector Base Address Register)
// que aponta para uma tabela de 16 vetores de 128 bytes cada.
//
// Estrutura da tabela (cada entrada = 128 bytes de código):
//
// Offset  Tipo          Origem
// 0x000   Síncronos     SP_EL0 (stack nível 0)
// 0x080   IRQ           SP_EL0
// 0x100   FIQ           SP_EL0
// 0x180   SError        SP_EL0
// 0x200   Síncronos     SP_ELx (stack nível atual)
// 0x280   IRQ           SP_ELx ← mais comum no kernel
// 0x300   FIQ           SP_ELx
// 0x380   SError        SP_ELx
// 0x400   Síncronos     AArch64 nível inferior
// 0x480   IRQ           AArch64 nível inferior
// 0x500   FIQ           AArch64 nível inferior
// 0x580   SError        AArch64 nível inferior
// 0x600   Síncronos     AArch32 nível inferior
// 0x680   IRQ           AArch32 nível inferior
// 0x700   FIQ           AArch32 nível inferior
// 0x780   SError        AArch32 nível inferior
//
// Tipos de exceção ARM64:
//   Síncronos  — instrução inválida, SVC, data abort, page fault
//   IRQ        — interrupção de hardware (timer, UART, etc.)
//   FIQ        — Fast IRQ (alta prioridade, latência mínima)
//   SError     — System Error (ECC, bus error)
// ============================================================

/// Contexto de CPU salvo ao entrar em exceção (AArch64)
/// Corresponde ao "exception frame" empurrado na stack
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ExceptionContext {
    /// Registradores de uso geral X0–X29
    pub gpr: [u64; 30],
    /// Link register (X30) — endereço de retorno
    pub lr: u64,
    /// Exception Link Register — endereço que causou a exceção
    pub elr_el1: u64,
    /// Saved Program Status Register
    pub spsr_el1: u64,
    /// Fault Address Register (para data/instruction aborts)
    pub far_el1: u64,
    /// Exception Syndrome Register — tipo + info da exceção
    pub esr_el1: u64,
    /// Stack pointer no momento da exceção
    pub sp: u64,
}

impl ExceptionContext {
    /// Extrai a classe de exceção do ESR_EL1 (bits 31:26)
    pub fn exception_class(&self) -> ExceptionClass {
        let ec = (self.esr_el1 >> 26) & 0x3F;
        ExceptionClass::from_u32(ec as u32)
    }

    /// Extrai o Instruction Specific Syndrome (bits 24:0)
    pub fn iss(&self) -> u32 {
        (self.esr_el1 & 0x01FF_FFFF) as u32
    }
}

/// Classe de exceção ARM64 (campo EC do ESR_EL1)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExceptionClass {
    Unknown           = 0x00,
    WFx               = 0x01,  // WFI/WFE
    MCR_MRC           = 0x03,  // MCR/MRC para CP15
    SVC_AA64          = 0x15,  // SVC (System Call) de AArch64
    HVC_AA64          = 0x16,  // HVC (Hypervisor Call)
    SMC_AA64          = 0x17,  // SMC (Secure Monitor Call)
    MSR_MRS           = 0x18,  // MSR/MRS para registradores de sistema
    InstructionAbort  = 0x20,  // Fault de instrução (nível inferior)
    InstructionAbortEl= 0x21,  // Fault de instrução (nível atual)
    PCAlignFault      = 0x22,  // PC desalinhado
    DataAbort         = 0x24,  // Fault de dados (nível inferior)
    DataAbortEl       = 0x25,  // Fault de dados (nível atual)
    SPAlignFault      = 0x26,  // SP desalinhado
    FloatingPoint     = 0x2C,  // Exceção de ponto flutuante
    SError            = 0x2F,  // System Error
    BreakpointEl      = 0x31,  // Breakpoint (nível atual)
    StepEl            = 0x33,  // Single-step (nível atual)
    WatchpointEl      = 0x35,  // Watchpoint (nível atual)
    BRK               = 0x3C,  // Instrução BRK
}

impl ExceptionClass {
    pub fn from_u32(ec: u32) -> Self {
        match ec {
            0x00 => Self::Unknown,
            0x01 => Self::WFx,
            0x15 => Self::SVC_AA64,
            0x16 => Self::HVC_AA64,
            0x17 => Self::SMC_AA64,
            0x20 => Self::InstructionAbort,
            0x21 => Self::InstructionAbortEl,
            0x24 => Self::DataAbort,
            0x25 => Self::DataAbortEl,
            0x2C => Self::FloatingPoint,
            0x2F => Self::SError,
            0x31 => Self::BreakpointEl,
            0x3C => Self::BRK,
            _    => Self::Unknown,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Unknown          => "Unknown",
            Self::SVC_AA64         => "SVC (syscall)",
            Self::InstructionAbort => "Instruction Abort",
            Self::InstructionAbortEl => "Instruction Abort (EL)",
            Self::DataAbort        => "Data Abort",
            Self::DataAbortEl      => "Data Abort (EL)",
            Self::PCAlignFault     => "PC Alignment Fault",
            Self::SPAlignFault     => "SP Alignment Fault",
            Self::FloatingPoint    => "Floating Point",
            Self::SError           => "System Error (SError)",
            Self::BreakpointEl     => "Breakpoint",
            Self::BRK              => "BRK Instruction",
            _                     => "Other",
        }
    }
}

/// Handler de exceção síncrona ARM64
/// Chamado pelo assembly da tabela de vetores
pub fn handle_sync_exception(ctx: &ExceptionContext) {
    let class = ctx.exception_class();

    match class {
        ExceptionClass::BRK | ExceptionClass::BreakpointEl => {
            // Breakpoint — usado para debugging
            crate::serial_println!(
                "[ARM][EXC] Breakpoint em ELR=0x{:016x}", ctx.elr_el1
            );
        }

        ExceptionClass::SVC_AA64 => {
            // System Call — interface usuário → kernel
            // ISS contém o número do syscall
            let syscall_nr = ctx.iss();
            crate::serial_println!(
                "[ARM][SVC] Syscall #{} de ELR=0x{:016x}",
                syscall_nr, ctx.elr_el1
            );
            // TODO Fase 4: dispatch para tabela de syscalls SOC-D
        }

        ExceptionClass::DataAbort | ExceptionClass::DataAbortEl => {
            crate::serial_println!(
                "[ARM][FAULT] Data Abort: FAR=0x{:016x} ESR=0x{:016x}",
                ctx.far_el1, ctx.esr_el1
            );
            // TODO Fase 4: demand paging / CoW
            panic!("ARM Data Abort não tratado: FAR=0x{:x}", ctx.far_el1);
        }

        ExceptionClass::InstructionAbort | ExceptionClass::InstructionAbortEl => {
            crate::serial_println!(
                "[ARM][FAULT] Instruction Abort: FAR=0x{:016x}",
                ctx.far_el1
            );
            panic!("ARM Instruction Abort: FAR=0x{:x}", ctx.far_el1);
        }

        ExceptionClass::SError => {
            crate::serial_println!(
                "[ARM][SERROR] System Error: ESR=0x{:016x}", ctx.esr_el1
            );
            panic!("ARM SError não recuperável");
        }

        other => {
            crate::serial_println!(
                "[ARM][EXC] {:?} ({}) ELR=0x{:016x}",
                other, other.name(), ctx.elr_el1
            );
        }
    }
}

/// Handler de IRQ ARM64
pub fn handle_irq(_ctx: &ExceptionContext) {
    // Lê o interruptor do GIC (Generic Interrupt Controller)
    // GIC_HPPIR (Highest Priority Pending Interrupt Register)
    // Fase 3: integração real com GIC-400/GIC-600
    let irq_id = read_pending_irq();

    match irq_id {
        30 => {
            // IRQ 30: Timer físico (CNTP_EL0) — equivalente ao PIT no x86
            handle_arm_timer();
        }
        32..=1019 => {
            crate::serial_println!("[ARM][IRQ] IRQ #{} recebido", irq_id);
        }
        _ => {
            // Spurious interrupt
        }
    }

    // EOI (End Of Interrupt) — sinaliza fim para o GIC
    eoi_irq(irq_id);
}

/// Lê o IRQ pendente do GIC (simulado na Fase 3)
fn read_pending_irq() -> u32 {
    // Fase 4: ler de GIC_CPU_INTERFACE + 0x018 (HPPIR)
    // Por agora: retorna timer virtual
    30
}

/// Sinaliza fim de interrupção para o GIC
fn eoi_irq(_irq_id: u32) {
    // Fase 4: escrever em GIC_CPU_INTERFACE + 0x010 (EOIR)
}

/// Handler do timer ARM (Generic Timer)
fn handle_arm_timer() {
    // Reconfigura o comparador para o próximo tick
    // CNTP_TVAL_EL0: conta regressiva
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // 1ms a 1GHz = 1_000_000 ciclos
        core::arch::asm!(
            "msr cntp_tval_el0, {}",
            in(reg) 1_000_000u64
        );
    }

    // Chama o scheduler
    if crate::modules::scheduler::timer_tick() {
        crate::modules::scheduler::schedule();
    }

    // Tick do Gossip P2P
    // (seria chamado aqui se tivéssemos tick count)
}

/// Inicializa o Generic Timer ARM
pub fn init_timer() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // Habilita o timer físico (CNTP)
        // CNTP_CTL_EL0: bit 0 = ENABLE, bit 1 = IMASK, bit 2 = ISTATUS
        core::arch::asm!(
            "msr cntp_tval_el0, {val}",  // Carrega valor inicial
            "msr cntp_ctl_el0, {ctl}",   // Habilita
            val = in(reg) 1_000_000u64,
            ctl = in(reg) 1u64,          // ENABLE=1
        );
    }
    crate::serial_println!("[ARM] Generic Timer inicializado (1ms @ 1GHz)");
}

/// Configura a tabela de vetores de exceção
/// Define VBAR_EL1 para apontar para a tabela
pub fn init_exception_vectors() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // Em produção, __exception_vectors é um símbolo assembly
        // que contém a tabela alinhada a 2KB (conforme especificação ARM)
        extern "C" {
            static __exception_vectors: u8;
        }
        let vbar = &__exception_vectors as *const u8 as u64;
        core::arch::asm!("msr vbar_el1, {}", in(reg) vbar);
        core::arch::asm!("isb"); // Instruction Synchronization Barrier
    }
    crate::serial_println!("[ARM] Tabela de vetores de excecao configurada (VBAR_EL1)");
}
