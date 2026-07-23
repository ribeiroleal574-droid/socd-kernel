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

/// Inicializa o TmpFS global com a estrutura de diretórios padrão
pub fn init() {
    let mut fs = TMPFS.lock();
    *fs = TmpFs::new();
    let stats = fs.fs_stats();
    crate::serial_println!(
        "[TMPFS] Inicializado: {} inodes ({} dirs, {} arquivos)",
        stats.total_inodes, stats.dirs, stats.files
    );
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
    fs.write_file(id, data)
}

/// Lista um diretório por caminho absoluto
pub fn ls(path: &str) -> Result<Vec<(String, InodeId)>, FsError> {
    let fs = TMPFS.lock();
    let id = fs.resolve_path(path)?;
    fs.list_dir(id)
}
