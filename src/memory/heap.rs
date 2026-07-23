// ============================================================
// SOC-D Kernel — Heap Allocator
// ============================================================
//
// O heap do kernel permite usar Box<T>, Vec<T>, String, etc.
// Usa linked_list_allocator: simples e adequado para kernel.
//
// Localização: endereço virtual fixo (4 MB no espaço virtual)
// Tamanho: 1 MB (configurável)
//
// TODO Fase 2: implementar slab allocator para objetos frequentes
// ============================================================

use linked_list_allocator::LockedHeap;
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

/// Endereço virtual inicial do heap do kernel
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Tamanho do heap: 16 MB
/// Aumentado na Fase 7 — 8MB insuficiente com todos os subsistemas activos
pub const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MB

/// Alocador global do kernel.
/// #[global_allocator] registra este como o alocador padrão do Rust,
/// permitindo uso de Box, Vec, String, etc. no kernel.
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Inicializa o heap do kernel.
///
/// Mapeia as páginas virtuais do heap para frames físicos reais,
/// e inicializa o alocador com o intervalo de endereços.
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // Cria o range de páginas virtuais para o heap
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // Mapeia cada página para um frame físico
    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;

        // Flags: presente + leitura/escrita (sem execução)
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush()
        };
    }

    // Inicializa o alocador com o intervalo de endereços mapeado
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}

/// Estatísticas do heap (para diagnóstico)
pub fn heap_stats() -> (usize, usize) {
    let allocator = ALLOCATOR.lock();
    let used = allocator.used();
    let free = allocator.free();
    (used, free)
}
