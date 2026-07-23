extern crate alloc;
// ============================================================
// SOC-D — Gossip Protocol
// ============================================================
// Propaga estado do nó pela rede P2P sem coordenação central.
// Cada nó envia seu estado para K peers aleatórios (fanout=3).
// Os peers repassam para outros, cobrindo a rede em O(log N).
// ============================================================

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;

/// Mensagem de estado propagada via Gossip
#[derive(Debug, Clone)]
pub struct GossipMessage {
    /// NodeId do originador
    pub origin_id: [u8; 32],
    /// Contador de versão (vector clock simplificado)
    pub version: u64,
    /// Tipo da mensagem
    pub kind: GossipKind,
    /// Número de hops já percorridos (TTL decresce)
    pub hops: u8,
    /// Tick de criação
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub enum GossipKind {
    /// Anúncio de presença (heartbeat)
    Heartbeat {
        name: String,
        load_cpu: u8,       // 0–100%
        load_mem: u8,       // 0–100%
        storage_free_mb: u32,
    },
    /// Novo arquivo disponível para sincronização
    FileAvailable {
        file_hash: [u8; 32],
        file_size: u64,
        path: String,
    },
    /// Solicitação de arquivo
    FileRequest {
        file_hash: [u8; 32],
        requester_id: [u8; 32],
    },
    /// Nó saindo da rede
    Departure,
}

/// Motor de Gossip
pub struct GossipEngine {
    pub running: bool,
    /// Mensagens recebidas recentemente (deduplicação)
    pub seen_messages: Vec<([u8; 32], u64)>, // (origin_id, version)
    /// Mensagens a propagar
    pub outbound: Vec<GossipMessage>,
    /// Estatísticas
    pub messages_sent: u64,
    pub messages_received: u64,
    pub messages_dropped: u64, // duplicatas
    pub last_heartbeat_tick: u64,
}

/// Intervalo de heartbeat: ~5 segundos (5000 ticks a 1kHz)
const HEARTBEAT_INTERVAL: u64 = 5_000;
/// TTL máximo de mensagens (hops)
const MAX_HOPS: u8 = 7;
/// Fanout: quantos peers recebem cada mensagem
const FANOUT: usize = 3;

impl GossipEngine {
    const fn new() -> Self {
        Self {
            running: false,
            seen_messages: Vec::new(),
            outbound: Vec::new(),
            messages_sent: 0,
            messages_received: 0,
            messages_dropped: 0,
            last_heartbeat_tick: 0,
        }
    }

    /// Processa um tick — envia heartbeat se necessário
    pub fn tick(&mut self, current_tick: u64) {
        if !self.running { return; }

        if current_tick.saturating_sub(self.last_heartbeat_tick) >= HEARTBEAT_INTERVAL {
            self.send_heartbeat(current_tick);
        }
    }

    fn send_heartbeat(&mut self, tick: u64) {
        let (used, free) = crate::memory::heap::heap_stats();
        let total = crate::memory::heap::HEAP_SIZE;
        let mem_pct = (used * 100 / total.max(1)) as u8;

        let msg = GossipMessage {
            origin_id: crate::p2p::node::get_node_id(),
            version: tick,
            kind: GossipKind::Heartbeat {
                name: "socd-node".into(),
                load_cpu: 5, // Fase 3: leitura real do scheduler
                load_mem: mem_pct,
                storage_free_mb: (free / 1024) as u32,
            },
            hops: 0,
            created_at: tick,
        };

        self.outbound.push(msg);
        self.messages_sent += 1;
        self.last_heartbeat_tick = tick;
    }

    /// Recebe e processa uma mensagem Gossip
    pub fn receive(&mut self, msg: GossipMessage) {
        // Deduplicação: ignora mensagens já vistas
        let seen = self.seen_messages.iter()
            .any(|(id, ver)| *id == msg.origin_id && *ver >= msg.version);

        if seen {
            self.messages_dropped += 1;
            return;
        }

        self.seen_messages.push((msg.origin_id, msg.version));
        // Mantém cache pequeno
        if self.seen_messages.len() > 200 {
            self.seen_messages.drain(0..50);
        }

        self.messages_received += 1;

        // Re-propaga se ainda tem TTL
        if msg.hops < MAX_HOPS {
            let mut forwarded = msg.clone();
            forwarded.hops += 1;
            self.outbound.push(forwarded);
        }
    }
}

static GOSSIP: Spinlock<GossipEngine> = Spinlock::new(GossipEngine::new());

pub fn init() {
    GOSSIP.lock().running = true;
    crate::serial_println!("[P2P][GOSSIP] Protocolo Gossip ativo (fanout={}, TTL={})",
        FANOUT, MAX_HOPS);
}

pub fn tick(current_tick: u64) {
    GOSSIP.lock().tick(current_tick);
}
