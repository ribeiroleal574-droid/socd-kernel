// ============================================================
// SOC-D Kernel — Suporte ARM (AArch64)
// ============================================================
//
// Permite ao SOC-D rodar em hardware ARM:
//   - Raspberry Pi 4/5 (BCM2711/BCM2712)
//   - Apple Silicon (M1/M2/M3) via VM
//   - Qualcomm Snapdragon (dispositivos móveis)
//   - NVIDIA Jetson (edge computing)
//
// Diferenças ARM vs x86_64:
//   - Sem GDT/IDT — usa tabelas de vetores de exceção
//   - Interrupções via GIC (Generic Interrupt Controller)
//   - MMU: VMSA (Virtual Memory System Architecture)
//   - Registradores: X0–X30, SP, PC, CPSR/SPSR
//   - Instruções: ISA AArch64 (A64)
//
// Fase 3 (atual): Estruturas e abstrações ARM
// Fase 4: Build target aarch64-unknown-none
// ============================================================

pub mod exception;  // Tabela de vetores de exceção ARM
pub mod gic;        // Generic Interrupt Controller
pub mod mmu;        // MMU e tabelas de página ARM

/// Arquitetura atual detectada em compile time
#[cfg(target_arch = "x86_64")]
pub const ARCH: &str = "x86_64";

#[cfg(target_arch = "aarch64")]
pub const ARCH: &str = "aarch64";

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const ARCH: &str = "unknown";

/// Informações da CPU ARM detectadas em runtime
#[derive(Debug, Clone)]
pub struct ArmCpuInfo {
    /// Implementador (ARM=0x41, Apple=0x61, Qualcomm=0x51)
    pub implementer: u8,
    /// Variante do core
    pub variant: u8,
    /// Arquitetura (8=ARMv8, 9=ARMv9)
    pub architecture: u8,
    /// Número do part (Cortex-A55=0xD05, Cortex-A78=0xD42)
    pub part_number: u16,
    /// Revisão
    pub revision: u8,
    /// Número de cores
    pub core_count: u32,
    /// Frequência em MHz
    pub freq_mhz: u32,
    /// Tem NEON/SVE (SIMD)?
    pub has_simd: bool,
    /// Tem hardware crypto?
    pub has_crypto: bool,
    /// Tem pointer authentication?
    pub has_pauth: bool,
}

impl ArmCpuInfo {
    /// Lê informações da CPU via MIDR_EL1 (Main ID Register)
    /// Em x86_64 retorna stub para compatibilidade
    pub fn read() -> Self {
        #[cfg(target_arch = "aarch64")]
        {
            let midr: u64;
            unsafe {
                core::arch::asm!("mrs {}, midr_el1", out(reg) midr);
            }
            Self {
                implementer:  ((midr >> 24) & 0xFF) as u8,
                variant:      ((midr >> 20) & 0x0F) as u8,
                architecture: ((midr >> 16) & 0x0F) as u8,
                part_number:  ((midr >>  4) & 0xFFF) as u16,
                revision:      (midr & 0x0F) as u8,
                core_count: read_core_count(),
                freq_mhz: 0, // ARM: lido do Device Tree
                has_simd:   true,
                has_crypto: true,
                has_pauth:  false,
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        Self {
            implementer: 0x41,   // Stub: ARM Ltd
            variant: 0,
            architecture: 8,
            part_number: 0xD05,  // Cortex-A55
            revision: 0,
            core_count: 4,
            freq_mhz: 1800,
            has_simd: true,
            has_crypto: true,
            has_pauth: false,
        }
    }

    pub fn implementer_name(&self) -> &'static str {
        match self.implementer {
            0x41 => "ARM Ltd",
            0x61 => "Apple",
            0x51 => "Qualcomm",
            0x4E => "NVIDIA",
            0x56 => "Marvell",
            _    => "Unknown",
        }
    }

    pub fn part_name(&self) -> &'static str {
        match self.part_number {
            0xD03 => "Cortex-A53",
            0xD05 => "Cortex-A55",
            0xD0B => "Cortex-A76",
            0xD0C => "Neoverse-N1",
            0xD40 => "Neoverse-V1",
            0xD42 => "Cortex-A78",
            0xD44 => "Cortex-X1",
            0xD47 => "Cortex-A710",
            0xD4D => "Cortex-A715",
            0x001 => "Apple Firestorm", // Apple Silicon
            _     => "Unknown ARM Core",
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn read_core_count() -> u32 {
    // Lê do MPIDR_EL1 — Aff2 contém cluster, Aff0 contém core
    let mpidr: u64;
    unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr); }
    ((mpidr >> 8) & 0xFF) as u32 + 1
}

/// Configuração do target ARM para o build system
pub struct ArmBuildTarget {
    pub triple:      &'static str,
    pub linker:      &'static str,
    pub features:    &'static str,
    pub qemu_machine: &'static str,
    pub qemu_cpu:    &'static str,
}

pub const RASPBERRY_PI4: ArmBuildTarget = ArmBuildTarget {
    triple:       "aarch64-unknown-none",
    linker:       "aarch64-linux-gnu-ld",
    features:     "+neon,+crypto,+crc",
    qemu_machine: "raspi4b",
    qemu_cpu:     "cortex-a72",
};

pub const QEMU_VIRT: ArmBuildTarget = ArmBuildTarget {
    triple:       "aarch64-unknown-none",
    linker:       "aarch64-linux-gnu-ld",
    features:     "+neon",
    qemu_machine: "virt",
    qemu_cpu:     "cortex-a57",
};

pub fn init() {
    let info = ArmCpuInfo::read();
    crate::serial_println!("[ARM] Suporte AArch64 inicializado");
    crate::serial_println!("[ARM] CPU: {} {} (arch=ARMv{}, {} cores)",
        info.implementer_name(), info.part_name(),
        info.architecture, info.core_count);
    crate::serial_println!("[ARM] SIMD={} CRYPTO={} PAUTH={}",
        info.has_simd, info.has_crypto, info.has_pauth);
}
