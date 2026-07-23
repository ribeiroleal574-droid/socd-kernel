// ============================================================
// SOC-D Kernel — Módulo de Memória
// ============================================================

pub mod frame_allocator;  // Alocação de frames físicos
pub mod heap;             // Heap do kernel (alloc)
pub mod paging;           // Tabelas de páginas virtuais
