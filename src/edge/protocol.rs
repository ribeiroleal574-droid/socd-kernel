// SOC-D Edge — Protocol (re-exports from balancer)
pub use super::balancer::{EdgeMessage, serialize};
pub fn init() { super::balancer::protocol_init(); }
