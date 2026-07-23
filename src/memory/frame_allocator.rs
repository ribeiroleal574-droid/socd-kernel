// ============================================================
// SOC-D Kernel — Frame Allocator (bootloader 0.9 API)
// ============================================================

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{
    structures::paging::{FrameAllocator, PhysFrame, Size4KiB},
    PhysAddr,
};

/// Alocador de frames baseado no mapa de memória do bootloader 0.9
pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

impl BootInfoFrameAllocator {
    /// Cria um novo alocador com o mapa de memória do boot.
    ///
    /// # Safety
    /// O caller deve garantir que os frames fornecidos são válidos.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        Self { memory_map, next: 0 }
    }

    /// Itera sobre todos os frames utilizáveis (RAM disponível).
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        self.memory_map
            .iter()
            .filter(|r| r.region_type == MemoryRegionType::Usable)
            .map(|r| r.range.start_addr()..r.range.end_addr())
            .flat_map(|r| r.step_by(4096))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }

    pub fn total_memory_mb(&self) -> usize {
        self.usable_frames().count() * 4 / 1024
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
