// ============================================================
// SOC-D Kernel — Módulo de Memória
// ============================================================

pub mod frame_allocator;  // Alocação de frames físicos
pub mod heap;             // Heap do kernel (alloc)
pub mod paging;           // Tabelas de páginas virtuais

use core::sync::atomic::{AtomicU64, Ordering};

/// Offset entre endereços físicos e virtuais no mapeamento completo
/// da RAM feito pelo bootloader (ver `boot_info.physical_memory_offset`
/// em main.rs). Guardado aqui para que qualquer driver que precise de
/// endereços físicos reais para DMA (ex: virtio-net) possa traduzir
/// endereços virtuais sem ter de passar este valor manualmente por
/// todo o código de inicialização.
static PHYS_MEM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Chamado uma única vez no arranque, logo depois do mapper de
/// paginação ser inicializado.
pub fn set_phys_mem_offset(offset: u64) {
    PHYS_MEM_OFFSET.store(offset, Ordering::SeqCst);
}

/// Offset físico↔virtual actual. 0 antes de `set_phys_mem_offset` ser
/// chamado (nunca deve ser usado antes disso).
pub fn phys_mem_offset() -> u64 {
    PHYS_MEM_OFFSET.load(Ordering::SeqCst)
}

/// Traduz um endereço virtual do kernel para o endereço físico real —
/// necessário para programar DMA (ex: endereços de virtqueues no
/// driver virtio-net) porque a memória do heap/estáticos do kernel
/// não está mapeada 1:1 com a física.
pub fn virt_to_phys(virt: x86_64::VirtAddr) -> Option<x86_64::PhysAddr> {
    paging::translate_addr(virt, x86_64::VirtAddr::new(phys_mem_offset()))
}
