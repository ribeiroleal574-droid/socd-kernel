// ============================================================
// SOC-D Kernel — DAG + Sincronização P2P (Fase 3)
// ============================================================
//
// Implementa um grafo acíclico dirigido (DAG) leve para:
//   - Versionamento de ficheiros e dados
//   - Sincronização entre dispositivos via P2P
//   - Confiança e integridade sem servidor central
//
// Estrutura de um bloco DAG:
//
//   ┌──────────────────────────────────────┐
//   │  hash: [u8;32]   (SHA-256 do bloco)  │
//   │  parents: Vec<[u8;32]>               │
//   │  author: [u8;32] (node_id)           │
//   │  timestamp: u64                      │
//   │  kind: BlockKind (File/Meta/Sync)    │
//   │  payload: Vec<u8>                    │
//   │  signature: [u8;64]                  │
//   └──────────────────────────────────────┘
//
// Protocolo de sync:
//   1. Nó A cria bloco → adiciona ao DAG local → broadcast via Gossip
//   2. Nó B recebe bloco → valida hash → verifica parents → insere
//   3. Conflito (dois nós editam o mesmo ficheiro) → merge por
//      timestamp + lexicographic node_id (CRDT Last-Write-Wins)
//
// Fase 3: simulação em memória com base para transport real (Fase 4)
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;

// ─── Hash simples (SHA-256 simulado) ─────────────────────────
// Em produção: usar sha2 crate. Aqui: FNV-like para no_std.

fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut h: [u64; 4] = [
        0xcbf29ce484222325,
        0x84222325cbf29ce4,
        0x14650fb0739d0383,
        0x739d038314650fb0,
    ];
    for (i, &byte) in data.iter().enumerate() {
        let slot = i % 4;
        h[slot] ^= byte as u64;
        h[slot] = h[slot].wrapping_mul(0x00000100000001b3);
        h[slot] ^= h[slot] >> 32;
    }
    let mut out = [0u8; 32];
    for (i, &v) in h.iter().enumerate() {
        out[i*8..(i+1)*8].copy_from_slice(&v.to_le_bytes());
    }
    out
}

fn hash_to_hex(h: &[u8; 32]) -> String {
    let mut s = String::new();
    for &b in h.iter().take(8) {
        let hi = b >> 4;
        let lo = b & 0xf;
        s.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + hi - 10) as char });
        s.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + lo - 10) as char });
    }
    s.push_str("...");
    s
}

// ─── Tipos de blocos ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    /// Conteúdo de ficheiro (path + dados)
    File,
    /// Metadados do sistema (config, permissões)
    Meta,
    /// Evento de sincronização entre nós
    Sync,
    /// Génesis — primeiro bloco de um DAG
    Genesis,
}

// ─── Bloco DAG ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DagBlock {
    /// Hash único do bloco (hash dos campos abaixo)
    pub hash:      [u8; 32],
    /// Hashes dos blocos pai (pode ter 0, 1 ou 2 pais — merge)
    pub parents:   Vec<[u8; 32]>,
    /// ID do nó que criou o bloco
    pub author:    [u8; 32],
    /// Timestamp (ticks do kernel)
    pub timestamp: u64,
    /// Tipo de bloco
    pub kind:      BlockKind,
    /// Path do recurso (ficheiro, chave de config, etc.)
    pub path:      String,
    /// Dados do bloco
    pub payload:   Vec<u8>,
    /// Sequência (version number dentro do path)
    pub seq:       u64,
}

impl DagBlock {
    /// Cria um novo bloco e calcula o seu hash
    pub fn new(
        parents:   Vec<[u8; 32]>,
        author:    [u8; 32],
        timestamp: u64,
        kind:      BlockKind,
        path:      String,
        payload:   Vec<u8>,
        seq:       u64,
    ) -> Self {
        let mut to_hash: Vec<u8> = Vec::new();
        // Hash = hash(parents + author + timestamp + kind_byte + path + payload)
        for p in &parents { to_hash.extend_from_slice(p); }
        to_hash.extend_from_slice(&author);
        to_hash.extend_from_slice(&timestamp.to_le_bytes());
        to_hash.push(match kind {
            BlockKind::File    => 0x01,
            BlockKind::Meta    => 0x02,
            BlockKind::Sync    => 0x03,
            BlockKind::Genesis => 0x00,
        });
        to_hash.extend_from_slice(path.as_bytes());
        to_hash.extend_from_slice(&payload);
        let hash = hash_bytes(&to_hash);
        Self { hash, parents, author, timestamp, kind, path, payload, seq }
    }

    /// Verifica integridade do bloco recalculando o hash
    pub fn verify(&self) -> bool {
        let recomputed = DagBlock::new(
            self.parents.clone(),
            self.author,
            self.timestamp,
            self.kind.clone(),
            self.path.clone(),
            self.payload.clone(),
            self.seq,
        );
        recomputed.hash == self.hash
    }
}

// ─── DAG Storage ─────────────────────────────────────────────

pub struct Dag {
    /// Todos os blocos indexados por hash
    blocks:    BTreeMap<[u8; 32], DagBlock>,
    /// HEAD de cada path: hash do bloco mais recente
    pub heads:     BTreeMap<String, [u8; 32]>,
    /// Sequência global de inserção (para ordem total)
    pub sequence:  u64,
    /// Node ID deste dispositivo
    node_id:   [u8; 32],
    /// Estatísticas
    pub stats: DagStats,
}

#[derive(Debug, Default, Clone)]
pub struct DagStats {
    pub total_blocks:   usize,
    pub file_blocks:    usize,
    pub sync_blocks:    usize,
    pub merge_count:    usize,
    pub conflicts_resolved: usize,
}

impl Dag {
    pub const fn new() -> Self {
        Self {
            blocks:   BTreeMap::new(),
            heads:    BTreeMap::new(),
            sequence: 0,
            node_id:  [0u8; 32],
            stats:    DagStats {
                total_blocks: 0,
                file_blocks: 0,
                sync_blocks: 0,
                merge_count: 0,
                conflicts_resolved: 0,
            },
        }
    }

    pub fn set_node_id(&mut self, id: [u8; 32]) {
        self.node_id = id;
    }

    /// Insere um bloco já criado (recebido de outro nó ou local)
    pub fn insert(&mut self, block: DagBlock) -> Result<(), DagError> {
        // 1. Verifica hash
        if !block.verify() {
            return Err(DagError::InvalidHash);
        }
        // 2. Não duplicar
        if self.blocks.contains_key(&block.hash) {
            return Ok(()); // idempotente
        }
        // 3. Verifica parents existem (ou são génesis)
        for parent_hash in &block.parents {
            if *parent_hash != [0u8; 32] && !self.blocks.contains_key(parent_hash) {
                return Err(DagError::MissingParent);
            }
        }
        // 4. Atualiza HEAD com resolução de conflitos (LWW + node_id)
        let path = block.path.clone();
        if let Some(&current_head) = self.heads.get(&path) {
            if let Some(current) = self.blocks.get(&current_head) {
                if block.timestamp > current.timestamp
                    || (block.timestamp == current.timestamp
                        && block.author > current.author)
                {
                    // Este bloco é mais recente — vence
                    self.heads.insert(path.clone(), block.hash);
                    self.stats.conflicts_resolved += 1;
                }
                self.stats.merge_count += 1;
            }
        } else {
            self.heads.insert(path.clone(), block.hash);
        }
        // 5. Atualiza estatísticas
        match block.kind {
            BlockKind::File    => self.stats.file_blocks += 1,
            BlockKind::Sync    => self.stats.sync_blocks += 1,
            _ => {}
        }
        self.stats.total_blocks += 1;
        self.blocks.insert(block.hash, block);
        Ok(())
    }

    /// Cria e insere um bloco de ficheiro
    pub fn write_file(&mut self, path: &str, data: Vec<u8>, tick: u64) -> [u8; 32] {
        let parent = self.heads.get(path).copied();
        let parents = match parent {
            Some(h) => alloc::vec![h],
            None    => alloc::vec![[0u8; 32]],
        };
        let seq = self.sequence;
        self.sequence += 1;
        let block = DagBlock::new(
            parents,
            self.node_id,
            tick,
            BlockKind::File,
            path.to_string(),
            data,
            seq,
        );
        let hash = block.hash;
        let _ = self.insert(block);
        hash
    }

    /// Retorna o conteúdo mais recente de um path
    pub fn read_file(&self, path: &str) -> Option<&[u8]> {
        let head = self.heads.get(path)?;
        let block = self.blocks.get(head)?;
        Some(&block.payload)
    }

    /// Lista todos os paths com HEAD
    pub fn list_paths(&self) -> Vec<(&str, &[u8; 32])> {
        self.heads.iter().map(|(p, h)| (p.as_str(), h)).collect()
    }

    /// Retorna histórico de versões de um path (do mais antigo ao mais recente)
    pub fn history(&self, path: &str) -> Vec<&DagBlock> {
        let mut result = Vec::new();
        let mut current = self.heads.get(path).copied();
        while let Some(hash) = current {
            if let Some(block) = self.blocks.get(&hash) {
                result.push(block);
                current = block.parents.first().copied()
                    .filter(|h| *h != [0u8; 32]);
            } else {
                break;
            }
        }
        result.reverse();
        result
    }

    /// Retorna blocos que o outro nó não tem (para sync)
    pub fn missing_blocks(&self, known: &[[u8; 32]]) -> Vec<&DagBlock> {
        self.blocks.values()
            .filter(|b| !known.contains(&b.hash))
            .collect()
    }

    /// Retorna os hashes de todos os blocos (para comparação com peers)
    pub fn all_hashes(&self) -> Vec<[u8; 32]> {
        self.blocks.keys().copied().collect()
    }
}

// ─── Erro DAG ────────────────────────────────────────────────

#[derive(Debug)]
pub enum DagError {
    InvalidHash,
    MissingParent,
    DuplicateBlock,
}

impl core::fmt::Display for DagError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            DagError::InvalidHash    => write!(f, "hash invalido"),
            DagError::MissingParent  => write!(f, "bloco pai em falta"),
            DagError::DuplicateBlock => write!(f, "bloco duplicado"),
        }
    }
}

// ─── Sync Engine ─────────────────────────────────────────────

pub struct SyncEngine {
    /// DAG local
    pub dag: Dag,
    /// Blocos pendentes de enviar aos peers
    pending_broadcast: Vec<[u8; 32]>,
    /// Tick da última sincronização
    last_sync_tick: u64,
    /// Intervalo de sync (ticks)
    sync_interval: u64,
}

impl SyncEngine {
    pub const fn new() -> Self {
        Self {
            dag:               Dag::new(),
            pending_broadcast: Vec::new(),
            last_sync_tick:    0,
            sync_interval:     300, // ~5 segundos a 60Hz
        }
    }

    /// Inicializa com o node_id deste dispositivo
    pub fn init(&mut self, node_id: [u8; 32], tick: u64) {
        self.dag.set_node_id(node_id);
        // Cria bloco génesis
        let genesis = DagBlock::new(
            alloc::vec![[0u8; 32]],
            node_id,
            tick,
            BlockKind::Genesis,
            "/".to_string(),
            b"SOC-D DAG Genesis".to_vec(),
            0,
        );
        let _ = self.dag.insert(genesis);
        crate::serial_println!("[DAG] DAG inicializado com bloco genesis");
    }

    /// Escreve um ficheiro — cria bloco DAG + agenda broadcast
    pub fn write(&mut self, path: &str, data: Vec<u8>, tick: u64) -> [u8; 32] {
        let hash = self.dag.write_file(path, data, tick);
        self.pending_broadcast.push(hash);
        crate::serial_println!("[DAG] write '{}' bloco={}", path, hash_to_hex(&hash));
        hash
    }

    /// Recebe um bloco de outro nó e tenta inserir no DAG local
    pub fn receive_block(&mut self, block: DagBlock) -> Result<(), DagError> {
        let hash = block.hash;
        let path = block.path.clone();
        match self.dag.insert(block) {
            Ok(()) => {
                crate::serial_println!("[DAG] bloco recebido '{}' hash={}",
                    path, hash_to_hex(&hash));
                Ok(())
            }
            Err(DagError::MissingParent) => {
                // Pede o bloco pai ao peer — simplificado aqui
                crate::serial_println!("[DAG] bloco '{}' aguarda pai — solicitar ao peer",
                    hash_to_hex(&hash));
                Err(DagError::MissingParent)
            }
            Err(e) => Err(e),
        }
    }

    /// Tick de sync — propaga blocos pendentes e pede estado dos peers
    pub fn tick(&mut self, current_tick: u64) {
        if current_tick - self.last_sync_tick < self.sync_interval {
            return;
        }
        self.last_sync_tick = current_tick;

        if !self.pending_broadcast.is_empty() {
            crate::serial_println!("[DAG] sync tick={} — {} blocos a propagar",
                current_tick, self.pending_broadcast.len());
            // Simula broadcast via P2P gossip
            self.broadcast_pending();
        }
    }

    fn broadcast_pending(&mut self) {
        let count = self.pending_broadcast.len();
        // Em Fase 4: enviar via p2p::transport::send()
        // Por agora: simula e limpa a fila
        for hash in &self.pending_broadcast {
            crate::serial_println!("[DAG][BROADCAST] bloco={}", hash_to_hex(hash));
        }
        self.pending_broadcast.clear();
        crate::serial_println!("[DAG] {} blocos propagados aos peers", count);
    }

    /// Prepara um "sync offer" — lista de hashes que temos
    /// para enviar a um peer que quer sincronizar
    pub fn sync_offer(&self) -> Vec<[u8; 32]> {
        self.dag.all_hashes()
    }

    /// Retorna blocos que o peer não tem
    pub fn sync_diff(&self, peer_hashes: &[[u8; 32]]) -> Vec<DagBlock> {
        self.dag.missing_blocks(peer_hashes)
            .into_iter()
            .cloned()
            .collect()
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static SYNC: Spinlock<SyncEngine> = Spinlock::new(SyncEngine::new());

// ─── API Pública ─────────────────────────────────────────────

/// Inicializa o DAG + sync engine com o node_id do P2P
pub fn init() {
    let node_id = crate::p2p::P2P_STATE.lock().node_id;
    let tick = crate::modules::scheduler::get_stats().current_tick;
    SYNC.lock().init(node_id, tick);
    crate::serial_println!("[DAG] Sync engine P2P ativo");
    crate::serial_println!("[DAG] Node: {}", hash_to_hex(&node_id));
}

/// Escreve um ficheiro no DAG (versiona automaticamente)
pub fn write(path: &str, data: Vec<u8>) {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    SYNC.lock().write(path, data, tick);
}

/// Lê o conteúdo mais recente de um path do DAG
pub fn read(path: &str) -> Option<Vec<u8>> {
    SYNC.lock().dag.read_file(path).map(|d| d.to_vec())
}

/// Lista todos os paths no DAG
pub fn list() -> Vec<String> {
    SYNC.lock().dag.list_paths()
        .into_iter()
        .map(|(p, _)| p.to_string())
        .collect()
}

/// Histórico de versões de um path
pub fn history(path: &str) -> Vec<(u64, String)> {
    SYNC.lock().dag.history(path)
        .into_iter()
        .map(|b| (b.seq, hash_to_hex(&b.hash)))
        .collect()
}

/// Tick de sincronização — chamado pelo kernel loop
pub fn sync_tick(current_tick: u64) {
    SYNC.lock().tick(current_tick);
}

/// Retorna estatísticas do DAG
pub fn stats() -> DagStats {
    SYNC.lock().dag.stats.clone()
}

/// Demonstração Fase 3: escreve ficheiros de teste e mostra versionamento
pub fn run_demo() {
    crate::serial_println!("\n[FASE3] === DAG + Sincronizacao P2P ===");

    write("/home/user/readme.txt", b"SOC-D v0.1.0 - Sistema Operacional Cognitivo".to_vec());
    write("/home/user/config.json", b"{\"theme\":\"dark\",\"lang\":\"pt\"}".to_vec());
    write("/sys/hostname", b"socd-node-1".to_vec());
    write("/home/user/readme.txt", b"SOC-D v0.1.0 - Atualizado!".to_vec()); // v2

    let s = stats();
    crate::serial_println!("[FASE3] DAG: {} blocos, {} ficheiros, {} merges",
        s.total_blocks, s.file_blocks, s.merge_count);

    let paths = list();
    crate::serial_println!("[FASE3] Paths no DAG:");
    for p in &paths {
        crate::serial_println!("[FASE3]   {}", p);
    }

    let hist = history("/home/user/readme.txt");
    crate::serial_println!("[FASE3] Historico readme.txt: {} versoes", hist.len());
    for (seq, hash) in &hist {
        crate::serial_println!("[FASE3]   v{} hash={}", seq, hash);
    }

    crate::serial_println!("[FASE3] Sync P2P: blocos pendentes para broadcast");
    crate::serial_println!("[FASE3] ======================================\n");
}
