extern crate alloc;
// ============================================================
// SOC-D Kernel — TmpFS (Sistema de Arquivos em RAM)
// ============================================================
//
// O TmpFS é o sistema de arquivos padrão da Fase 1.
// Vive inteiramente na RAM (no heap do kernel) e serve como:
//   - Sistema de arquivos raiz durante o boot
//   - /dev  — dispositivos virtuais
//   - /proc — informações de processos (como Linux /proc)
//   - /tmp  — arquivos temporários de processos
//   - /mod  — módulos ELF carregáveis
//
// Estrutura de árvore:
//   Cada nó é um Inode que pode ser:
//   - Diretório: contém mapa de nome→inode_id
//   - Arquivo: contém Vec<u8> de dados
//   - Link simbólico: contém caminho alvo
//   - Dispositivo: contém funções de leitura/escrita
//
// Fase 2: Persistência via protocolo P2P (sincronização entre nós)
// Fase 3: VFS — camada de abstração suportando ext4, btrfs, etc.
// ============================================================

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;

// ─── Identificadores ──────────────────────────────────────────────────────────

/// Identificador de inode
pub type InodeId = u64;

const ROOT_INODE: InodeId = 1;

/// Assinatura no início de um snapshot válido em disco
const SNAPSHOT_MAGIC: &[u8; 8] = b"SOCDFS01";

static NEXT_INODE: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(2);

fn alloc_inode_id() -> InodeId {
    NEXT_INODE.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

// ─── Tipos de Nó ─────────────────────────────────────────────────────────────

/// Tipo de um nó no sistema de arquivos
#[derive(Debug, Clone, PartialEq)]
pub enum InodeKind {
    /// Diretório — contém outros inodes
    Directory,
    /// Arquivo regular — contém dados brutos
    File,
    /// Link simbólico — aponta para outro caminho
    Symlink,
}

/// Permissões de acesso (simplificadas para Fase 1)
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub read:    bool,
    pub write:   bool,
    pub execute: bool,
}

impl Permissions {
    pub fn read_write()    -> Self { Self { read: true, write: true, execute: false } }
    pub fn read_only()     -> Self { Self { read: true, write: false, execute: false } }
    pub fn read_execute()  -> Self { Self { read: true, write: false, execute: true } }
    pub fn full()          -> Self { Self { read: true, write: true, execute: true } }
}

// ─── Inode ───────────────────────────────────────────────────────────────────

/// Um nó no sistema de arquivos
#[derive(Debug, Clone)]
pub struct Inode {
    /// Identificador único
    pub id: InodeId,
    /// Nome do nó (sem path)
    pub name: String,
    /// Tipo: diretório, arquivo ou link
    pub kind: InodeKind,
    /// Permissões
    pub perms: Permissions,
    /// Tick de criação
    pub created_at: u64,
    /// Tick da última modificação
    pub modified_at: u64,
    /// Conteúdo (arquivo: dados; diretório: entradas; link: caminho)
    pub content: InodeContent,
}

/// Conteúdo de um inode por tipo
#[derive(Debug, Clone)]
pub enum InodeContent {
    /// Dados de arquivo
    FileData(Vec<u8>),
    /// Entradas de diretório: nome → inode_id
    DirEntries(BTreeMap<String, InodeId>),
    /// Caminho alvo de link simbólico
    SymlinkTarget(String),
}

impl Inode {
    /// Tamanho em bytes
    pub fn size(&self) -> usize {
        match &self.content {
            InodeContent::FileData(data)     => data.len(),
            InodeContent::DirEntries(map)    => map.len() * 32, // Estimativa
            InodeContent::SymlinkTarget(s)   => s.len(),
        }
    }
}

// ─── Erros do FS ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum FsError {
    NotFound(String),
    NotADirectory(String),
    NotAFile(String),
    AlreadyExists(String),
    PermissionDenied(String),
    InvalidPath(String),
    DirectoryNotEmpty(String),
    OutOfMemory,
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FsError::NotFound(p)          => write!(f, "Nao encontrado: {}", p),
            FsError::NotADirectory(p)     => write!(f, "Nao e diretorio: {}", p),
            FsError::NotAFile(p)          => write!(f, "Nao e arquivo: {}", p),
            FsError::AlreadyExists(p)     => write!(f, "Ja existe: {}", p),
            FsError::PermissionDenied(p)  => write!(f, "Permissao negada: {}", p),
            FsError::InvalidPath(p)       => write!(f, "Caminho invalido: {}", p),
            FsError::DirectoryNotEmpty(p) => write!(f, "Diretorio nao vazio: {}", p),
            FsError::OutOfMemory          => write!(f, "Sem memoria"),
        }
    }
}

// ─── TmpFS ───────────────────────────────────────────────────────────────────

/// Sistema de arquivos em RAM do SOC-D
pub struct TmpFs {
    /// Todos os inodes indexados por ID
    inodes: BTreeMap<InodeId, Inode>,
    /// Tick atual (atualizado externamente)
    current_tick: u64,
}

impl TmpFs {
    /// Cria um novo TmpFS com a estrutura de diretórios padrão
    pub fn new() -> Self {
        let mut fs = Self {
            inodes: BTreeMap::new(),
            current_tick: 0,
        };
        fs.init_root_structure();
        fs
    }

    /// Cria a árvore de diretórios padrão do SOC-D
    fn init_root_structure(&mut self) {
        // Diretório raiz
        self.create_dir_inode(ROOT_INODE, "/", 0);

        // Estrutura padrão
        let dirs = [
            ("bin",  "Executaveis do sistema"),
            ("dev",  "Dispositivos virtuais"),
            ("etc",  "Configuracoes do sistema"),
            ("mod",  "Modulos ELF carregaveis"),
            ("proc", "Informacoes de processos"),
            ("tmp",  "Arquivos temporarios"),
            ("var",  "Dados variaveis"),
        ];

        for (name, _desc) in &dirs {
            let id = alloc_inode_id();
            self.create_dir_inode(id, name, ROOT_INODE);
        }

        // Arquivo de versão em /etc/version
        let etc_id = self.find_in_dir(ROOT_INODE, "etc").unwrap_or(0);
        if etc_id != 0 {
            self.create_file(etc_id, "version",
                b"SOC-D Kernel v0.1.0-alpha\nFase 1 - Kernel Base\n")
                .ok();
            self.create_file(etc_id, "hostname", b"socd-node-1\n").ok();
        }

        // Arquivos virtuais em /proc
        let proc_id = self.find_in_dir(ROOT_INODE, "proc").unwrap_or(0);
        if proc_id != 0 {
            self.create_file(proc_id, "uptime",   b"0\n").ok();
            self.create_file(proc_id, "meminfo",  b"MemTotal: 262144 kB\n").ok();
            self.create_file(proc_id, "modules",  b"(nenhum modulo externo carregado)\n").ok();
        }
    }

    /// Cria um inode de diretório
    fn create_dir_inode(&mut self, id: InodeId, name: &str, parent: InodeId) {
        let mut entries = BTreeMap::new();

        // Entradas especiais
        entries.insert(".".to_string(), id);
        if parent != 0 {
            entries.insert("..".to_string(), parent);
        }

        let inode = Inode {
            id,
            name: name.to_string(),
            kind: InodeKind::Directory,
            perms: Permissions::full(),
            created_at: self.current_tick,
            modified_at: self.current_tick,
            content: InodeContent::DirEntries(entries),
        };

        self.inodes.insert(id, inode);

        // Adiciona entrada no diretório pai
        if parent != 0 && parent != id {
            if let Some(parent_inode) = self.inodes.get_mut(&parent) {
                if let InodeContent::DirEntries(ref mut map) = parent_inode.content {
                    map.insert(name.to_string(), id);
                }
            }
        }
    }

    /// Cria um arquivo em um diretório
    pub fn create_file(&mut self, dir_id: InodeId, name: &str, data: &[u8]) -> Result<InodeId, FsError> {
        // Verifica se o diretório existe
        if !self.inodes.contains_key(&dir_id) {
            return Err(FsError::NotFound(dir_id.to_string()));
        }

        // Verifica se já existe
        if self.find_in_dir(dir_id, name).is_some() {
            return Err(FsError::AlreadyExists(name.to_string()));
        }

        let file_id = alloc_inode_id();
        let inode = Inode {
            id: file_id,
            name: name.to_string(),
            kind: InodeKind::File,
            perms: Permissions::read_write(),
            created_at: self.current_tick,
            modified_at: self.current_tick,
            content: InodeContent::FileData(data.to_vec()),
        };

        self.inodes.insert(file_id, inode);

        // Adiciona entrada no diretório pai
        if let Some(dir) = self.inodes.get_mut(&dir_id) {
            if let InodeContent::DirEntries(ref mut map) = dir.content {
                map.insert(name.to_string(), file_id);
                dir.modified_at = self.current_tick;
            }
        }

        Ok(file_id)
    }

    /// Cria um diretório
    pub fn mkdir(&mut self, parent_id: InodeId, name: &str) -> Result<InodeId, FsError> {
        if self.find_in_dir(parent_id, name).is_some() {
            return Err(FsError::AlreadyExists(name.to_string()));
        }

        let id = alloc_inode_id();
        self.create_dir_inode(id, name, parent_id);
        Ok(id)
    }

    /// Lê o conteúdo de um arquivo
    pub fn read_file(&self, inode_id: InodeId) -> Result<&[u8], FsError> {
        let inode = self.inodes.get(&inode_id)
            .ok_or_else(|| FsError::NotFound(inode_id.to_string()))?;

        match &inode.content {
            InodeContent::FileData(data) => Ok(data.as_slice()),
            _ => Err(FsError::NotAFile(inode.name.clone())),
        }
    }

    /// Escreve dados em um arquivo (substitui conteúdo)
    pub fn write_file(&mut self, inode_id: InodeId, data: &[u8]) -> Result<(), FsError> {
        let inode = self.inodes.get_mut(&inode_id)
            .ok_or_else(|| FsError::NotFound(inode_id.to_string()))?;

        if !inode.perms.write {
            return Err(FsError::PermissionDenied(inode.name.clone()));
        }

        match &mut inode.content {
            InodeContent::FileData(ref mut buf) => {
                buf.clear();
                buf.extend_from_slice(data);
                inode.modified_at = self.current_tick;
                Ok(())
            }
            _ => Err(FsError::NotAFile(inode.name.clone())),
        }
    }

    /// Lê entradas de um diretório
    pub fn list_dir(&self, dir_id: InodeId) -> Result<Vec<(String, InodeId)>, FsError> {
        let inode = self.inodes.get(&dir_id)
            .ok_or_else(|| FsError::NotFound(dir_id.to_string()))?;

        match &inode.content {
            InodeContent::DirEntries(map) => {
                Ok(map.iter()
                    .filter(|(k, _)| k.as_str() != "." && k.as_str() != "..")
                    .map(|(k, v)| (k.clone(), *v))
                    .collect())
            }
            _ => Err(FsError::NotADirectory(inode.name.clone())),
        }
    }

    /// Busca um inode pelo nome dentro de um diretório
    pub fn find_in_dir(&self, dir_id: InodeId, name: &str) -> Option<InodeId> {
        let inode = self.inodes.get(&dir_id)?;
        match &inode.content {
            InodeContent::DirEntries(map) => map.get(name).copied(),
            _ => None,
        }
    }

    /// Resolve um caminho absoluto para um inode_id
    pub fn resolve_path(&self, path: &str) -> Result<InodeId, FsError> {
        if path == "/" {
            return Ok(ROOT_INODE);
        }

        let mut current = ROOT_INODE;
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        for part in parts {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                // Sobe um nível
                if let Some(inode) = self.inodes.get(&current) {
                    if let InodeContent::DirEntries(map) = &inode.content {
                        current = *map.get("..").unwrap_or(&ROOT_INODE);
                    }
                }
                continue;
            }

            match self.find_in_dir(current, part) {
                Some(id) => current = id,
                None => return Err(FsError::NotFound(path.to_string())),
            }
        }

        Ok(current)
    }

    /// Retorna informações de um inode
    pub fn stat(&self, inode_id: InodeId) -> Option<FsStat> {
        let inode = self.inodes.get(&inode_id)?;
        Some(FsStat {
            id: inode.id,
            name: inode.name.clone(),
            kind: inode.kind.clone(),
            size: inode.size(),
            perms: inode.perms,
        })
    }

    /// Estatísticas globais do FS
    pub fn fs_stats(&self) -> TmpFsStats {
        let total_inodes = self.inodes.len();
        let total_bytes: usize = self.inodes.values().map(|i| i.size()).sum();
        let files = self.inodes.values().filter(|i| i.kind == InodeKind::File).count();
        let dirs  = self.inodes.values().filter(|i| i.kind == InodeKind::Directory).count();

        TmpFsStats { total_inodes, total_bytes, files, dirs }
    }

    // ─── Snapshot em disco (Fase 8) ──────────────────────────

    /// Serializa todo o TmpFS para bytes (formato binário simples,
    /// próprio — sem serde, para não trazer dependências pesadas
    /// para um kernel bare-metal). Usado para gravar um snapshot no
    /// disco virtio-blk.
    pub fn serialize(&self, next_inode: u64) -> Vec<u8> {
        let mut buf = Vec::new();

        fn push_u16(b: &mut Vec<u8>, v: u16) { b.extend_from_slice(&v.to_le_bytes()); }
        fn push_u32(b: &mut Vec<u8>, v: u32) { b.extend_from_slice(&v.to_le_bytes()); }
        fn push_u64(b: &mut Vec<u8>, v: u64) { b.extend_from_slice(&v.to_le_bytes()); }
        fn push_str(b: &mut Vec<u8>, s: &str) {
            push_u16(b, s.len() as u16);
            b.extend_from_slice(s.as_bytes());
        }

        buf.extend_from_slice(SNAPSHOT_MAGIC);
        push_u64(&mut buf, 0); // placeholder para total_len, preenchido no fim
        push_u64(&mut buf, next_inode);
        push_u64(&mut buf, self.current_tick);
        push_u32(&mut buf, self.inodes.len() as u32);

        for inode in self.inodes.values() {
            push_u64(&mut buf, inode.id);
            push_str(&mut buf, &inode.name);
            let kind_byte = match inode.kind {
                InodeKind::Directory => 0u8,
                InodeKind::File      => 1u8,
                InodeKind::Symlink   => 2u8,
            };
            buf.push(kind_byte);
            let perm_byte = (inode.perms.read as u8)
                | ((inode.perms.write as u8) << 1)
                | ((inode.perms.execute as u8) << 2);
            buf.push(perm_byte);
            push_u64(&mut buf, inode.created_at);
            push_u64(&mut buf, inode.modified_at);

            match &inode.content {
                InodeContent::FileData(data) => {
                    buf.push(1);
                    push_u32(&mut buf, data.len() as u32);
                    buf.extend_from_slice(data);
                }
                InodeContent::DirEntries(map) => {
                    buf.push(0);
                    push_u32(&mut buf, map.len() as u32);
                    for (name, child_id) in map {
                        push_str(&mut buf, name);
                        push_u64(&mut buf, *child_id);
                    }
                }
                InodeContent::SymlinkTarget(target) => {
                    buf.push(2);
                    push_str(&mut buf, target);
                }
            }
        }

        let total_len = buf.len() as u64;
        buf[8..16].copy_from_slice(&total_len.to_le_bytes());
        buf
    }

    /// Reconstrói um TmpFS a partir de bytes gravados por `serialize`.
    /// Devolve também o valor de `next_inode` guardado, para o
    /// contador global `NEXT_INODE` poder ser restaurado.
    pub fn deserialize(data: &[u8]) -> Option<(Self, u64)> {
        if data.len() < 8 + 8 + 8 + 8 + 4 { return None; }
        if &data[0..8] != SNAPSHOT_MAGIC { return None; }

        let mut off = 8usize;
        fn read_u16(d: &[u8], o: usize) -> u16 { u16::from_le_bytes([d[o], d[o+1]]) }
        fn read_u32(d: &[u8], o: usize) -> u32 { u32::from_le_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
        fn read_u64(d: &[u8], o: usize) -> u64 {
            let mut a = [0u8; 8];
            a.copy_from_slice(&d[o..o+8]);
            u64::from_le_bytes(a)
        }

        let total_len = read_u64(data, off); off += 8;
        if total_len as usize > data.len() { return None; } // snapshot truncado/corrompido

        let next_inode = read_u64(data, off); off += 8;
        let current_tick = read_u64(data, off); off += 8;
        let inode_count = read_u32(data, off); off += 4;

        let mut inodes = BTreeMap::new();

        for _ in 0..inode_count {
            if off + 8 > data.len() { return None; }
            let id = read_u64(data, off); off += 8;

            if off + 2 > data.len() { return None; }
            let name_len = read_u16(data, off) as usize; off += 2;
            if off + name_len > data.len() { return None; }
            let name = core::str::from_utf8(&data[off..off+name_len]).ok()?.to_string();
            off += name_len;

            if off + 2 > data.len() { return None; }
            let kind = match data[off] {
                0 => InodeKind::Directory,
                1 => InodeKind::File,
                2 => InodeKind::Symlink,
                _ => return None,
            };
            let perm_byte = data[off+1];
            off += 2;
            let perms = Permissions {
                read:    perm_byte & 0x1 != 0,
                write:   perm_byte & 0x2 != 0,
                execute: perm_byte & 0x4 != 0,
            };

            if off + 16 > data.len() { return None; }
            let created_at = read_u64(data, off); off += 8;
            let modified_at = read_u64(data, off); off += 8;

            if off + 1 > data.len() { return None; }
            let content_tag = data[off]; off += 1;
            let content = match content_tag {
                1 => {
                    if off + 4 > data.len() { return None; }
                    let len = read_u32(data, off) as usize; off += 4;
                    if off + len > data.len() { return None; }
                    let d = data[off..off+len].to_vec();
                    off += len;
                    InodeContent::FileData(d)
                }
                0 => {
                    if off + 4 > data.len() { return None; }
                    let count = read_u32(data, off); off += 4;
                    let mut map = BTreeMap::new();
                    for _ in 0..count {
                        if off + 2 > data.len() { return None; }
                        let nlen = read_u16(data, off) as usize; off += 2;
                        if off + nlen > data.len() { return None; }
                        let name = core::str::from_utf8(&data[off..off+nlen]).ok()?.to_string();
                        off += nlen;
                        if off + 8 > data.len() { return None; }
                        let child_id = read_u64(data, off); off += 8;
                        map.insert(name, child_id);
                    }
                    InodeContent::DirEntries(map)
                }
                2 => {
                    if off + 2 > data.len() { return None; }
                    let tlen = read_u16(data, off) as usize; off += 2;
                    if off + tlen > data.len() { return None; }
                    let target = core::str::from_utf8(&data[off..off+tlen]).ok()?.to_string();
                    off += tlen;
                    InodeContent::SymlinkTarget(target)
                }
                _ => return None,
            };

            inodes.insert(id, Inode { id, name, kind, perms, created_at, modified_at, content });
        }

        Some((Self { inodes, current_tick }, next_inode))
    }
}

/// Informações de um inode (equivalente ao stat() do Unix)
#[derive(Debug, Clone)]
pub struct FsStat {
    pub id:    InodeId,
    pub name:  String,
    pub kind:  InodeKind,
    pub size:  usize,
    pub perms: Permissions,
}

/// Estatísticas globais do TmpFS
#[derive(Debug, Clone)]
pub struct TmpFsStats {
    pub total_inodes: usize,
    pub total_bytes:  usize,
    pub files:        usize,
    pub dirs:         usize,
}

// ─── Instância Global ────────────────────────────────────────────────────────

pub static TMPFS: Spinlock<TmpFs> = Spinlock::new(TmpFs {
    inodes: BTreeMap::new(),
    current_tick: 0,
});

/// Inicializa o TmpFS global — tenta primeiro carregar um snapshot
/// válido do disco virtio-blk (ver `load_from_disk`); se não houver
/// disco, ou não houver snapshot válido ainda (primeiro arranque),
/// cria a estrutura de diretórios padrão em RAM e grava-a no disco
/// (se disponível) para o próximo arranque já ter algo para carregar.
pub fn init() {
    if load_from_disk() {
        let stats = TMPFS.lock().fs_stats();
        crate::serial_println!(
            "[TMPFS] Snapshot carregado do disco: {} inodes ({} dirs, {} arquivos)",
            stats.total_inodes, stats.dirs, stats.files
        );
        return;
    }

    let mut fs = TMPFS.lock();
    *fs = TmpFs::new();
    let stats = fs.fs_stats();
    crate::serial_println!(
        "[TMPFS] Inicializado: {} inodes ({} dirs, {} arquivos)",
        stats.total_inodes, stats.dirs, stats.files
    );
    drop(fs);
    if save_to_disk() {
        crate::serial_println!("[TMPFS] Snapshot inicial gravado no disco");
    }
}

/// Grava um snapshot completo do TmpFS no disco virtio-blk (sector 0
/// em diante). Sem efeito (devolve false) se não houver disco real
/// disponível — TmpFS continua a funcionar normalmente só em RAM.
pub fn save_to_disk() -> bool {
    if !crate::drivers::virtio_blk::is_up() { return false; }
    let next_inode = NEXT_INODE.load(core::sync::atomic::Ordering::Relaxed);
    let data = TMPFS.lock().serialize(next_inode);
    crate::drivers::virtio_blk::write_at(0, &data)
}

/// Carrega um snapshot do disco virtio-blk, se existir e for válido.
/// Lê primeiro um bloco pequeno para confirmar a assinatura e o
/// tamanho total, depois lê exactamente esse tamanho.
pub fn load_from_disk() -> bool {
    if !crate::drivers::virtio_blk::is_up() { return false; }

    let mut header = alloc::vec![0u8; 512];
    if !crate::drivers::virtio_blk::read_at(0, &mut header) { return false; }
    if &header[0..8] != SNAPSHOT_MAGIC { return false; }
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&header[8..16]);
    let total_len = u64::from_le_bytes(len_bytes) as usize;
    if total_len == 0 || total_len > 64 * 1024 * 1024 { return false; } // limite defensivo (64 MB)

    let padded_len = (total_len + 511) & !511;
    let mut data = alloc::vec![0u8; padded_len];
    if !crate::drivers::virtio_blk::read_at(0, &mut data) { return false; }
    data.truncate(total_len);

    match TmpFs::deserialize(&data) {
        Some((fs, next_inode)) => {
            *TMPFS.lock() = fs;
            NEXT_INODE.store(next_inode, core::sync::atomic::Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Lê um arquivo por caminho absoluto
pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    let fs = TMPFS.lock();
    let id = fs.resolve_path(path)?;
    fs.read_file(id).map(|d| d.to_vec())
}

/// Escreve em um arquivo por caminho absoluto
pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    let mut fs = TMPFS.lock();
    let id = fs.resolve_path(path)?;
    let result = fs.write_file(id, data);
    drop(fs);
    if result.is_ok() { save_to_disk(); }
    result
}

/// Lista um diretório por caminho absoluto
pub fn ls(path: &str) -> Result<Vec<(String, InodeId)>, FsError> {
    let fs = TMPFS.lock();
    let id = fs.resolve_path(path)?;
    fs.list_dir(id)
}
