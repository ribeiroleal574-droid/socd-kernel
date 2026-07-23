extern crate alloc;
// ============================================================
// SOC-D — Registro de Nós Edge
// ============================================================
//
// Cada nó na rede edge publica seu perfil de capacidade.
// O dispatcher usa este perfil para tomar decisões ótimas.
//
// Perfil de um nó:
//   - Capacidade de CPU (MIPS estimado)
//   - Memória disponível (MB)
//   - GPU disponível (GFLOPS)
//   - Latência de rede (ms)
//   - Bateria (0–100%, -1 se alimentado)
//   - Tipos de tarefa suportados
//   - Carga atual (0–100%)
//
// Score de um nó para uma tarefa específica:
//   score = (cpu_cap × 0.4 + mem_cap × 0.2 + gpu_cap × 0.2)
//           × (1 - load/100) × (1 - latency/1000)
//           × battery_factor
// ============================================================

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;

use super::task::TaskKind;

/// Identificador de nó edge
pub type EdgeNodeId = [u8; 32];

/// Perfil de capacidade de um nó
#[derive(Debug, Clone)]
pub struct NodeCapabilityProfile {
    /// MIPS estimado (milhões de instruções por segundo)
    pub cpu_mips: u32,
    /// Núcleos de CPU disponíveis
    pub cpu_cores: u8,
    /// Arquitetura: x86_64, aarch64, riscv64, wasm
    pub arch: NodeArch,
    /// RAM disponível em MB
    pub ram_available_mb: u32,
    /// GFLOPS da GPU (0 se sem GPU)
    pub gpu_gflops: f32,
    /// Latência de rede para este nó (ms)
    pub network_latency_ms: u32,
    /// Nível de bateria (-1 = alimentado, 0–100 = bateria)
    pub battery_pct: i8,
    /// Carga atual de CPU (0–100%)
    pub current_load_pct: u8,
    /// Temperatura da CPU em °C
    pub temp_celsius: u8,
    /// Tipos de tarefa que este nó suporta
    pub supported_tasks: Vec<TaskKind>,
    /// Velocidade de rede em Mbps
    pub network_mbps: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeArch {
    X86_64,
    AArch64,
    RiscV64,
    WASM,   // Nó rodando em browser/WASM sandbox
}

impl NodeCapabilityProfile {
    /// Calcula o score para executar um tipo específico de tarefa
    pub fn score_for(&self, kind: &TaskKind) -> f32 {
        if !self.supported_tasks.contains(kind) {
            return 0.0;
        }

        let cpu_score = (self.cpu_mips as f32 / 10000.0).min(1.0);
        let mem_score = (self.ram_available_mb as f32 / 4096.0).min(1.0);
        let gpu_score = match kind {
            TaskKind::MLInference | TaskKind::Render3D => {
                (self.gpu_gflops / 10.0).min(1.0)
            }
            _ => 0.2, // GPU não importante para outras tarefas
        };

        let load_factor  = 1.0 - (self.current_load_pct as f32 / 100.0);
        let lat_factor   = 1.0 - (self.network_latency_ms as f32 / 1000.0).min(0.9);
        let temp_factor  = if self.temp_celsius > 80 { 0.5 } else { 1.0 };
        let batt_factor  = if self.battery_pct >= 0 && self.battery_pct < 20 {
            0.3
        } else { 1.0 };

        (cpu_score * 0.35 + mem_score * 0.20 + gpu_score * 0.25)
            * load_factor * lat_factor * temp_factor * batt_factor
    }
}

/// Estado de um nó edge
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeNodeState {
    /// Descoberto via mDNS, ainda não conectado
    Discovered,
    /// Conectado e respondendo ao heartbeat
    Online,
    /// Executando tarefas
    Busy,
    /// Temporariamente indisponível
    Unavailable,
    /// Saiu da rede
    Offline,
}

/// Um nó na rede edge
#[derive(Debug, Clone)]
pub struct EdgeNode {
    pub id: EdgeNodeId,
    pub name: String,
    pub state: EdgeNodeState,
    pub profile: NodeCapabilityProfile,
    pub tasks_active: u32,
    pub tasks_completed: u64,
    pub last_heartbeat_tick: u64,
    pub is_self: bool, // Este nó é o dispositivo local?
}

impl EdgeNode {
    /// Verifica se o nó está disponível para novas tarefas
    pub fn is_available(&self) -> bool {
        matches!(self.state, EdgeNodeState::Online | EdgeNodeState::Busy)
            && self.tasks_active < self.profile.cpu_cores as u32 * 2
    }

    /// Score ponderado para uma tarefa
    pub fn score_for_task(&self, kind: &TaskKind) -> f32 {
        if !self.is_available() { return 0.0; }
        // Penaliza nós já ocupados
        let busy_penalty = 1.0 - (self.tasks_active as f32 / 8.0).min(0.8);
        self.profile.score_for(kind) * busy_penalty
    }
}

/// Registro global de nós edge
pub struct NodeRegistry {
    nodes: BTreeMap<EdgeNodeId, EdgeNode>,
    local_node_id: EdgeNodeId,
}

impl NodeRegistry {
    fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            local_node_id: [0u8; 32],
        }
    }

    /// Registra o nó local
    fn register_local(&mut self) {
        let node_id = crate::p2p::node::get_node_id();
        self.local_node_id = node_id;

        let profile = NodeCapabilityProfile {
            cpu_mips: 8000,
            cpu_cores: 4,
            arch: NodeArch::X86_64,
            ram_available_mb: {
                let (_, free) = crate::memory::heap::heap_stats();
                (free / 1024 / 1024).max(1) as u32
            },
            gpu_gflops: 0.0, // Sem GPU no bare metal por ora
            network_latency_ms: 0, // Local
            battery_pct: -1, // Alimentado
            current_load_pct: 10,
            temp_celsius: 45,
            supported_tasks: alloc::vec![
                TaskKind::Compute,
                TaskKind::MLInference,
                TaskKind::DataProcessing,
                TaskKind::Compression,
                TaskKind::Encryption,
            ],
            network_mbps: 1000,
        };

        self.nodes.insert(node_id, EdgeNode {
            id: node_id,
            name: "socd-local".into(),
            state: EdgeNodeState::Online,
            profile,
            tasks_active: 0,
            tasks_completed: 0,
            last_heartbeat_tick: 0,
            is_self: true,
        });
    }

    /// Registra peers P2P como nós edge
    fn register_p2p_peers(&mut self) {
        let peers = crate::p2p::peer::get_active_peers();
        for peer in peers {
            if self.nodes.contains_key(&peer.node_id) { continue; }

            // Infere capacidade com base no score de confiança
            let inferred_mips = 4000 + peer.trust_score as u32 * 60;

            self.nodes.insert(peer.node_id, EdgeNode {
                id: peer.node_id,
                name: peer.name,
                state: if peer.is_own_device {
                    EdgeNodeState::Online
                } else {
                    EdgeNodeState::Discovered
                },
                profile: NodeCapabilityProfile {
                    cpu_mips: inferred_mips,
                    cpu_cores: 4,
                    arch: NodeArch::AArch64,
                    ram_available_mb: 2048,
                    gpu_gflops: 2.5,
                    network_latency_ms: peer.latency_us / 1000,
                    battery_pct: 80,
                    current_load_pct: 20,
                    temp_celsius: 40,
                    supported_tasks: alloc::vec![
                        TaskKind::Compute,
                        TaskKind::MLInference,
                        TaskKind::DataProcessing,
                        TaskKind::Render3D,
                    ],
                    network_mbps: 100,
                },
                tasks_active: 0,
                tasks_completed: 0,
                last_heartbeat_tick: 0,
                is_self: false,
            });
        }
    }

    /// Melhor nó para um tipo de tarefa
    pub fn best_node_for(&self, kind: &TaskKind) -> Option<EdgeNodeId> {
        self.nodes.values()
            .filter(|n| n.is_available())
            .max_by(|a, b| {
                a.score_for_task(kind)
                    .partial_cmp(&b.score_for_task(kind))
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .map(|n| n.id)
    }

    /// Lista todos os nós disponíveis com seus scores
    pub fn ranked_nodes_for(&self, kind: &TaskKind) -> Vec<(EdgeNodeId, f32)> {
        let mut ranked: Vec<(EdgeNodeId, f32)> = self.nodes.values()
            .filter(|n| n.is_available())
            .map(|n| (n.id, n.score_for_task(kind)))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        ranked
    }

    pub fn count_active(&self) -> usize {
        self.nodes.values()
            .filter(|n| n.state == EdgeNodeState::Online || n.state == EdgeNodeState::Busy)
            .count()
    }

    pub fn all_nodes(&self) -> Vec<&EdgeNode> {
        self.nodes.values().collect()
    }
}

lazy_static::lazy_static! {
    static ref REGISTRY: Spinlock<NodeRegistry> = Spinlock::new(NodeRegistry::new());
}

pub fn init() {
    let mut reg = REGISTRY.lock();
    reg.register_local();
    reg.register_p2p_peers();
    crate::serial_println!("[EDGE][NODE] {} nos registrados", reg.nodes.len());
}

pub fn count_active() -> usize {
    REGISTRY.lock().count_active()
}

pub fn best_for(kind: &TaskKind) -> Option<EdgeNodeId> {
    REGISTRY.lock().best_node_for(kind)
}

pub fn ranked_for(kind: &TaskKind) -> Vec<(EdgeNodeId, f32)> {
    REGISTRY.lock().ranked_nodes_for(kind)
}

pub fn get_all() -> Vec<EdgeNode> {
    REGISTRY.lock().all_nodes().into_iter().cloned().collect()
}
