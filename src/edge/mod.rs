extern crate alloc;
// ============================================================
// SOC-D Kernel — Edge Computing (Fase 4)
// ============================================================
//
// O módulo de Edge Computing permite ao SOC-D distribuir
// cargas de trabalho entre dispositivos fisicamente próximos,
// formando um cluster ad-hoc sem infraestrutura centralizada.
//
// Casos de uso:
//   - Inferência de IA distribuída (modelos grandes)
//   - Renderização 3D distribuída (AR/VR)
//   - Processamento de sensores IoT em tempo real
//   - Compilação distribuída de código
//   - Backup e sincronização local rápida
//
// Arquitetura:
//   ┌──────────────────────────────────────────────────────┐
//   │                  Task Dispatcher                     │
//   ├────────────────┬─────────────────┬───────────────────┤
//   │  Node Registry │  Load Balancer  │  Result Collector │
//   ├────────────────┴─────────────────┴───────────────────┤
//   │           Transport (P2P / mDNS discovery)           │
//   └──────────────────────────────────────────────────────┘
//
// Modelo de tarefa:
//   Task { id, payload, requirements, deadline }
//   → Dispatcher escolhe nó com base em capacidade + latência
//   → Nó executa (CPU/GPU/WASM) e retorna Result
//   → Collector agrega resultados parciais
//
// Fase 4 (atual):
//   - Registro e descoberta de nós edge
//   - Perfil de capacidade de cada nó
//   - Algoritmo de balanceamento de carga
//   - Submissão e rastreamento de tarefas
//   - Simulação de execução distribuída
// ============================================================

pub mod node;       // Registro e perfil de nós edge
pub mod task;       // Definição e ciclo de vida de tarefas
pub mod balancer;   // Algoritmo de balanceamento de carga
pub mod collector;  // Coleta e agregação de resultados
pub mod protocol;   // Protocolo de comunicação edge

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;

/// Estado global do subsistema edge
pub static EDGE_STATE: Spinlock<EdgeState> = Spinlock::new(EdgeState::new());

pub struct EdgeState {
    pub initialized: bool,
    pub active_nodes: usize,
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub bytes_offloaded: u64,
    pub total_compute_ms: u64,
}

impl EdgeState {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            active_nodes: 0,
            tasks_submitted: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            bytes_offloaded: 0,
            total_compute_ms: 0,
        }
    }
}

/// Inicializa o subsistema de edge computing
pub fn init() {
    node::init();
    task::init();
    balancer::init();
    collector::init();
    protocol::init();

    let mut state = EDGE_STATE.lock();
    state.initialized = true;
    state.active_nodes = node::count_active();

    crate::serial_println!("[EDGE] Subsistema Edge Computing inicializado");
    crate::serial_println!("[EDGE] {} nos disponiveis", state.active_nodes);
}

/// Submete uma tarefa para execução distribuída
pub fn submit_task(payload: Vec<u8>, kind: task::TaskKind) -> task::TaskId {
    let tid = task::submit(payload, kind);
    EDGE_STATE.lock().tasks_submitted += 1;
    tid
}

/// Tick periódico do edge — processa fila de tarefas
pub fn tick(current_tick: u64) {
    if current_tick % 500 != 0 { return; } // A cada 500ms
    task::process_queue(current_tick);
    balancer::rebalance();
}

/// Estatísticas do edge computing
pub fn get_stats() -> EdgeStats {
    let state = EDGE_STATE.lock();
    EdgeStats {
        active_nodes: node::count_active(),
        tasks_submitted: state.tasks_submitted,
        tasks_completed: state.tasks_completed,
        tasks_failed: state.tasks_failed,
        bytes_offloaded: state.bytes_offloaded,
        throughput_tasks_per_sec: if state.total_compute_ms > 0 {
            state.tasks_completed * 1000 / state.total_compute_ms.max(1)
        } else { 0 },
    }
}

#[derive(Debug, Clone)]
pub struct EdgeStats {
    pub active_nodes: usize,
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub bytes_offloaded: u64,
    pub throughput_tasks_per_sec: u64,
}
