extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D Kernel — Criptografia E2E
// ============================================================
//
// Toda comunicação entre nós SOC-D é criptografada E2E.
//
// Protocolo de estabelecimento de chave (Fase 2, simulado):
//   1. Alice e Bob possuem pares de chaves X25519
//   2. Diffie-Hellman: shared_secret = Alice_priv × Bob_pub
//   3. Derivação: session_key = HKDF(shared_secret, nonce)
//   4. Comunicação: AES-256-GCM com session_key
//
// Estrutura de um pacote criptografado:
//   ┌─────────────────────────────────────────┐
//   │  Header (16 bytes): versão + flags      │
//   │  Sender NodeId (32 bytes)               │
//   │  Nonce (12 bytes): único por mensagem   │
//   │  Ciphertext (variável)                  │
//   │  GCM Auth Tag (16 bytes)                │
//   └─────────────────────────────────────────┘
//
// Fase 3: usar crate 'ring' ou 'aes-gcm' real (no_std)
// ============================================================

use spinning_top::Spinlock;

/// Overhead de criptografia por mensagem (bytes)
pub const CRYPTO_OVERHEAD: usize = 16 + 32 + 12 + 16; // header+id+nonce+tag

/// Versão do protocolo cripto
pub const CRYPTO_VERSION: u8 = 1;

// ─── Estruturas ───────────────────────────────────────────────────────────────

/// Chave de sessão AES-256 (32 bytes)
#[derive(Debug, Clone, Copy)]
pub struct SessionKey([u8; 32]);

/// Nonce para AES-GCM (12 bytes)
#[derive(Debug, Clone, Copy)]
pub struct Nonce([u8; 12]);

/// Pacote criptografado completo
#[derive(Debug, Clone)]
pub struct EncryptedPacket {
    /// Versão do protocolo
    pub version: u8,
    /// NodeId do remetente
    pub sender_id: [u8; 32],
    /// Nonce único desta mensagem
    pub nonce: Nonce,
    /// Dados cifrados
    pub ciphertext: Vec<u8>,
    /// Tag de autenticação GCM (16 bytes)
    pub auth_tag: [u8; 16],
}

/// Resultado de descriptografia
#[derive(Debug, Clone)]
pub enum DecryptResult {
    Success(Vec<u8>),
    AuthFailed,
    InvalidVersion,
    UnknownSender,
}

// ─── Operações Criptográficas ─────────────────────────────────────────────────

/// Deriva uma chave de sessão a partir do shared secret
/// Fase 3: usar HKDF-SHA256 real
fn derive_session_key(shared_secret: &[u8; 32], nonce: &[u8; 12]) -> SessionKey {
    let mut key = [0u8; 32];
    // Simulação de HKDF: XOR + mix
    for i in 0..32 {
        key[i] = shared_secret[i] ^ nonce[i % 12];
        key[i] = key[i].wrapping_add(0x5A);
        key[i] ^= shared_secret[(i + 16) % 32];
    }
    SessionKey(key)
}

/// Simula DH X25519 para obter shared secret
/// Fase 3: usar x25519-dalek ou ring
fn simulate_x25519(our_private: &[u8; 64], their_public: &[u8; 32]) -> [u8; 32] {
    let mut secret = [0u8; 32];
    for i in 0..32 {
        secret[i] = our_private[i] ^ their_public[i];
        secret[i] = secret[i].wrapping_mul(0x6B).wrapping_add(0x23);
        secret[i] ^= our_private[i + 32];
    }
    secret
}

/// Gera um nonce único baseado em contador + tick
fn generate_nonce(counter: u64, tick: u64) -> Nonce {
    let mut nonce = [0u8; 12];
    let c_bytes = counter.to_le_bytes();
    let t_bytes = tick.to_le_bytes();
    nonce[0..8].copy_from_slice(&c_bytes);
    nonce[4..12].copy_from_slice(&t_bytes); // Overlap intencional para mixing
    Nonce(nonce)
}

/// Cifra dados com AES-256-GCM simulado
/// Fase 3: usar aes-gcm crate no_std
fn encrypt_aes_gcm(key: &SessionKey, nonce: &Nonce, plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
    // Simulação: XOR com keystream derivado da chave + nonce
    // NÃO é AES real — substituir na Fase 3
    let mut ciphertext = Vec::with_capacity(plaintext.len());
    let mut keystream_state = key.0[0] as u64;

    for (i, &byte) in plaintext.iter().enumerate() {
        // Gera keystream pseudo-aleatório
        keystream_state = keystream_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(key.0[i % 32] as u64 ^ nonce.0[i % 12] as u64);
        let ks_byte = (keystream_state >> 33) as u8;
        ciphertext.push(byte ^ ks_byte);
    }

    // Tag de autenticação simulada (GHASH)
    let mut tag = [0u8; 16];
    let mut auth_state = u64::from_le_bytes(nonce.0[0..8].try_into().unwrap_or([0;8]));
    for (i, &byte) in ciphertext.iter().enumerate() {
        auth_state = auth_state.wrapping_mul(0x9e3779b97f4a7c15)
                               .wrapping_add(byte as u64)
                               .wrapping_add(key.0[i % 32] as u64);
        tag[i % 16] ^= (auth_state >> 24) as u8;
    }

    (ciphertext, tag)
}

/// Decifra dados com AES-256-GCM simulado
fn decrypt_aes_gcm(
    key: &SessionKey,
    nonce: &Nonce,
    ciphertext: &[u8],
    expected_tag: &[u8; 16],
) -> Option<Vec<u8>> {
    // Verifica tag antes de decifrar (fail-fast)
    let (decrypted, computed_tag) = encrypt_aes_gcm(key, nonce, ciphertext);

    // Comparação em tempo constante (evita timing attacks)
    let tag_ok = computed_tag.iter()
        .zip(expected_tag.iter())
        .fold(0u8, |acc, (&a, &b)| acc | (a ^ b)) == 0;

    if tag_ok { Some(decrypted) } else { None }
}

// ─── Gerenciador de Sessões ────────────────────────────────────────────────────

/// Sessão criptográfica com um peer
#[derive(Debug, Clone)]
pub struct CryptoSession {
    pub peer_id: [u8; 32],
    pub session_key: SessionKey,
    pub message_counter: u64,
    pub established_at_tick: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_encrypted: u64,
}

impl CryptoSession {
    /// Cifra uma mensagem para este peer
    pub fn encrypt(&mut self, plaintext: &[u8], tick: u64) -> EncryptedPacket {
        self.message_counter += 1;
        let nonce = generate_nonce(self.message_counter, tick);
        let (ciphertext, auth_tag) = encrypt_aes_gcm(&self.session_key, &nonce, plaintext);

        self.messages_sent += 1;
        self.bytes_encrypted += plaintext.len() as u64;

        // Obtém nosso NodeId
        let sender_id = crate::p2p::node::get_node_id();

        EncryptedPacket {
            version: CRYPTO_VERSION,
            sender_id,
            nonce,
            ciphertext,
            auth_tag,
        }
    }

    /// Decifra uma mensagem recebida
    pub fn decrypt(&mut self, packet: &EncryptedPacket) -> DecryptResult {
        if packet.version != CRYPTO_VERSION {
            return DecryptResult::InvalidVersion;
        }

        match decrypt_aes_gcm(
            &self.session_key,
            &packet.nonce,
            &packet.ciphertext,
            &packet.auth_tag,
        ) {
            Some(plaintext) => {
                self.messages_received += 1;
                DecryptResult::Success(plaintext)
            }
            None => DecryptResult::AuthFailed,
        }
    }
}

/// Gerenciador de sessões criptográficas
pub struct CryptoManager {
    pub sessions: alloc::collections::BTreeMap<[u8; 32], CryptoSession>,
    pub our_private_key: [u8; 64],
    pub total_bytes_encrypted: u64,
    pub total_messages: u64,
}

impl CryptoManager {
    fn new() -> Self {
        Self {
            sessions: alloc::collections::BTreeMap::new(),
            our_private_key: [0u8; 64],
            total_bytes_encrypted: 0,
            total_messages: 0,
        }
    }

    fn init_keys(&mut self) {
        // Em produção: carregar chaves do TPM ou gerar com TRNG
        let nid = crate::p2p::node::get_node_id();
        for (i, b) in self.our_private_key.iter_mut().enumerate() {
            *b = nid[i % 32].wrapping_add(i as u8).wrapping_mul(0x37);
        }
    }

    /// Estabelece sessão criptográfica com um peer
    pub fn establish_session(
        &mut self,
        peer_id: [u8; 32],
        peer_public_key: [u8; 32],
        tick: u64,
    ) -> bool {
        if self.sessions.contains_key(&peer_id) {
            return true; // Já existe
        }

        let shared_secret = simulate_x25519(&self.our_private_key, &peer_public_key);
        let nonce = generate_nonce(tick, peer_id[0] as u64);
        let session_key = derive_session_key(&shared_secret, &nonce.0);

        self.sessions.insert(peer_id, CryptoSession {
            peer_id,
            session_key,
            message_counter: 0,
            established_at_tick: tick,
            messages_sent: 0,
            messages_received: 0,
            bytes_encrypted: 0,
        });

        crate::serial_println!("[P2P][CRYPTO] Sessao estabelecida com {:02x}{:02x}...",
            peer_id[0], peer_id[1]);
        true
    }

    /// Cifra mensagem para um peer (estabelece sessão se necessário)
    pub fn encrypt_for(
        &mut self,
        peer_id: [u8; 32],
        peer_pk: [u8; 32],
        data: &[u8],
        tick: u64,
    ) -> Option<EncryptedPacket> {
        self.establish_session(peer_id, peer_pk, tick);
        let session = self.sessions.get_mut(&peer_id)?;
        let pkt = session.encrypt(data, tick);
        self.total_bytes_encrypted += data.len() as u64;
        self.total_messages += 1;
        Some(pkt)
    }

    pub fn stats(&self) -> CryptoStats {
        CryptoStats {
            active_sessions: self.sessions.len(),
            total_bytes_encrypted: self.total_bytes_encrypted,
            total_messages: self.total_messages,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CryptoStats {
    pub active_sessions: usize,
    pub total_bytes_encrypted: u64,
    pub total_messages: u64,
}

lazy_static::lazy_static! {
    static ref CRYPTO: Spinlock<CryptoManager> = Spinlock::new(CryptoManager {
        sessions: alloc::collections::BTreeMap::new(),
        our_private_key: [0u8; 64],
        total_bytes_encrypted: 0,
        total_messages: 0,
    });
}

pub fn init() {
    let mut mgr = CRYPTO.lock();
    mgr.init_keys();

    // Estabelece sessões com peers já conhecidos
    let peers = crate::p2p::peer::get_active_peers();
    let tick = 0u64;
    for peer in peers {
        let id = peer.node_id;
        let pk = peer.public_key;
        mgr.establish_session(id, pk, tick);
    }

    let stats = mgr.stats();
    crate::serial_println!("[P2P][CRYPTO] {} sessoes estabelecidas", stats.active_sessions);
}

pub fn get_stats() -> CryptoStats {
    CRYPTO.lock().stats()
}

pub fn encrypt_for_peer(peer_id: [u8; 32], peer_pk: [u8; 32], data: &[u8], tick: u64) -> Option<EncryptedPacket> {
    CRYPTO.lock().encrypt_for(peer_id, peer_pk, data, tick)
}
