extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spinning_top::Spinlock;

// ─── LOAD BALANCER ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BalanceStrategy { LeastLoaded, BestFit, RoundRobin, LowestLatency, LocalFirst }

pub struct LoadBalancer {
    pub strategy: BalanceStrategy,
    pub decisions: u64,
}

impl LoadBalancer {
    const fn new() -> Self {
        Self { strategy: BalanceStrategy::BestFit, decisions: 0 }
    }
    pub fn rebalance(&mut self) { self.decisions += 1; }
}

static BALANCER: Spinlock<LoadBalancer> = Spinlock::new(LoadBalancer::new());

pub fn balancer_init() {
    crate::serial_println!("[EDGE][BAL] Balanceador ativo (BestFit)");
}
pub fn rebalance() { BALANCER.lock().rebalance(); }
pub fn set_strategy(s: BalanceStrategy) { BALANCER.lock().strategy = s; }

// ─── RESULT COLLECTOR ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: super::task::TaskId,
    pub node_id: [u8; 32],
    pub data: Vec<u8>,
    pub duration_ms: u64,
    pub checksum: u32,
}

impl TaskResult {
    pub fn compute_checksum(data: &[u8]) -> u32 {
        data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
    }
}

use alloc::collections::BTreeMap;

pub struct ResultCollector {
    results: BTreeMap<super::task::TaskId, TaskResult>,
    pub total_bytes_received: u64,
}

impl ResultCollector {
    const fn new() -> Self {
        Self { results: BTreeMap::new(), total_bytes_received: 0 }
    }
    pub fn collect(&mut self, result: TaskResult) {
        self.total_bytes_received += result.data.len() as u64;
        self.results.insert(result.task_id, result);
    }
    pub fn get(&self, task_id: super::task::TaskId) -> Option<&TaskResult> {
        self.results.get(&task_id)
    }
}

lazy_static::lazy_static! {
    static ref COLLECTOR: Spinlock<ResultCollector> = Spinlock::new(ResultCollector::new());
}

pub fn collector_init() {
    crate::serial_println!("[EDGE][COL] Coletor de resultados ativo");
}
pub fn collect(result: TaskResult) { COLLECTOR.lock().collect(result); }
pub fn get_result(task_id: super::task::TaskId) -> Option<TaskResult> {
    COLLECTOR.lock().get(task_id).cloned()
}

// ─── PROTOCOL ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum EdgeMessage {
    Announce { profile_hash: u32 },
    TaskSubmit { task_id: super::task::TaskId, kind_id: u8, payload_size: u32 },
    TaskAccept { task_id: super::task::TaskId },
    TaskReject { task_id: super::task::TaskId, reason: String },
    TaskResult { task_id: super::task::TaskId, result_size: u32, checksum: u32 },
    Heartbeat  { load_pct: u8, tasks_active: u32 },
}

pub fn serialize(msg: &EdgeMessage) -> Vec<u8> {
    let mut buf = Vec::new();
    match msg {
        EdgeMessage::Announce { profile_hash } => {
            buf.push(0x01);
            buf.extend_from_slice(&profile_hash.to_le_bytes());
        }
        EdgeMessage::Heartbeat { load_pct, tasks_active } => {
            buf.push(0x07);
            buf.push(*load_pct);
            buf.extend_from_slice(&tasks_active.to_le_bytes());
        }
        _ => { buf.push(0xFF); }
    }
    buf
}

pub fn protocol_init() {
    crate::serial_println!("[EDGE][PROTO] Protocolo Edge v1 inicializado");
}

/// Ponto de entrada público chamado por edge::init()
pub fn init() {
    balancer_init();
}
