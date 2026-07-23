// ============================================================
// SOC-D Kernel — ARM MMU (VMSA AArch64)
// ============================================================
//
// A MMU ARM64 usa VMSA (Virtual Memory System Architecture)
// com tabelas de página de 4 níveis (igual x86_64):
//
//   VA[47:39] → PGD (Page Global Directory) — nível 0
//   VA[38:30] → PUD (Page Upper Directory)  — nível 1
//   VA[29:21] → PMD (Page Middle Directory) — nível 2
//   VA[20:12] → PTE (Page Table Entry)      — nível 3
//   VA[11: 0] → offset dentro da página (4KB)
//
// Tamanhos de página suportados: 4KB, 16KB, 64KB
// SOC-D usa 4KB (padrão, maior granularidade)
//
// Dois espaços de endereço:
//   TTBR0_EL1 — espaço do usuário  (VA 0x0000_0000_0000_0000)
//   TTBR1_EL1 — espaço do kernel   (VA 0xFFFF_0000_0000_0000)
//
// Atributos de memória (MAIR_EL1):
//   Índice 0: Normal, Write-Back Cacheable (RAM)
//   Índice 1: Device-nGnRnE (MMIO, framebuffer)
//   Índice 2: Normal, Non-Cacheable
// ============================================================

/// Tamanho de página (4KB)
pub const PAGE_SIZE:  u64 = 4096;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_MASK:  u64 = !(PAGE_SIZE - 1);

/// Número de entradas por nível de tabela
pub const PTRS_PER_TABLE: usize = 512;

/// Endereço base do espaço do kernel (bit 47 = 1 → VA negativo)
pub const KERNEL_VA_BASE: u64 = 0xFFFF_0000_0000_0000;

/// Atributos de entrada de tabela de páginas ARM64
pub mod desc_bits {
    // Bits comuns
    pub const VALID:       u64 = 1 << 0;  // Entrada válida
    pub const TABLE:       u64 = 1 << 1;  // Aponta para próximo nível (vs bloco)
    pub const PAGE:        u64 = 1 << 1;  // Entrada de página (nível 3)

    // Atributos de memória
    pub const ATTR_NORMAL: u64 = 0 << 2;  // Índice 0: Normal cacheable
    pub const ATTR_DEVICE: u64 = 1 << 2;  // Índice 1: Device memory
    pub const ATTR_NC:     u64 = 2 << 2;  // Índice 2: Normal non-cacheable

    // Privilégio de acesso
    pub const AP_RW_EL1:   u64 = 0 << 6;  // R/W apenas kernel
    pub const AP_RW_ALL:   u64 = 1 << 6;  // R/W kernel e usuário
    pub const AP_RO_EL1:   u64 = 2 << 6;  // Read-only kernel
    pub const AP_RO_ALL:   u64 = 3 << 6;  // Read-only todos

    pub const NS:          u64 = 1 << 5;  // Non-Secure
    pub const SH_INNER:    u64 = 3 << 8;  // Inner Shareable
    pub const AF:          u64 = 1 << 10; // Access Flag (deve setar = 1)
    pub const NG:          u64 = 1 << 11; // Non-Global (processo específico)

    // Bits superiores
    pub const PXN:         u64 = 1 << 53; // Privileged Execute Never
    pub const UXN:         u64 = 1 << 54; // Unprivileged Execute Never
    pub const DIRTY:       u64 = 1 << 55; // Dirty bit (hardware/software)

    // Combinações comuns
    pub const KERNEL_RW_DATA: u64 =
        VALID | PAGE | ATTR_NORMAL | AP_RW_EL1 | SH_INNER | AF | UXN | PXN;

    pub const KERNEL_RX_CODE: u64 =
        VALID | PAGE | ATTR_NORMAL | AP_RO_EL1 | SH_INNER | AF | UXN;

    pub const KERNEL_DEVICE: u64 =
        VALID | PAGE | ATTR_DEVICE | AP_RW_EL1 | AF | UXN | PXN;

    pub const USER_RW: u64 =
        VALID | PAGE | ATTR_NORMAL | AP_RW_ALL | SH_INNER | AF | NG | UXN | PXN;

    pub const USER_RX: u64 =
        VALID | PAGE | ATTR_NORMAL | AP_RO_ALL | SH_INNER | AF | NG | UXN;
}

/// Uma entrada de tabela de páginas ARM64 (64 bits)
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const INVALID: Self = Self(0);

    pub fn new_page(phys_addr: u64, attrs: u64) -> Self {
        Self((phys_addr & PAGE_MASK) | attrs)
    }

    pub fn new_table(next_table_phys: u64) -> Self {
        Self((next_table_phys & PAGE_MASK) | desc_bits::VALID | desc_bits::TABLE)
    }

    pub fn is_valid(self) -> bool  { self.0 & desc_bits::VALID != 0 }
    pub fn is_table(self) -> bool  { self.0 & desc_bits::TABLE != 0 && self.is_valid() }
    pub fn is_page(self) -> bool   { self.is_valid() && !self.is_table() }

    pub fn phys_addr(self) -> u64  { self.0 & 0x0000_FFFF_FFFF_F000 }
    pub fn attrs(self) -> u64      { self.0 & !0x0000_FFFF_FFFF_F000 }
}

/// Tabela de páginas ARM64 (512 entradas de 8 bytes = 4KB)
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; PTRS_PER_TABLE],
}

impl PageTable {
    pub const fn empty() -> Self {
        Self { entries: [PageTableEntry::INVALID; PTRS_PER_TABLE] }
    }

    pub fn entry_mut(&mut self, idx: usize) -> &mut PageTableEntry {
        &mut self.entries[idx]
    }

    pub fn phys_addr(&self) -> u64 {
        self as *const Self as u64
    }
}

/// Extrai o índice de nível N de um endereço virtual
#[inline]
pub fn va_index(va: u64, level: u32) -> usize {
    // Nível 0: bits [47:39], nível 1: [38:30], nível 2: [29:21], nível 3: [20:12]
    let shift = 39 - level * 9;
    ((va >> shift) & 0x1FF) as usize
}

// ─── Configuração inicial da MMU ──────────────────────────────────────────────

/// Valores do MAIR_EL1 (Memory Attribute Indirection Register)
/// Define 3 perfis de memória (índices 0, 1, 2):
///   0: Normal WB (Write-Back Cacheable) — para RAM
///   1: Device nGnRnE — para MMIO, sem reordenamento
///   2: Normal NC (Non-Cacheable) — para DMA buffers
pub const MAIR_VALUE: u64 =
    (0xFF << 0)  |   // Índice 0: Normal WB RW-Allocate
    (0x00 << 8)  |   // Índice 1: Device nGnRnE
    (0x44 << 16);    // Índice 2: Normal Non-Cacheable

/// Valor do TCR_EL1 (Translation Control Register)
/// Configura tamanho do VA, tamanho de página, caminhamento de tabelas
pub const TCR_VALUE: u64 =
    (16 << 0)   |   // T0SZ=16: VA de 48 bits para TTBR0 (usuário)
    (16 << 16)  |   // T1SZ=16: VA de 48 bits para TTBR1 (kernel)
    (0 << 6)    |   // IRGN0=0b00: Inner Non-Cacheable (simplificado)
    (0 << 8)    |   // ORGN0=0b00: Outer Non-Cacheable
    (3 << 10)   |   // SH0=0b11: Inner Shareable
    (0 << 22)   |   // IRGN1=0b00
    (0 << 24)   |   // ORGN1=0b00
    (3 << 26)   |   // SH1=0b11
    (0b00 << 14)|   // TG0=4KB para TTBR0
    (0b10 << 30)|   // TG1=4KB para TTBR1
    (1u64 << 23)|   // EPD1=0: TTBR1 ativo
    (1u64 << 36);   // IPS=0b001: PA de 36 bits (64GB)

/// Habilita a MMU e configura os registradores de controle
pub fn enable_mmu(ttbr0_phys: u64, ttbr1_phys: u64) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            // Configura atributos de memória
            "msr mair_el1, {mair}",
            // Configura controle de tradução
            "msr tcr_el1, {tcr}",
            // Carrega tabelas de páginas
            "msr ttbr0_el1, {ttbr0}",
            "msr ttbr1_el1, {ttbr1}",
            // Barreira de instrução
            "isb",
            // Habilita MMU: SCTLR_EL1 bit 0 = M (MMU Enable)
            // bit 2 = C (Data Cache), bit 12 = I (Instruction Cache)
            "mrs x0, sctlr_el1",
            "orr x0, x0, #(1 << 0)",  // MMU
            "orr x0, x0, #(1 << 2)",  // D-Cache
            "orr x0, x0, #(1 << 12)", // I-Cache
            "msr sctlr_el1, x0",
            "isb",
            mair  = in(reg) MAIR_VALUE,
            tcr   = in(reg) TCR_VALUE,
            ttbr0 = in(reg) ttbr0_phys,
            ttbr1 = in(reg) ttbr1_phys,
            out("x0") _,
        );
    }
    crate::serial_println!("[ARM][MMU] MMU habilitada (TTBR0=0x{:x} TTBR1=0x{:x})",
        ttbr0_phys, ttbr1_phys);
}

/// Invalida TLB completo (todos os cores)
pub fn invalidate_tlb_all() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "dsb ishst",       // Data Synchronization Barrier
            "tlbi vmalle1is",  // Invalida TLB EL1, Inner Shareable
            "dsb ish",
            "isb",
        );
    }
}

/// Invalida TLB para endereço virtual específico
pub fn invalidate_tlb_va(_va: u64) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vaae1is, {va}",  // Invalida TLB para VA específico
            "dsb ish",
            "isb",
            va = in(reg) va >> 12,  // ARM espera VA >> 12
        );
    }
}

pub fn init() {
    crate::serial_println!("[ARM][MMU] Subsistema de memoria virtual ARM64");
    crate::serial_println!("[ARM][MMU] VMSA: 4 niveis, 4KB paginas, 48-bit VA");
    crate::serial_println!("[ARM][MMU] MAIR=0x{:016x} TCR=0x{:016x}",
        MAIR_VALUE, TCR_VALUE);
}
