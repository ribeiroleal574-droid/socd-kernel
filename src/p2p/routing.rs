extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D — Roteamento P2P
// ============================================================
// Tabela de roteamento Kademlia-like: cada nó conhece peers
// "próximos" no espaço XOR do NodeId. Mensagens são roteadas
// iterativamente até o destino.
// ============================================================

use spinning_top::Spinlock;

/// Entrada na tabela de roteamento
#[derive(Debug, Clone)]
pub struct RouteEntry {
    pub node_id: [u8; 32],
    pub next_hop: [u8; 32], // Peer direto para alcançar node_id
    pub hops: u8,
    pub latency_us: u32,
}

pub struct RoutingTable {
    entries: Vec<RouteEntry>,
}

impl RoutingTable {
    const fn new() -> Self { Self { entries: Vec::new() } }

    /// Distância XOR entre dois NodeIds (métrica Kademlia)
    pub fn xor_distance(a: &[u8; 32], b: &[u8; 32]) -> u32 {
        let mut dist = 0u32;
        for i in 0..4 {
            dist ^= (a[i] as u32) << (24 - i * 8);
            dist ^= (b[i] as u32) << (24 - i * 8);
        }
        dist
    }

    /// Adiciona ou atualiza rota
    pub fn upsert(&mut self, entry: RouteEntry) {
        if let Some(existing) = self.entries.iter_mut()
            .find(|e| e.node_id == entry.node_id)
        {
            // Atualiza se nova rota for melhor (menos hops ou latência)
            if entry.hops < existing.hops
                || (entry.hops == existing.hops && entry.latency_us < existing.latency_us)
            {
                *existing = entry;
            }
        } else {
            self.entries.push(entry);
        }
    }

    /// Encontra melhor próximo hop para um destino
    pub fn next_hop(&self, destination: &[u8; 32]) -> Option<[u8; 32]> {
        self.entries.iter()
            .filter(|e| e.node_id == *destination)
            .min_by_key(|e| e.hops)
            .map(|e| e.next_hop)
    }

    pub fn entry_count(&self) -> usize { self.entries.len() }
}

static ROUTING: Spinlock<RoutingTable> = Spinlock::new(RoutingTable::new());

pub fn init() {
    // Adiciona rotas diretas para peers conhecidos (1 hop)
    let peers = crate::p2p::peer::get_active_peers();
    let mut table = ROUTING.lock();
    for peer in peers {
        table.upsert(RouteEntry {
            node_id: peer.node_id,
            next_hop: peer.node_id,
            hops: 1,
            latency_us: peer.latency_us,
        });
    }
    crate::serial_println!("[P2P][ROUTE] {} rotas iniciais", table.entry_count());
}

pub fn find_route(destination: &[u8; 32]) -> Option<[u8; 32]> {
    ROUTING.lock().next_hop(destination)
}
