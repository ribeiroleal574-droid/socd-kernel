// SOC-D Edge — Collector (re-exports from balancer)
pub use super::balancer::{ResultCollector, TaskResult, collect, get_result};
pub fn init() { super::balancer::collector_init(); }
