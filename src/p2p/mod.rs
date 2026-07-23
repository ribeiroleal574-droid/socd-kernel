extern crate alloc;
use alloc::string::{String, ToString};
// ============================================================
// SOC-D Kernel — Módulo P2P (Nuvem Pessoal Descentralizada)
// ============================================================
//
// Este módulo implementa a rede P2P do SOC-D.
// Cada dispositivo do usuário é um "nó" que:
//   - Descobre outros nós na rede local (mDNS)
//   - Mantém conexões persistentes (Gossip Protocol)
//   - Sincroniza arquivos criptografados entre nós
//   - Forma um cluster pessoal sem servidores centrais
//
// Arquitetura:
//   ┌─────────────────────────────────────────────┐
//   │              Aplicação / IA Engine          │
//   ├─────────────────────────────────────────────┤
//   │           Sync Layer (sync/)                │
//   ├─────────────────────────────────────────────┤
//   │     P2P Core  │  Crypto  │  Discovery       │
//   ├─────────────────────────────────────────────┤
//   │          Transport (TCP/UDP)                │
//   └─────────────────────────────────────────────┘
//
// Fase 2 (atual): Simulação do protocolo em memória
//   - Estruturas de dados e lógica completas
//   - Transport layer simulado (sem sockets reais no kernel bare metal)
//   - Base para integração com libp2p quando tiver userspace
//
// Fase 3: Integração real com libp2p via userspace driver
// ============================================================

pub mod node;        // Identidade e estado do nó local
pub mod peer;        // Gerenciamento de peers conhecidos
pub mod discovery;   // Descoberta de nós (mDNS simulado)
pub mod gossip;      // Protocolo Gossip para propagação de estado
pub mod crypto;      // Criptografia E2E (AES-256-GCM + X25519)
pub mod routing;     // Roteamento de mensagens entre nós
pub mod transport;   // Camada de transporte (simulada na Fase 2)
pub mod dag;         // DAG + Sync Engine (Fase 3)
pub mod dag_sig;     // Assinaturas criptográficas DAG (Fase 6.2)

use spinning_top::Spinlock;

/// Estado global da rede P2P
pub static P2P_STATE: Spinlock<P2PNetworkState> =
    Spinlock::new(P2PNetworkState::new());

/// Estado completo da rede P2P do nó local
pub struct P2PNetworkState {
    pub initialized: bool,
    pub online: bool,
    pub node_id: [u8; 32],        // Chave pública Ed25519 como ID
    pub peers_count: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub messages_routed: u64,
}

impl P2PNetworkState {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            online: false,
            node_id: [0u8; 32],
            peers_count: 0,
            bytes_sent: 0,
            bytes_received: 0,
            messages_routed: 0,
        }
    }
}

/// Inicializa o subsistema P2P completo
pub fn init() {
    node::init();
    peer::init();
    discovery::init();
    gossip::init();
    crypto::init();
    routing::init();

    let mut state = P2P_STATE.lock();
    state.initialized = true;
    state.online = true;

    // Copia o node_id do módulo node
    let nid = node::get_node_id();
    state.node_id = nid;

    crate::serial_println!("[P2P] Subsistema P2P inicializado");
    crate::serial_println!("[P2P] Node ID: {:02x}{:02x}{:02x}{:02x}...",
        nid[0], nid[1], nid[2], nid[3]);
}

/// Estatísticas da rede P2P
pub fn get_stats() -> P2PStats {
    let state = P2P_STATE.lock();
    let peers = peer::count_peers();
    P2PStats {
        online: state.online,
        node_id_short: {
            let id = state.node_id;
            alloc::format!("{:02x}{:02x}{:02x}{:02x}", id[0], id[1], id[2], id[3])
        },
        peers_known: peers.0,
        peers_active: peers.1,
        bytes_sent: state.bytes_sent,
        bytes_received: state.bytes_received,
        messages_routed: state.messages_routed,
    }
}

#[derive(Debug, Clone)]
pub struct P2PStats {
    pub online: bool,
    pub node_id_short: alloc::string::String,
    pub peers_known: usize,
    pub peers_active: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub messages_routed: u64,
}
