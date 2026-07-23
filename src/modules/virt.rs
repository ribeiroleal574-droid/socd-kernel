// ============================================================
// SOC-D Kernel — Virtualização Leve / Containers (Fase 3.2)
// ============================================================
//
// Implementa isolamento de processos em "containers" leves,
// sem hypervisor completo. Inspirado em Linux namespaces + cgroups,
// adaptado para o ambiente bare-metal do SOC-D.
//
// Cada container tem:
//   - Namespace de ficheiros (vista isolada do TmpFS)
//   - Namespace de PIDs (PIDs relativos ao container)
//   - Limites de recursos (heap, ticks de CPU)
//   - Runtime type: Native | WASM | Linux-compat | Android-compat
//
// Arquitectura:
//
//   ┌─────────────────────────────────────────────────────┐
//   │                  ContainerManager                   │
//   ├─────────────┬──────────────┬───────────────────────┤
//   │  Container  │  Container   │  Container            │
//   │  (Native)   │  (WASM)      │  (Linux-compat)       │
//   │  fs: /app1  │  fs: /app2   │  fs: /app3            │
//   │  pid_ns: 1  │  pid_ns: 1   │  pid_ns: 1            │
//   │  cpu: 25%   │  cpu: 10%    │  cpu: 50%             │
//   └─────────────┴──────────────┴───────────────────────┘
//             ↓             ↓              ↓
//   ┌─────────────────────────────────────────────────────┐
//   │              SOC-D Kernel (bare metal)              │
//   │         Scheduler + TmpFS + WASM Runtime           │
//   └─────────────────────────────────────────────────────┘
//
// Fase 3: isolamento em memória (sem MMU por container)
// Fase 4: isolamento MMU real com page tables por container
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;
use crate::modules::scheduler::{Pid, Priority};

// ─── Runtime Type ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeKind {
    /// Processo nativo SOC-D (ELF x86_64)
    Native,
    /// WebAssembly (via WASM runtime interno)
    Wasm,
    /// Compatibilidade Linux (syscall translation)
    LinuxCompat,
    /// Compatibilidade Android (ART simulado)
    AndroidCompat,
}

impl RuntimeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeKind::Native       => "native",
            RuntimeKind::Wasm         => "wasm",
            RuntimeKind::LinuxCompat  => "linux",
            RuntimeKind::AndroidCompat=> "android",
        }
    }
}

// ─── Estado do Container ────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Stopped,
    Failed(String),
}

// ─── Limites de recursos ─────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Máximo de heap em bytes
    pub max_heap:    usize,
    /// Máximo de ticks de CPU por janela de scheduling
    pub cpu_ticks:   u64,
    /// Máximo de ficheiros abertos
    pub max_files:   usize,
    /// Máximo de processos filhos
    pub max_procs:   usize,
    /// Acesso à rede permitido
    pub net_access:  bool,
    /// Acesso P2P permitido
    pub p2p_access:  bool,
}

impl ResourceLimits {
    pub fn default() -> Self {
        Self {
            max_heap:   64 * 1024 * 1024, // 64 MB
            cpu_ticks:  1000,
            max_files:  64,
            max_procs:  8,
            net_access: true,
            p2p_access: false,
        }
    }

    pub fn minimal() -> Self {
        Self {
            max_heap:   8 * 1024 * 1024, // 8 MB
            cpu_ticks:  100,
            max_files:  16,
            max_procs:  2,
            net_access: false,
            p2p_access: false,
        }
    }

    pub fn trusted() -> Self {
        Self {
            max_heap:   256 * 1024 * 1024,
            cpu_ticks:  10000,
            max_files:  256,
            max_procs:  32,
            net_access: true,
            p2p_access: true,
        }
    }
}

// ─── Namespace de Ficheiros ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct FsNamespace {
    /// Root do container (mapeado no TmpFS global)
    pub root:    String,
    /// Montagens: path_no_container → path_no_host
    pub mounts:  BTreeMap<String, String>,
    /// Ficheiros criados dentro do container
    pub files:   BTreeMap<String, Vec<u8>>,
}

impl FsNamespace {
    pub fn new(root: String) -> Self {
        let mut mounts = BTreeMap::new();
        // Monta /proc e /sys como read-only por padrão
        mounts.insert("/proc".to_string(), "/sys/proc".to_string());
        mounts.insert("/sys".to_string(),  "/sys".to_string());
        Self { root, mounts, files: BTreeMap::new() }
    }

    pub fn write(&mut self, path: &str, data: Vec<u8>) {
        self.files.insert(path.to_string(), data);
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    pub fn resolve(&self, path: &str) -> String {
        alloc::format!("{}{}", self.root, path)
    }
}

// ─── Container ───────────────────────────────────────────────

pub struct Container {
    pub id:       u64,
    pub name:     String,
    pub runtime:  RuntimeKind,
    pub state:    ContainerState,
    pub limits:   ResourceLimits,
    pub fs:       FsNamespace,
    /// PIDs dos processos dentro do container
    pub pids:     Vec<Pid>,
    /// Variáveis de ambiente
    pub env:      BTreeMap<String, String>,
    /// Tick de criação
    pub created_at: u64,
    /// Ticks de CPU consumidos
    pub cpu_used: u64,
    /// Heap consumido (estimado)
    pub heap_used: usize,
}

impl Container {
    pub fn new(id: u64, name: String, runtime: RuntimeKind,
               limits: ResourceLimits, tick: u64) -> Self {
        let root = alloc::format!("/containers/{}", id);
        let mut env = BTreeMap::new();
        env.insert("CONTAINER_ID".to_string(), alloc::format!("{}", id));
        env.insert("RUNTIME".to_string(), runtime.as_str().to_string());
        env.insert("HOME".to_string(), alloc::format!("{}/home", root));
        Self {
            id, name, runtime,
            state: ContainerState::Created,
            limits,
            fs: FsNamespace::new(root),
            pids: Vec::new(),
            env,
            created_at: tick,
            cpu_used: 0,
            heap_used: 0,
        }
    }

    pub fn start(&mut self, entry: fn(), tick: u64) -> Pid {
        self.state = ContainerState::Running;
        let pid = crate::modules::scheduler::spawn(
            &self.name, entry, Priority::Normal
        );
        self.pids.push(pid);
        crate::serial_println!("[CONTAINER] '{}' (id={}) iniciado PID={}",
            self.name, self.id, pid);
        pid
    }

    pub fn stop(&mut self) {
        for &pid in &self.pids {
            crate::modules::scheduler::kill(pid, 0);
        }
        self.pids.clear();
        self.state = ContainerState::Stopped;
        crate::serial_println!("[CONTAINER] '{}' parado", self.name);
    }

    pub fn pause(&mut self) {
        self.state = ContainerState::Paused;
        crate::serial_println!("[CONTAINER] '{}' em pausa", self.name);
    }

    pub fn resume(&mut self) {
        if self.state == ContainerState::Paused {
            self.state = ContainerState::Running;
            crate::serial_println!("[CONTAINER] '{}' retomado", self.name);
        }
    }

    pub fn is_within_limits(&self) -> bool {
        self.heap_used  <= self.limits.max_heap &&
        self.cpu_used   <= self.limits.cpu_ticks * 1000 // com margem
    }

    pub fn info(&self) -> ContainerInfo {
        ContainerInfo {
            id:      self.id,
            name:    self.name.clone(),
            runtime: self.runtime.as_str(),
            state:   match &self.state {
                ContainerState::Created  => "created",
                ContainerState::Running  => "running",
                ContainerState::Paused   => "paused",
                ContainerState::Stopped  => "stopped",
                ContainerState::Failed(_)=> "failed",
            },
            pids:     self.pids.clone(),
            cpu_used: self.cpu_used,
            heap_mb:  self.heap_used / (1024 * 1024),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id:      u64,
    pub name:    String,
    pub runtime: &'static str,
    pub state:   &'static str,
    pub pids:    Vec<Pid>,
    pub cpu_used:u64,
    pub heap_mb: usize,
}

// ─── Container Manager ───────────────────────────────────────

pub struct ContainerManager {
    containers: Vec<Container>,
    next_id:    u64,
}

impl ContainerManager {
    pub const fn new() -> Self {
        Self { containers: Vec::new(), next_id: 1 }
    }

    /// Cria um novo container (não o inicia ainda)
    pub fn create(&mut self, name: &str, runtime: RuntimeKind,
                  limits: ResourceLimits, tick: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let c = Container::new(id, name.to_string(), runtime.clone(), limits, tick);
        crate::serial_println!("[CONTAINER] Criado '{}' id={} runtime={}",
            name, id, runtime.as_str());
        self.containers.push(c);
        id
    }

    /// Inicia um container pelo id com uma função de entrada
    pub fn start(&mut self, id: u64, entry: fn(), tick: u64) -> Option<Pid> {
        let c = self.containers.iter_mut().find(|c| c.id == id)?;
        Some(c.start(entry, tick))
    }

    /// Para um container
    pub fn stop(&mut self, id: u64) -> bool {
        if let Some(c) = self.containers.iter_mut().find(|c| c.id == id) {
            c.stop();
            true
        } else { false }
    }

    /// Remove um container parado
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.containers.len();
        self.containers.retain(|c| {
            if c.id == id {
                matches!(c.state, ContainerState::Stopped | ContainerState::Created)
            } else { true }
        });
        self.containers.len() < before
    }

    /// Lista todos os containers
    pub fn list(&self) -> Vec<ContainerInfo> {
        self.containers.iter().map(|c| c.info()).collect()
    }

    /// Estatísticas globais
    pub fn stats(&self) -> ContainerStats {
        ContainerStats {
            total:   self.containers.len(),
            running: self.containers.iter()
                .filter(|c| c.state == ContainerState::Running).count(),
            paused:  self.containers.iter()
                .filter(|c| c.state == ContainerState::Paused).count(),
            stopped: self.containers.iter()
                .filter(|c| c.state == ContainerState::Stopped).count(),
        }
    }

    /// Tick de gestão — verifica limites de recursos
    pub fn tick(&mut self, current_tick: u64) {
        for c in self.containers.iter_mut() {
            if c.state == ContainerState::Running {
                c.cpu_used += 1;
                if !c.is_within_limits() {
                    crate::serial_println!(
                        "[CONTAINER] '{}' excedeu limites — a parar", c.name);
                    c.stop();
                    c.state = ContainerState::Failed(
                        "resource limit exceeded".to_string());
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerStats {
    pub total:   usize,
    pub running: usize,
    pub paused:  usize,
    pub stopped: usize,
}

// ─── Instância Global ─────────────────────────────────────────

pub static CONTAINERS: Spinlock<ContainerManager> =
    Spinlock::new(ContainerManager::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[VIRT] Motor de containers inicializado");
    crate::serial_println!("[VIRT] Runtimes: native | wasm | linux | android");
}

pub fn create(name: &str, runtime: RuntimeKind, limits: ResourceLimits) -> u64 {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    CONTAINERS.lock().create(name, runtime, limits, tick)
}

pub fn start(id: u64, entry: fn()) -> Option<Pid> {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    CONTAINERS.lock().start(id, entry, tick)
}

pub fn stop(id: u64) -> bool  { CONTAINERS.lock().stop(id) }
pub fn remove(id: u64) -> bool { CONTAINERS.lock().remove(id) }
pub fn list() -> Vec<ContainerInfo> { CONTAINERS.lock().list() }
pub fn stats() -> ContainerStats { CONTAINERS.lock().stats() }

pub fn virt_tick(current_tick: u64) {
    CONTAINERS.lock().tick(current_tick);
}

// ─── Demonstração Fase 3.2 ───────────────────────────────────

pub fn run_demo() {
    crate::serial_println!("\n[FASE3.2] === Virtualizacao Leve / Containers ===");

    // Container 1: app nativa
    let id1 = create("socd-shell",  RuntimeKind::Native,       ResourceLimits::default());
    let id2 = create("wasm-app",    RuntimeKind::Wasm,         ResourceLimits::minimal());
    let id3 = create("linux-app",   RuntimeKind::LinuxCompat,  ResourceLimits::default());
    let id4 = create("android-app", RuntimeKind::AndroidCompat,ResourceLimits::minimal());

    // Inicia com tarefas de demonstração
    start(id1, demo_native_task);
    start(id2, demo_wasm_task);
    start(id3, demo_linux_task);
    // id4 fica em Created (não iniciado)

    let s = stats();
    crate::serial_println!("[FASE3.2] Containers: {} total | {} running | {} paused | {} stopped",
        s.total, s.running, s.paused, s.stopped);

    for c in list() {
        crate::serial_println!("[FASE3.2]   [{}] '{}' runtime={} state={} pids={:?}",
            c.id, c.name, c.runtime, c.state, c.pids);
    }

    crate::serial_println!("[FASE3.2] Use 'ct ls' no shell para listar containers");
    crate::serial_println!("[FASE3.2] =============================================\n");
}

fn demo_native_task() {
    crate::serial_println!("[CT:native] socd-shell a correr no container");
}
fn demo_wasm_task() {
    crate::serial_println!("[CT:wasm] wasm-app a correr no container");
}
fn demo_linux_task() {
    crate::serial_println!("[CT:linux] linux-app a correr no container (compat mode)");
}
