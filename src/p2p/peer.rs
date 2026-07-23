extern crate alloc;
// ============================================================
// SOC-D Kernel — Gerenciamento de Peers
// ============================================================
//
// O módulo de peers mantém o estado de todos os nós
// conhecidos na rede P2P do usuário.
//
// Estados de um peer:
//   Unknown    → descoberto mas não conectado
//   Discovered → respondeu ao mDNS/Gossip
//   Connected  → conexão ativa, troca de heartbeat
//   Trusted    → verificado criptograficamente
//   Banned     → bloqueado por comportamento suspeito
//
// Peer scoring:
//   Cada peer tem um score de confiança (0-100).
//   Score cai com: timeouts, dados inválidos, violações
//   Score sobe com: respostas rápidas, dados corretos
//   A IA usa o score para decidir quais peers usar
// ============================================================

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;

/// Estado da conexão com um peer
#[derive(Debug, Clone, PartialEq)]
pub enum PeerState {
    /// Descoberto via mDNS ou Gossip, sem conexão ainda
    Discovered,
    /// Tentativa de conexão em andamento
    Connecting,
    /// Conexão ativa e autenticada
    Connected,
    /// Verificado criptograficamente (chave pública confirmada)
    Trusted,
    /// Desconectado (última tentativa falhou)
    Disconnected { reason: DisconnectReason },
    /// Banido por comportamento suspeito
    Banned { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisconnectReason {
    Timeout,
    InvalidData,
    ProtocolError,
    UserInitiated,
    NetworkError,
}

/// Informações sobre um peer conhecido
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// ID único do peer (hash da chave pública)
    pub node_id: [u8; 32],
    /// Chave pública (para verificação criptográfica)
    pub public_key: [u8; 32],
    /// Nome amigável
    pub name: String,
    /// Endereço principal de contato
    pub address: String,
    /// Porta
    pub port: u16,
    /// Estado atual da conexão
    pub state: PeerState,
    /// Score de confiança (0–100)
    pub trust_score: u8,
    /// Latência média em microssegundos
    pub latency_us: u32,
    /// Tick do último contato
    pub last_seen_tick: u64,
    /// Tick da primeira descoberta
    pub first_seen_tick: u64,
    /// Total de bytes sincronizados com este peer
    pub bytes_synced: u64,
    /// Versão do protocolo SOC-D do peer
    pub protocol_version: u32,
    /// Este peer pertence ao mesmo usuário?
    pub is_own_device: bool,
}

impl PeerInfo {
    /// ID curto para exibição (8 hex chars)
    pub fn short_id(&self) -> alloc::string::String {
        alloc::format!("{:02x}{:02x}{:02x}{:02x}",
            self.node_id[0], self.node_id[1],
            self.node_id[2], self.node_id[3])
    }

    /// Atualiza score com base em resultado de operação
    pub fn update_score(&mut self, success: bool, latency_us: u32) {
        if success {
            // Sucesso: score sobe gradualmente, latência influencia
            let latency_bonus = if latency_us < 1000 { 2u8 }
                                else if latency_us < 10_000 { 1 }
                                else { 0 };
            self.trust_score = self.trust_score.saturating_add(latency_bonus);
            self.latency_us = (self.latency_us / 2) + (latency_us / 2); // EMA
        } else {
            // Falha: score cai mais rapidamente
            self.trust_score = self.trust_score.saturating_sub(5);
            if self.trust_score == 0 {
                self.state = PeerState::Disconnected {
                    reason: DisconnectReason::Timeout,
                };
            }
        }
        self.trust_score = self.trust_score.min(100);
    }
}

/// Tabela de peers conhecidos
pub struct PeerTable {
    /// Peers indexados por NodeId
    peers: BTreeMap<[u8; 32], PeerInfo>,
    /// Tick atual do sistema
    current_tick: u64,
}

impl PeerTable {
    const fn new() -> Self {
        Self {
            peers: BTreeMap::new(),
            current_tick: 0,
        }
    }

    /// Adiciona ou atualiza um peer na tabela
    pub fn upsert(&mut self, info: PeerInfo) {
        self.peers.insert(info.node_id, info);
    }

    /// Adiciona um peer descoberto via mDNS
    pub fn add_discovered(
        &mut self,
        node_id: [u8; 32],
        public_key: [u8; 32],
        name: &str,
        address: &str,
        port: u16,
        is_own: bool,
    ) {
        if self.peers.contains_key(&node_id) {
            // Atualiza last_seen se já existe
            if let Some(peer) = self.peers.get_mut(&node_id) {
                peer.last_seen_tick = self.current_tick;
                peer.state = PeerState::Discovered;
            }
            return;
        }

        self.peers.insert(node_id, PeerInfo {
            node_id,
            public_key,
            name: name.to_string(),
            address: address.to_string(),
            port,
            state: PeerState::Discovered,
            trust_score: if is_own { 80 } else { 30 }, // Próprios dispositivos têm score maior
            latency_us: 0,
            last_seen_tick: self.current_tick,
            first_seen_tick: self.current_tick,
            bytes_synced: 0,
            protocol_version: super::node::PROTOCOL_VERSION,
            is_own_device: is_own,
        });

        crate::serial_println!("[P2P][PEER] Novo peer: {} @ {}:{} (proprio={})",
            name, address, port, is_own);
    }

    /// Marca um peer como conectado
    pub fn mark_connected(&mut self, node_id: &[u8; 32]) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.state = PeerState::Connected;
            peer.last_seen_tick = self.current_tick;
        }
    }

    /// Marca um peer como confiável (chave verificada)
    pub fn mark_trusted(&mut self, node_id: &[u8; 32]) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.state = PeerState::Trusted;
            peer.trust_score = peer.trust_score.max(70);
        }
    }

    /// Lista peers ativos (Connected ou Trusted)
    pub fn active_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values()
            .filter(|p| matches!(p.state, PeerState::Connected | PeerState::Trusted))
            .collect()
    }

    /// Lista todos os peers conhecidos
    pub fn all_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Melhor peer para sincronização (maior score + menor latência)
    pub fn best_sync_peer(&self) -> Option<&PeerInfo> {
        self.peers.values()
            .filter(|p| matches!(p.state, PeerState::Connected | PeerState::Trusted))
            .filter(|p| p.is_own_device) // Prefere próprios dispositivos
            .max_by_key(|p| p.trust_score)
    }

    /// Contagem de peers por estado
    pub fn counts(&self) -> (usize, usize) {
        let known = self.peers.len();
        let active = self.active_peers().len();
        (known, active)
    }
}

static PEER_TABLE: Spinlock<PeerTable> = Spinlock::new(PeerTable::new());

pub fn init() {
    // Adiciona peers simulados (próprios dispositivos do usuário)
    // Fase 3: descobertos via mDNS real
    let mut table = PEER_TABLE.lock();

    let simulated_peers = [
        ([0x01u8; 32], "socd-phone",  "192.168.1.101", 7700u16, true),
        ([0x02u8; 32], "socd-tablet", "192.168.1.102", 7700,    true),
        ([0x03u8; 32], "socd-server", "192.168.1.103", 7700,    true),
    ];

    for (mut id, name, addr, port, is_own) in simulated_peers {
        // Varia o ID para cada peer simulado
        id[1] = name.len() as u8;
        let pk = id; // simplificação — Fase 3 usa chaves reais
        table.add_discovered(id, pk, name, addr, port, is_own);
        if is_own {
            table.mark_connected(&id);
            table.mark_trusted(&id);
        }
    }

    let (known, active) = table.counts();
    crate::serial_println!("[P2P][PEER] Tabela iniciada: {} conhecidos, {} ativos",
        known, active);
}

pub fn count_peers() -> (usize, usize) {
    PEER_TABLE.lock().counts()
}

pub fn get_all_peers() -> Vec<PeerInfo> {
    PEER_TABLE.lock().all_peers().into_iter().cloned().collect()
}

pub fn get_active_peers() -> Vec<PeerInfo> {
    PEER_TABLE.lock().active_peers().into_iter().cloned().collect()
}

/// Retorna os node_ids de todos os peers conhecidos (Fase 6.2)
pub fn get_known_peers() -> Vec<[u8; 32]> {
    get_all_peers().into_iter().map(|p| p.node_id).collect()
}
