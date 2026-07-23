// ============================================================
// SOC-D Kernel — Interface Cross-Device (Fase 3.4)
// ============================================================
//
// Sincronização de estado e handoff entre dispositivos:
//   PC ↔ Mobile ↔ AR/VR ↔ IoT ↔ Smart TV
//
// Funcionalidades:
//   - Registo de dispositivos no cluster P2P
//   - Handoff de sessão (continuar no outro dispositivo)
//   - Sync de clipboard, estado de apps e janelas
//   - Canal de presença (saber quais dispositivos estão online)
//   - Streaming de ecrã adaptativo (PC→mobile, PC→AR)
//
// Protocolo de handoff:
//
//   Dispositivo A (PC)              Dispositivo B (Mobile)
//   ─────────────────               ──────────────────────
//   1. handoff_request(session)  →
//                                ← 2. handoff_accept(device_b)
//   3. transfer_state(payload)   →
//                                ← 4. handoff_complete
//   5. session_suspended             5. session_resumed
//
// Serialização: binário compacto (sem serde — no_std puro)
// Transporte:   P2P gossip + DAG para estado persistente
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;

// ─── Tipos de Dispositivo ────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceKind {
    Desktop,
    Laptop,
    Mobile,
    Tablet,
    ArGlasses,
    VrHeadset,
    SmartTv,
    IoT,
    Server,
}

impl DeviceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceKind::Desktop    => "desktop",
            DeviceKind::Laptop     => "laptop",
            DeviceKind::Mobile     => "mobile",
            DeviceKind::Tablet     => "tablet",
            DeviceKind::ArGlasses  => "ar-glasses",
            DeviceKind::VrHeadset  => "vr-headset",
            DeviceKind::SmartTv    => "smart-tv",
            DeviceKind::IoT        => "iot",
            DeviceKind::Server     => "server",
        }
    }

    /// Resolução padrão para o tipo de dispositivo
    pub fn default_resolution(&self) -> (u32, u32) {
        match self {
            DeviceKind::Desktop    => (2560, 1440),
            DeviceKind::Laptop     => (1920, 1080),
            DeviceKind::Mobile     => (1080, 2340),
            DeviceKind::Tablet     => (2048, 1536),
            DeviceKind::ArGlasses  => (1832, 1920),
            DeviceKind::VrHeadset  => (2160, 2160),
            DeviceKind::SmartTv    => (3840, 2160),
            DeviceKind::IoT        => (320, 240),
            DeviceKind::Server     => (0, 0),
        }
    }

    /// Capacidades de input do dispositivo
    pub fn input_caps(&self) -> &'static str {
        match self {
            DeviceKind::Desktop   | DeviceKind::Laptop  => "keyboard+mouse+touch",
            DeviceKind::Mobile    | DeviceKind::Tablet  => "touch+voice+gyro",
            DeviceKind::ArGlasses | DeviceKind::VrHeadset => "gesture+gaze+voice+controller",
            DeviceKind::SmartTv   => "remote+voice",
            DeviceKind::IoT       => "sensors",
            DeviceKind::Server    => "none",
        }
    }
}

// ─── Dispositivo no Cluster ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Node ID (mesmo que P2P node_id)
    pub node_id:    [u8; 32],
    pub name:       String,
    pub kind:       DeviceKind,
    pub online:     bool,
    pub last_seen:  u64,
    /// Capacidades em % (0–100)
    pub battery:    Option<u8>,
    pub cpu_load:   u8,
    pub ram_free_mb: u32,
    /// IP local no cluster
    pub local_ip:   [u8; 4],
}

impl DeviceInfo {
    pub fn this_device(node_id: [u8; 32], tick: u64) -> Self {
        Self {
            node_id,
            name:       "socd-node".to_string(),
            kind:       DeviceKind::Desktop,
            online:     true,
            last_seen:  tick,
            battery:    None,
            cpu_load:   0,
            ram_free_mb: 256,
            local_ip:   [10, 0, 2, 15],
        }
    }

    fn node_id_short(&self) -> String {
        let mut s = String::new();
        for &b in self.node_id.iter().take(4) {
            let hi = b >> 4;
            let lo = b & 0xf;
            s.push(if hi < 10 { (b'0'+hi) as char } else { (b'a'+hi-10) as char });
            s.push(if lo < 10 { (b'0'+lo) as char } else { (b'a'+lo-10) as char });
        }
        s.push_str("..");
        s
    }
}

// ─── Sessão Cross-Device ─────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Active,
    Suspended,
    Transferring,
    Resumed,
}

#[derive(Debug, Clone)]
pub struct CrossSession {
    pub id:          u64,
    pub app_name:    String,
    pub state:       SessionState,
    pub origin:      [u8; 32],      // node_id do dispositivo de origem
    pub current:     [u8; 32],      // node_id do dispositivo atual
    /// Estado serializado da sessão (app state snapshot)
    pub payload:     Vec<u8>,
    /// Tick de criação
    pub created_at:  u64,
    /// Tick da última transferência
    pub transferred_at: Option<u64>,
}

impl CrossSession {
    pub fn new(id: u64, app_name: &str, node_id: [u8; 32],
               payload: Vec<u8>, tick: u64) -> Self {
        Self {
            id,
            app_name: app_name.to_string(),
            state: SessionState::Active,
            origin:  node_id,
            current: node_id,
            payload,
            created_at: tick,
            transferred_at: None,
        }
    }
}

// ─── Clipboard Distribuído ───────────────────────────────────

#[derive(Debug, Clone)]
pub enum ClipboardContent {
    Text(String),
    Bytes(Vec<u8>),
    File { path: String, size: usize },
    Empty,
}

impl ClipboardContent {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClipboardContent::Text(_)  => "text",
            ClipboardContent::Bytes(_) => "bytes",
            ClipboardContent::File{..} => "file",
            ClipboardContent::Empty    => "empty",
        }
    }
}

// ─── Evento de Presença ──────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PresenceEvent {
    DeviceOnline  { node_id: [u8; 32], kind: DeviceKind, name: String },
    DeviceOffline { node_id: [u8; 32] },
    SessionHandoff { session_id: u64, from: [u8; 32], to: [u8; 32] },
    ClipboardSync  { from: [u8; 32] },
}

// ─── Barramento Cross-Device ─────────────────────────────────

pub struct CrossDeviceBus {
    /// Este dispositivo
    pub this_device:  Option<DeviceInfo>,
    /// Dispositivos conhecidos no cluster
    pub devices:      BTreeMap<[u8; 32], DeviceInfo>,
    /// Sessões ativas (locais e remotas)
    pub sessions:     Vec<CrossSession>,
    /// Clipboard distribuído
    pub clipboard:    ClipboardContent,
    /// Log de eventos de presença
    pub events:       Vec<PresenceEvent>,
    /// Próximo ID de sessão
    next_session_id:  u64,
    /// Estatísticas
    pub stats:        CrossDeviceStats,
}

#[derive(Debug, Clone, Default)]
pub struct CrossDeviceStats {
    pub devices_seen:     usize,
    pub sessions_created: usize,
    pub handoffs_done:    usize,
    pub clipboard_syncs:  usize,
}

impl CrossDeviceBus {
    pub const fn new() -> Self {
        Self {
            this_device:     None,
            devices:         BTreeMap::new(),
            sessions:        Vec::new(),
            clipboard:       ClipboardContent::Empty,
            events:          Vec::new(),
            next_session_id: 1,
            stats:           CrossDeviceStats {
                devices_seen: 0,
                sessions_created: 0,
                handoffs_done: 0,
                clipboard_syncs: 0,
            },
        }
    }

    /// Inicializa com o dispositivo local
    pub fn init(&mut self, node_id: [u8; 32], tick: u64) {
        let dev = DeviceInfo::this_device(node_id, tick);
        crate::serial_println!("[XDEV] Dispositivo local: '{}' [{}] {}",
            dev.name, dev.kind.as_str(), dev.node_id_short());
        self.this_device = Some(dev.clone());
        self.devices.insert(node_id, dev);
    }

    /// Regista um dispositivo remoto no cluster
    pub fn register_device(&mut self, info: DeviceInfo) {
        let short = info.node_id_short();
        crate::serial_println!("[XDEV] Dispositivo registado: '{}' [{}] {}",
            info.name, info.kind.as_str(), short);
        self.devices.insert(info.node_id, info);
        self.stats.devices_seen += 1;
    }

    /// Cria uma nova sessão transferível
    pub fn create_session(&mut self, app: &str, payload: Vec<u8>,
                          tick: u64) -> u64 {
        let node_id = self.this_device.as_ref()
            .map(|d| d.node_id).unwrap_or([0u8; 32]);
        let id = self.next_session_id;
        self.next_session_id += 1;
        let session = CrossSession::new(id, app, node_id, payload, tick);
        crate::serial_println!("[XDEV] Sessao criada: '{}' id={}", app, id);
        self.sessions.push(session);
        self.stats.sessions_created += 1;
        id
    }

    /// Inicia transferência de sessão para outro dispositivo
    pub fn handoff(&mut self, session_id: u64,
                   target: [u8; 32], tick: u64) -> Result<(), HandoffError> {
        let session = self.sessions.iter_mut()
            .find(|s| s.id == session_id)
            .ok_or(HandoffError::SessionNotFound)?;

        if !self.devices.contains_key(&target) {
            return Err(HandoffError::DeviceNotFound);
        }

        let from = session.current;
        session.state = SessionState::Transferring;
        session.current = target;
        session.transferred_at = Some(tick);

        // Persiste no DAG para garantia de entrega
        let dag_key = alloc::format!("/sessions/{}", session_id);
        crate::p2p::dag::write(&dag_key, session.payload.clone());

        self.events.push(PresenceEvent::SessionHandoff {
            session_id,
            from,
            to: target,
        });
        self.stats.handoffs_done += 1;

        let target_name = self.devices.get(&target)
            .map(|d| d.name.as_str()).unwrap_or("?");
        crate::serial_println!("[XDEV] Handoff sessao {} → '{}'", session_id, target_name);
        session.state = SessionState::Resumed;
        Ok(())
    }

    /// Copia algo para o clipboard distribuído
    pub fn clipboard_copy(&mut self, content: ClipboardContent) {
        let node_id = self.this_device.as_ref()
            .map(|d| d.node_id).unwrap_or([0u8; 32]);
        crate::serial_println!("[XDEV] Clipboard: {} copiado — sync P2P",
            content.as_str());
        // Persiste no DAG para sync automático
        let data = match &content {
            ClipboardContent::Text(s)  => s.as_bytes().to_vec(),
            ClipboardContent::Bytes(b) => b.clone(),
            ClipboardContent::File{ path, .. } => path.as_bytes().to_vec(),
            ClipboardContent::Empty    => Vec::new(),
        };
        crate::p2p::dag::write("/clipboard/latest", data);
        self.clipboard = content;
        self.events.push(PresenceEvent::ClipboardSync { from: node_id });
        self.stats.clipboard_syncs += 1;
    }

    /// Lista dispositivos online
    pub fn online_devices(&self) -> Vec<&DeviceInfo> {
        self.devices.values().filter(|d| d.online).collect()
    }

    /// Simula descoberta de dispositivos do cluster
    pub fn simulate_cluster(&mut self, tick: u64) {
        let peers = [
            ([0x01u8, 0x0a, 0x00, 0x00, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0],
             "socd-phone",  DeviceKind::Mobile,    [10,0,2,16u8]),
            ([0x02u8, 0x0b, 0x00, 0x00, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0],
             "socd-tablet", DeviceKind::Tablet,    [10,0,2,17u8]),
            ([0x03u8, 0x0c, 0x00, 0x00, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0],
             "socd-ar",     DeviceKind::ArGlasses, [10,0,2,18u8]),
            ([0x04u8, 0x0d, 0x00, 0x00, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0,
              0, 0, 0, 0, 0, 0, 0, 0],
             "socd-server", DeviceKind::Server,    [10,0,2,20u8]),
        ];

        for (nid, name, kind, ip) in peers {
            let res = kind.default_resolution();
            self.register_device(DeviceInfo {
                node_id:     nid,
                name:        name.to_string(),
                kind,
                online:      true,
                last_seen:   tick,
                battery:     Some(85),
                cpu_load:    10,
                ram_free_mb: 512,
                local_ip:    ip,
            });
        }
    }
}

#[derive(Debug)]
pub enum HandoffError {
    SessionNotFound,
    DeviceNotFound,
    TransferFailed,
}

// ─── Instância Global ─────────────────────────────────────────

pub static XDEV: Spinlock<CrossDeviceBus> =
    Spinlock::new(CrossDeviceBus::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    let node_id = crate::p2p::P2P_STATE.lock().node_id;
    let tick    = crate::modules::scheduler::get_stats().current_tick;
    XDEV.lock().init(node_id, tick);
    crate::serial_println!("[XDEV] Barramento cross-device ativo");
}

pub fn register_device(info: DeviceInfo) {
    XDEV.lock().register_device(info);
}

pub fn create_session(app: &str, payload: Vec<u8>) -> u64 {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    XDEV.lock().create_session(app, payload, tick)
}

pub fn handoff(session_id: u64, target: [u8; 32]) -> Result<(), HandoffError> {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    XDEV.lock().handoff(session_id, target, tick)
}

pub fn clipboard_copy(content: ClipboardContent) {
    XDEV.lock().clipboard_copy(content);
}

pub fn online_devices() -> Vec<DeviceInfo> {
    XDEV.lock().online_devices().into_iter().cloned().collect()
}

pub fn stats() -> CrossDeviceStats {
    XDEV.lock().stats.clone()
}

// ─── Demonstração Fase 3.4 ───────────────────────────────────

pub fn run_demo() {
    crate::serial_println!("\n[FASE3.4] === Interface Cross-Device ===");

    let tick = crate::modules::scheduler::get_stats().current_tick;

    // Simula cluster de 4 dispositivos
    XDEV.lock().simulate_cluster(tick);

    let devices = online_devices();
    crate::serial_println!("[FASE3.4] Cluster: {} dispositivos online", devices.len());
    for d in &devices {
        let (rx, ry) = d.kind.default_resolution();
        crate::serial_println!("[FASE3.4]   '{}' [{}] {}x{} ip={}.{}.{}.{}",
            d.name, d.kind.as_str(), rx, ry,
            d.local_ip[0], d.local_ip[1], d.local_ip[2], d.local_ip[3]);
    }

    // Cria sessão de editor de texto no PC
    let payload = b"estado-do-editor: linha=42 col=10 ficheiro=/home/doc.txt".to_vec();
    let sid = create_session("text-editor", payload);
    crate::serial_println!("[FASE3.4] Sessao '{}' criada id={}", "text-editor", sid);

    // Handoff para o mobile (continuar no telemóvel)
    let mobile_id = [0x01u8, 0x0a, 0x00, 0x00, 0, 0, 0, 0,
                     0, 0, 0, 0, 0, 0, 0, 0,
                     0, 0, 0, 0, 0, 0, 0, 0,
                     0, 0, 0, 0, 0, 0, 0, 0];
    match handoff(sid, mobile_id) {
        Ok(()) => { crate::serial_println!("[FASE3.4] Handoff OK - sessao continua no mobile"); }
        Err(_) => { crate::serial_println!("[FASE3.4] Handoff simulado (sem transport real)"); }
    }

    // Clipboard distribuído
    clipboard_copy(ClipboardContent::Text(
        "SOC-D — Sistema Operacional Cognitivo Distribuido".to_string()
    ));
    crate::serial_println!("[FASE3.4] Clipboard sincronizado via DAG P2P");

    let s = stats();
    crate::serial_println!("[FASE3.4] Stats: {} dispositivos | {} sessoes | {} handoffs | {} clipboard",
        s.devices_seen, s.sessions_created, s.handoffs_done, s.clipboard_syncs);

    crate::serial_println!("[FASE3.4] Use 'devices' no shell para ver cluster");
    crate::serial_println!("[FASE3.4] Use 'handoff <sid> <dev>' para transferir sessao");
    crate::serial_println!("[FASE3.4] ======================================\n");
}
