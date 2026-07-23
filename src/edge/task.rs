extern crate alloc;
// ============================================================
// SOC-D — Tarefas Edge, Balanceador, Coletor e Protocolo
// ============================================================

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use spinning_top::Spinlock;

// ─── TASK ────────────────────────────────────────────────────────────────────

pub type TaskId = u64;
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Tipo de tarefa para edge computing
#[derive(Debug, Clone, PartialEq)]
pub enum TaskKind {
    /// Cálculo genérico (álgebra linear, simulação)
    Compute,
    /// Inferência de modelo ML/IA
    MLInference,
    /// Renderização 3D (rasterização, ray-tracing)
    Render3D,
    /// Processamento de dados (filtros, transformações)
    DataProcessing,
    /// Compressão/descompressão
    Compression,
    /// Operações criptográficas (hashing, cifra)
    Encryption,
    /// Offload quântico (Fase 4+)
    QuantumOffload,
}

impl TaskKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Compute        => "Compute",
            Self::MLInference    => "ML-Inference",
            Self::Render3D       => "Render3D",
            Self::DataProcessing => "DataProcessing",
            Self::Compression    => "Compression",
            Self::Encryption     => "Encryption",
            Self::QuantumOffload => "QuantumOffload",
        }
    }

    /// Estimativa de duração em ms para payload de 1KB
    pub fn base_duration_ms(&self) -> u64 {
        match self {
            Self::Compute        => 50,
            Self::MLInference    => 200,
            Self::Render3D       => 500,
            Self::DataProcessing => 30,
            Self::Compression    => 20,
            Self::Encryption     => 10,
            Self::QuantumOffload => 2000,
        }
    }
}

/// Estado de uma tarefa
#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    Queued,
    Dispatched { node_id: [u8; 32] },
    Running    { node_id: [u8; 32], started_at: u64 },
    Completed  { result_size: usize, duration_ms: u64 },
    Failed     { reason: String },
    TimedOut,
}

/// Uma tarefa edge completa
#[derive(Debug, Clone)]
pub struct EdgeTask {
    pub id:         TaskId,
    pub kind:       TaskKind,
    pub payload:    Vec<u8>,
    pub state:      TaskState,
    pub priority:   u8,     // 0=baixa, 255=crítica
    pub deadline_ms: u64,   // deadline absoluto em ticks
    pub created_at:  u64,
    pub result:      Option<Vec<u8>>,
}

impl EdgeTask {
    pub fn new(payload: Vec<u8>, kind: TaskKind, tick: u64) -> Self {
        let deadline = tick + kind.base_duration_ms() * 10; // 10× tolerância
        Self {
            id: NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed),
            kind,
            payload,
            state: TaskState::Queued,
            priority: 128,
            deadline_ms: deadline,
            created_at: tick,
            result: None,
        }
    }

    pub fn is_overdue(&self, current_tick: u64) -> bool {
        current_tick > self.deadline_ms
    }
}

/// Fila de tarefas
pub struct TaskQueue {
    tasks: BTreeMap<TaskId, EdgeTask>,
    completed: Vec<TaskId>,
}

impl TaskQueue {
    const fn new() -> Self {
        Self { tasks: BTreeMap::new(), completed: Vec::new() }
    }

    pub fn enqueue(&mut self, task: EdgeTask) -> TaskId {
        let id = task.id;
        self.tasks.insert(id, task);
        id
    }

    pub fn get_queued(&self) -> Vec<&EdgeTask> {
        self.tasks.values()
            .filter(|t| t.state == TaskState::Queued)
            .collect()
    }

    pub fn process(&mut self, current_tick: u64) {
        for task in self.tasks.values_mut() {
            match &task.state {
                TaskState::Queued => {
                    // Simula despacho imediato para o melhor nó
                    if let Some(node_id) = super::node::best_for(&task.kind) {
                        task.state = TaskState::Running {
                            node_id,
                            started_at: current_tick,
                        };
                    }
                }
                TaskState::Running { node_id, started_at } => {
                    let elapsed = current_tick.saturating_sub(*started_at);
                    let expected = task.kind.base_duration_ms();
                    if elapsed >= expected {
                        // Simula conclusão com resultado
                        let result_size = task.payload.len();
                        let duration = elapsed;
                        task.result = Some(alloc::vec![0u8; result_size / 2]); // "compressed"
                        task.state = TaskState::Completed { result_size, duration_ms: duration };
                        self.completed.push(task.id);

                        crate::serial_println!(
                            "[EDGE] Tarefa #{} ({}) concluida em {}ms",
                            task.id, task.kind.name(), duration
                        );

                        // Atualiza estado global
                        let mut es = super::EDGE_STATE.lock();
                        es.tasks_completed += 1;
                        es.total_compute_ms += duration;
                        es.bytes_offloaded += task.payload.len() as u64;
                    }
                }
                _ => {}
            }

            // Timeout
            if task.is_overdue(current_tick) {
                if matches!(task.state, TaskState::Queued | TaskState::Running {..}) {
                    task.state = TaskState::TimedOut;
                    super::EDGE_STATE.lock().tasks_failed += 1;
                }
            }
        }
    }

    pub fn stats(&self) -> TaskStats {
        TaskStats {
            queued:    self.tasks.values().filter(|t| t.state == TaskState::Queued).count(),
            running:   self.tasks.values().filter(|t| matches!(t.state, TaskState::Running{..})).count(),
            completed: self.completed.len(),
            failed:    self.tasks.values().filter(|t|
                matches!(t.state, TaskState::Failed{..} | TaskState::TimedOut)).count(),
            total: self.tasks.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskStats {
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub total: usize,
}

static TASK_QUEUE: Spinlock<TaskQueue> = Spinlock::new(TaskQueue::new());

pub fn init() {
    crate::serial_println!("[EDGE][TASK] Motor de tarefas edge ativo");
}

pub fn submit(payload: Vec<u8>, kind: TaskKind) -> TaskId {
    let tick = 0u64; // Fase 4: obter tick real
    let task = EdgeTask::new(payload, kind, tick);
    let id = TASK_QUEUE.lock().enqueue(task);
    crate::serial_println!("[EDGE] Tarefa #{} submetida", id);
    id
}

pub fn process_queue(tick: u64) {
    TASK_QUEUE.lock().process(tick);
}

pub fn get_stats() -> TaskStats {
    TASK_QUEUE.lock().stats()
}
