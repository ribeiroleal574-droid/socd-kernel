extern crate alloc;
// ============================================================
// SOC-D Kernel — Identidade do Nó P2P
// ============================================================
//
// Cada nó SOC-D possui uma identidade criptográfica única
// baseada em um par de chaves Ed25519:
//   - Chave privada: mantida em segredo no dispositivo
//   - Chave pública: usada como NodeId na rede
//
// O NodeId é derivado da chave pública via SHA-256,
// resultando em 32 bytes que identificam o nó globalmente.
//
// Informações do nó:
//   - NodeId (32 bytes)
//   - Nome amigável (configurável pelo usuário)
//   - Endereços de rede (IPv4/IPv6/mDNS)
//   - Capacidades (o que este nó oferece à rede)
//   - Versão do protocolo SOC-D
// ============================================================

use alloc::{string::{String, ToString}, vec::Vec};
use spinning_top::Spinlock;

/// Versão do protocolo P2P do SOC-D
pub const PROTOCOL_VERSION: u32 = 1;

/// Capacidades que um nó pode oferecer
#[derive(Debug, Clone, PartialEq)]
pub enum NodeCapability {
    /// Armazena e replica dados de outros nós
    Storage,
    /// Processa tarefas de IA distribuídas
    Compute,
    /// Retransmite mensagens para nós atrás de NAT
    Relay,
    /// Indexa conteúdo da rede
    Index,
    /// Fornece acesso à internet para a rede
    Gateway,
}

/// Endereço de rede de um nó
#[derive(Debug, Clone)]
pub struct NodeAddress {
    pub kind: AddressKind,
    pub addr: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AddressKind {
    IPv4,
    IPv6,
    #[allow(non_camel_case_types)]
    mDNS,  // local.socd-node-xxx.local
    Relay, // via nó relay
}

/// Identidade completa do nó local
#[derive(Debug, Clone)]
pub struct LocalNode {
    /// ID único: hash da chave pública (32 bytes)
    pub node_id: [u8; 32],
    /// Chave pública Ed25519 (32 bytes)
    pub public_key: [u8; 32],
    /// Chave privada Ed25519 (64 bytes) — NUNCA exposta pela rede
    pub private_key: [u8; 64],
    /// Nome amigável configurado pelo usuário
    pub name: String,
    /// Versão do protocolo
    pub protocol_version: u32,
    /// Endereços de rede conhecidos
    pub addresses: Vec<NodeAddress>,
    /// Capacidades oferecidas
    pub capabilities: Vec<NodeCapability>,
    /// Tick de inicialização
    pub started_at: u64,
    /// Uptime em ticks
    pub uptime_ticks: u64,
}

impl LocalNode {
    /// Cria um novo nó com identidade gerada deterministicamente
    /// Gera par de chaves Ed25519 real com entropia de hardware
    fn generate(_seed: u64) -> Self {
        // Ed25519 real via crate::crypto
        let kp = crate::crypto::KeyPair::generate();
        let public_key = kp.verifying_key;
        let mut private_key = [0u8; 64];
        private_key[..32].copy_from_slice(&kp.signing_key);
        private_key[32..].copy_from_slice(&kp.verifying_key);

        // NodeId = SHA-256 real da chave pública
        let node_id = crate::crypto::pubkey_to_node_id(&public_key);

        Self {
            node_id,
            public_key,
            private_key,
            name: "socd-node".to_string(),
            protocol_version: PROTOCOL_VERSION,
            addresses: alloc::vec![
                NodeAddress {
                    kind: AddressKind::mDNS,
                    addr: alloc::format!("socd-{:02x}{:02x}.local",
                        node_id[0], node_id[1]),
                    port: 7700,
                }
            ],
            capabilities: alloc::vec![
                NodeCapability::Storage,
                NodeCapability::Compute,
            ],
            started_at: 0,
            uptime_ticks: 0,
        }
    }

    /// Serializa as informações públicas do nó para anúncio na rede
    pub fn to_announcement(&self) -> NodeAnnouncement {
        NodeAnnouncement {
            node_id: self.node_id,
            public_key: self.public_key,
            name: self.name.clone(),
            protocol_version: self.protocol_version,
            addresses: self.addresses.clone(),
            capabilities: self.capabilities.clone(),
        }
    }
}

/// Informações públicas anunciadas para outros nós
#[derive(Debug, Clone)]
pub struct NodeAnnouncement {
    pub node_id: [u8; 32],
    pub public_key: [u8; 32],
    pub name: String,
    pub protocol_version: u32,
    pub addresses: Vec<NodeAddress>,
    pub capabilities: Vec<NodeCapability>,
}

/// Hash simples (DJB2 estendido para 32 bytes)
/// Fase 3: substituir por SHA-256 real
fn simple_hash(input: &[u8]) -> [u8; 32] {
    let mut result = [0u8; 32];
    let mut hash: u64 = 5381;
    for (i, &byte) in input.iter().enumerate() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        result[i % 32] ^= (hash & 0xFF) as u8;
    }
    // Segunda passagem para melhor distribuição
    hash = 0x9e3779b97f4a7c15;
    for (i, &byte) in input.iter().rev().enumerate() {
        hash = hash.wrapping_mul(6364136223846793005)
                   .wrapping_add(byte as u64 ^ 1442695040888963407);
        result[(i + 16) % 32] ^= (hash >> 32) as u8;
    }
    result
}

// ─── Estado Global ────────────────────────────────────────────────────────────

pub static LOCAL_NODE: Spinlock<Option<LocalNode>> = Spinlock::new(None);

pub fn init() {
    // Seed baseado em um valor pseudo-único do sistema
    // Fase 3: usar RDRAND ou TPM para entropia real
    
    let node = LocalNode::generate(0x50CD_FA5E_0002_0000u64);
    let id = node.node_id;
    *LOCAL_NODE.lock() = Some(node);
    crate::serial_println!("[P2P][NODE] Identidade criada: {:02x}{:02x}{:02x}{:02x}...",
        id[0], id[1], id[2], id[3]);
}

pub fn get_node_id() -> [u8; 32] {
    LOCAL_NODE.lock().as_ref().map(|n| n.node_id).unwrap_or([0u8; 32])
}

pub fn get_announcement() -> Option<NodeAnnouncement> {
    LOCAL_NODE.lock().as_ref().map(|n| n.to_announcement())
}

pub fn set_name(name: &str) {
    if let Some(node) = LOCAL_NODE.lock().as_mut() {
        node.name = name.to_string();
    }
}

pub fn get_info() -> Option<(String, [u8; 32], u32)> {
    LOCAL_NODE.lock().as_ref().map(|n| {
        (n.name.clone(), n.node_id, n.protocol_version)
    })
}


/// Retorna a chave pública do nó local (Fase 6.2)
pub fn get_public_key() -> [u8; 32] {
    LOCAL_NODE.lock().as_ref().map(|n| n.public_key).unwrap_or([0u8; 32])
}
