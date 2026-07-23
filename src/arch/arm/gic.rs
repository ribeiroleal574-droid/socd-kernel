// ============================================================
// SOC-D Kernel — GIC (Generic Interrupt Controller) ARM
// ============================================================
//
// O GIC é o controlador de interrupções padrão ARM.
// Equivalente ao PIC 8259 / APIC no x86.
//
// Versões:
//   GIC-400  — ARMv7/ARMv8 básico (Raspberry Pi 4)
//   GIC-500  — ARMv8.x multi-cluster
//   GIC-600  — ARMv8.x/ARMv9 (Cortex-A78, X2)
//   GIC-700  — ARMv9 (Cortex-A710, X3)
//
// Componentes do GIC:
//   GICD — Distributor: configura prioridades, afinidade, enable/disable
//   GICC — CPU Interface: cada core tem sua interface (ou GICv3 GICC via ICC_* regs)
//   GICR — Redistributor (GICv3+): um por PE (Processing Element)
//
// Tipos de interrupções:
//   SGI  (0–15)   — Software Generated Interrupts (IPI entre cores)
//   PPI  (16–31)  — Private Peripheral Interrupts (timer, PMU por core)
//   SPI  (32–1019) — Shared Peripheral Interrupts (UART, Ethernet, etc.)
//   LPI  (8192+)  — Locality-specific Peripheral Interrupts (GICv3+, PCIe MSI)
//
// Endereços base típicos:
//   GIC-400 RPi4:  GICD=0xFF841000, GICC=0xFF842000
//   QEMU virt:     GICD=0x08000000, GICC=0x08010000, GICR=0x080A0000
// ============================================================

use spinning_top::Spinlock;

/// Endereços do GIC para QEMU virt machine (padrão de desenvolvimento)
pub const GICD_BASE:  u64 = 0x0800_0000;  // Distributor
pub const GICC_BASE:  u64 = 0x0801_0000;  // CPU Interface (GICv2)
pub const GICR_BASE:  u64 = 0x080A_0000;  // Redistributor (GICv3)

/// Offsets dos registradores do Distributor (GICD)
mod gicd {
    pub const CTLR:      u32 = 0x000;  // Control Register
    pub const TYPER:     u32 = 0x004;  // Type Register (num IRQs)
    pub const IIDR:      u32 = 0x008;  // Implementer ID
    pub const ISENABLER: u32 = 0x100;  // Interrupt Set-Enable (array)
    pub const ICENABLER: u32 = 0x180;  // Interrupt Clear-Enable (array)
    pub const ISPENDR:   u32 = 0x200;  // Interrupt Set-Pending
    pub const ICPENDR:   u32 = 0x280;  // Interrupt Clear-Pending
    pub const ISACTIVER: u32 = 0x300;  // Interrupt Set-Active
    pub const IPRIORITYR: u32 = 0x400; // Interrupt Priority (array, 8 bits cada)
    pub const ITARGETSR: u32 = 0x800;  // Interrupt Target (array, qual CPU)
    pub const ICFGR:     u32 = 0xC00;  // Interrupt Config (edge/level)
    pub const SGIR:      u32 = 0xF00;  // Software Generated Interrupt
}

/// Offsets dos registradores da CPU Interface (GICC)
mod gicc {
    pub const CTLR:  u32 = 0x000;  // CPU Interface Control
    pub const PMR:   u32 = 0x004;  // Priority Mask Register
    pub const BPR:   u32 = 0x008;  // Binary Point Register
    pub const IAR:   u32 = 0x00C;  // Interrupt Acknowledge Register
    pub const EOIR:  u32 = 0x010;  // End Of Interrupt Register
    pub const RPR:   u32 = 0x014;  // Running Priority Register
    pub const HPPIR: u32 = 0x018;  // Highest Priority Pending IRQ
    pub const DIR:   u32 = 0x1000; // Deactivate Interrupt Register
}

/// Prioridades de IRQ (ARM: 0=mais alta, 255=mais baixa)
pub const PRIORITY_MAX:    u8 = 0x00;
pub const PRIORITY_HIGH:   u8 = 0x40;
pub const PRIORITY_NORMAL: u8 = 0x80;
pub const PRIORITY_LOW:    u8 = 0xC0;
pub const PRIORITY_MIN:    u8 = 0xFE;
pub const PRIORITY_MASK:   u8 = 0xF0; // Threshold de aceitação

/// Número do IRQ do timer (PPI 30 = Generic Timer EL1 Physical)
pub const TIMER_IRQ: u32 = 30;

/// Número do IRQ da UART (SPI, varia por plataforma)
pub const UART_IRQ_QEMU: u32 = 33;
pub const UART_IRQ_RPI4: u32 = 125;

// ─── Leitura/Escrita de Registradores ────────────────────────────────────────

/// Lê um registrador do GICD
#[inline]
unsafe fn gicd_read(offset: u32) -> u32 {
    let addr = (GICD_BASE + offset as u64) as *const u32;
    core::ptr::read_volatile(addr)
}

/// Escreve em um registrador do GICD
#[inline]
unsafe fn gicd_write(offset: u32, value: u32) {
    let addr = (GICD_BASE + offset as u64) as *mut u32;
    core::ptr::write_volatile(addr, value);
}

/// Lê um registrador do GICC
#[inline]
unsafe fn gicc_read(offset: u32) -> u32 {
    let addr = (GICC_BASE + offset as u64) as *const u32;
    core::ptr::read_volatile(addr)
}

/// Escreve em um registrador do GICC
#[inline]
unsafe fn gicc_write(offset: u32, value: u32) {
    let addr = (GICC_BASE + offset as u64) as *mut u32;
    core::ptr::write_volatile(addr, value);
}

// ─── Driver GIC ───────────────────────────────────────────────────────────────

pub struct GicDriver {
    pub initialized: bool,
    pub version: u8,        // 2 ou 3
    pub num_irqs: u32,
    pub num_cpus: u32,
    pub pending_irqs: [bool; 1020],
}

impl GicDriver {
    const fn new() -> Self {
        Self {
            initialized: false,
            version: 2,
            num_irqs: 0,
            num_cpus: 0,
            pending_irqs: [false; 1020],
        }
    }

    /// Inicializa o GIC — configura Distributor e CPU Interface
    pub fn init(&mut self) {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            self.init_hardware();
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // Stub para desenvolvimento em x86_64
            self.num_irqs = 256;
            self.num_cpus = 4;
        }

        self.initialized = true;
        crate::serial_println!(
            "[ARM][GIC] GIC-v{} inicializado: {} IRQs, {} CPUs",
            self.version, self.num_irqs, self.num_cpus
        );
    }

    #[cfg(target_arch = "aarch64")]
    unsafe fn init_hardware(&mut self) {
        // Lê tipo do GIC: número de IRQs e CPUs
        let typer = gicd_read(gicd::TYPER);
        self.num_irqs = ((typer & 0x1F) + 1) * 32;
        self.num_cpus = ((typer >> 5) & 0x07) + 1;

        // ── Passo 1: Desabilita o Distributor durante configuração
        gicd_write(gicd::CTLR, 0);

        // ── Passo 2: Configura todas as SPIs (32–num_irqs)
        let n_words = (self.num_irqs / 32) as u32;
        for i in 1..n_words {
            // Desabilita todos os IRQs inicialmente
            gicd_write(gicd::ICENABLER + i * 4, 0xFFFF_FFFF);
            // Prioridade normal para todos
            for j in 0..8 {
                let off = gicd::IPRIORITYR + (i * 32 + j * 4) * (8 / 8);
                gicd_write(off, 0xA0A0_A0A0); // PRIORITY_NORMAL × 4
            }
        }

        // ── Passo 3: Configura PPIs (16–31) para este core
        // Desabilita PPIs inicialmente, exceto timer
        gicd_write(gicd::ICENABLER, 0xFFFF_0000); // Desabilita SGIs 0-15 e PPIs

        // Configura prioridade do timer (PPI 30) como alta
        let timer_prio_reg = gicd::IPRIORITYR + (TIMER_IRQ / 4) * 4;
        let timer_prio_shift = (TIMER_IRQ % 4) * 8;
        let current = gicd_read(timer_prio_reg);
        let mask = !(0xFF << timer_prio_shift);
        gicd_write(timer_prio_reg,
            (current & mask) | ((PRIORITY_HIGH as u32) << timer_prio_shift));

        // Habilita timer (PPI 30): bit 30 do ISENABLER[0]
        gicd_write(gicd::ISENABLER, 1 << TIMER_IRQ);

        // ── Passo 4: Habilita o Distributor
        gicd_write(gicd::CTLR, 1); // Enable Group 1 (non-secure)

        // ── Passo 5: Configura CPU Interface
        // PMR: aceita IRQs com prioridade ≤ PRIORITY_MASK (threshold)
        gicc_write(gicc::PMR, PRIORITY_MASK as u32);
        // BPR: sem preempção por subgrupo
        gicc_write(gicc::BPR, 0);
        // CTLR: habilita CPU Interface
        gicc_write(gicc::CTLR, 1);
    }

    /// Habilita um IRQ específico
    pub fn enable_irq(&self, irq: u32) {
        if irq >= self.num_irqs { return; }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let reg = gicd::ISENABLER + (irq / 32) * 4;
            gicd_write(reg, 1 << (irq % 32));
        }
        crate::serial_println!("[ARM][GIC] IRQ {} habilitado", irq);
    }

    /// Desabilita um IRQ específico
    pub fn disable_irq(&self, irq: u32) {
        if irq >= self.num_irqs { return; }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let reg = gicd::ICENABLER + (irq / 32) * 4;
            gicd_write(reg, 1 << (irq % 32));
        }
    }

    /// Lê o IRQ mais prioritário pendente (IAR — Interrupt Acknowledge)
    /// Esta operação "aceita" o IRQ, marcando-o como ativo no GIC
    pub fn acknowledge(&self) -> u32 {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            gicc_read(gicc::IAR) & 0x3FF  // bits 9:0 = IRQ ID
        }
        #[cfg(not(target_arch = "aarch64"))]
        TIMER_IRQ
    }

    /// Finaliza o tratamento de um IRQ (EOIR — End Of Interrupt)
    pub fn end_of_interrupt(&self, _irq: u32) {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            gicc_write(gicc::EOIR, irq);
        }
    }

    /// Envia SGI (Software Generated Interrupt) para outro core
    /// Usado para IPI (Inter-Processor Interrupts)
    pub fn send_sgi(&self, target_cpu: u32, sgi_id: u32) {
        if sgi_id >= 16 || target_cpu >= self.num_cpus { return; }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // SGIR: [25:24]=target_filter, [23:16]=cpu_target_list, [3:0]=SGIINTID
            let value = (1u32 << target_cpu) << 16 | sgi_id;
            gicd_write(gicd::SGIR, value);
        }
        crate::serial_println!("[ARM][GIC] SGI {} enviado para CPU {}", sgi_id, target_cpu);
    }
}

static GIC: Spinlock<GicDriver> = Spinlock::new(GicDriver::new());

/// Inicializa o GIC global
pub fn init() {
    GIC.lock().init();
}

/// Habilita um IRQ
pub fn enable_irq(irq: u32) {
    GIC.lock().enable_irq(irq);
}

/// Aceita o IRQ pendente e retorna seu número
pub fn acknowledge() -> u32 {
    GIC.lock().acknowledge()
}

/// Sinaliza fim de tratamento de IRQ
pub fn end_of_interrupt(irq: u32) {
    GIC.lock().end_of_interrupt(irq);
}

/// Envia IPI para outro core
pub fn send_ipi(target_cpu: u32, sgi_id: u32) {
    GIC.lock().send_sgi(target_cpu, sgi_id);
}
