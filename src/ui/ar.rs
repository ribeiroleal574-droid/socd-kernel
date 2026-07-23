// ============================================================
// SOC-D Kernel — Interface Holográfica AR (Fase 4.2)
// ============================================================
//
// Sistema de UI 3D para óculos AR/VR integrado com OpenXR.
// Elementos holográficos ancorados no espaço real ou virtual.
//
// Conceitos:
//   - Anchor: ponto fixo no espaço 3D onde um elemento está preso
//   - Hologram: widget 3D com posição/rotação/escala no mundo
//   - Spatial UI: painéis flutuantes com transparência
//   - Gaze Input: interação por olhar (eye tracking)
//   - Hand Tracking: gestos de mão para interação
//
// Arquitectura:
//
//   SpatialScene
//   ├── Anchors[]     (pontos no espaço real)
//   │   └── Hologram  (widget 3D preso ao anchor)
//   ├── SpatialPanels[] (painéis flutuantes)
//   └── GazeTarget    (elemento focado pelo olhar)
//
// Pipeline AR:
//   1. OpenXR begin_frame() → obter pose da cabeça
//   2. Para cada hologram: calcular posição relativa à câmara
//   3. Renderizar com transparência adaptativa
//   4. Gaze tracking → highlight do elemento focado
//   5. Hand gesture → activar elemento focado
//   6. OpenXR end_frame() → submeter layers
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;
use crate::xr::{Vec3f, Quaternionf, XrPose};

// ─── Vector 3D helpers ───────────────────────────────────────

fn vec3(x: f32, y: f32, z: f32) -> Vec3f { Vec3f { x, y, z } }
fn quat_identity() -> Quaternionf {
    Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
}
fn pose(x: f32, y: f32, z: f32) -> XrPose {
    XrPose {
        position:    vec3(x, y, z),
        orientation: quat_identity(),
    }
}

// ─── Anchor Espacial ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SpatialAnchor {
    pub id:       u64,
    pub name:     String,
    /// Posição no espaço do mundo (metros)
    pub pose:     XrPose,
    /// Persiste entre sessões (via DAG)
    pub persistent: bool,
    /// Confiança da localização (0.0–1.0)
    pub confidence: f32,
}

impl SpatialAnchor {
    pub fn new(id: u64, name: &str, x: f32, y: f32, z: f32, persistent: bool) -> Self {
        Self {
            id,
            name: name.to_string(),
            pose: pose(x, y, z),
            persistent,
            confidence: 1.0,
        }
    }
}

// ─── Hologram (widget 3D) ────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HologramContent {
    /// Painel 2D flutuante com texto
    Panel { width: f32, height: f32, title: String, body: String },
    /// Ícone 3D com label
    Icon  { symbol: char, label: String, size: f32 },
    /// Indicador de estado (barra de progresso 3D)
    Gauge { value: f32, max: f32, label: String },
    /// Seta de navegação (para AR wayfinding)
    Arrow { direction: Vec3f, label: String },
    /// Notificação flutuante
    Toast { message: String, level: ToastLevel, duration_ticks: u64 },
    /// Dashboard de métricas (painel informativo)
    Dashboard { title: String, metrics: Vec<(String, String)> },
}

#[derive(Debug, Clone)]
pub enum ToastLevel { Info, Success, Warning, Error }

impl ToastLevel {
    pub fn color(&self) -> u32 {
        match self {
            ToastLevel::Info    => 0xCC0088FF,
            ToastLevel::Success => 0xCC00FF88,
            ToastLevel::Warning => 0xCCFFAA00,
            ToastLevel::Error   => 0xCCFF3333,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hologram {
    pub id:          u64,
    pub content:     HologramContent,
    /// Pose relativa ao anchor (ou ao utilizador se anchor=None)
    pub local_pose:  XrPose,
    /// Escala (1.0 = tamanho real)
    pub scale:       f32,
    /// Opacidade (0.0–1.0)
    pub opacity:     f32,
    /// Anchor a que está preso (None = world-locked)
    pub anchor_id:   Option<u64>,
    /// Visível?
    pub visible:     bool,
    /// Tick de criação
    pub created_at:  u64,
    /// Tick de expiração (None = permanente)
    pub expires_at:  Option<u64>,
    /// Está focado pelo olhar (gaze)?
    pub gaze_focused: bool,
}

impl Hologram {
    pub fn new(id: u64, content: HologramContent, x: f32, y: f32, z: f32,
               anchor_id: Option<u64>, tick: u64) -> Self {
        Self {
            id, content,
            local_pose: pose(x, y, z),
            scale: 1.0,
            opacity: 0.9,
            anchor_id,
            visible: true,
            created_at: tick,
            expires_at: None,
            gaze_focused: false,
        }
    }

    pub fn with_opacity(mut self, o: f32) -> Self { self.opacity = o; self }
    pub fn with_scale(mut self, s: f32) -> Self   { self.scale = s; self }
    pub fn expires_in(mut self, ticks: u64, now: u64) -> Self {
        self.expires_at = Some(now + ticks);
        self
    }
}

// ─── Gaze Input ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GazeState {
    /// Direção do olhar no espaço do mundo
    pub direction: Vec3f,
    /// Ponto de fixação estimado
    pub focus_point: Option<Vec3f>,
    /// ID do hologram focado
    pub focused_hologram: Option<u64>,
    /// Quanto tempo está focado (ticks)
    pub dwell_ticks: u64,
    /// Threshold para "dwell click" (ticks)
    pub dwell_threshold: u64,
}

impl GazeState {
    pub const fn new() -> Self {
        Self {
            direction: Vec3f { x: 0.0, y: 0.0, z: -1.0 },
            focus_point: None,
            focused_hologram: None,
            dwell_ticks: 0,
            dwell_threshold: 90, // ~1.5 segundos a 60Hz
        }
    }

    pub fn is_dwell_complete(&self) -> bool {
        self.focused_hologram.is_some() && self.dwell_ticks >= self.dwell_threshold
    }
}

// ─── Hand Gesture (AR) ───────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HandGesture {
    Pinch   { hand: Hand, strength: f32 },
    Grab    { hand: Hand },
    Release { hand: Hand },
    Point   { hand: Hand, direction: Vec3f },
    OpenPalm { hand: Hand },
    ThumbsUp,
    Ok,
}

#[derive(Debug, Clone)]
pub enum Hand { Left, Right }

impl HandGesture {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandGesture::Pinch{..}    => "pinch",
            HandGesture::Grab{..}     => "grab",
            HandGesture::Release{..}  => "release",
            HandGesture::Point{..}    => "point",
            HandGesture::OpenPalm{..} => "open-palm",
            HandGesture::ThumbsUp     => "thumbs-up",
            HandGesture::Ok           => "ok",
        }
    }
}

// ─── Cena Espacial ───────────────────────────────────────────

pub struct SpatialScene {
    pub anchors:    Vec<SpatialAnchor>,
    pub holograms:  Vec<Hologram>,
    pub gaze:       GazeState,
    pub hand_gestures: Vec<HandGesture>,
    next_id:        u64,
    pub stats:      SpatialStats,
}

#[derive(Debug, Clone, Default)]
pub struct SpatialStats {
    pub anchors_created:    usize,
    pub holograms_active:   usize,
    pub gaze_activations:   usize,
    pub gestures_processed: usize,
    pub frames_rendered:    u64,
}

impl SpatialScene {
    pub const fn new() -> Self {
        Self {
            anchors:       Vec::new(),
            holograms:     Vec::new(),
            gaze:          GazeState::new(),
            hand_gestures: Vec::new(),
            next_id:       1,
            stats:         SpatialStats {
                anchors_created: 0,
                holograms_active: 0,
                gaze_activations: 0,
                gestures_processed: 0,
                frames_rendered: 0,
            },
        }
    }

    pub fn create_anchor(&mut self, name: &str, x: f32, y: f32, z: f32,
                         persistent: bool) -> u64 {
        let id = self.next_id();
        let anchor = SpatialAnchor::new(id, name, x, y, z, persistent);
        crate::serial_println!("[AR] Anchor '{}' criado id={} pos=({:.1},{:.1},{:.1})",
            name, id, x, y, z);
        if persistent {
            // Persiste via DAG para restaurar em sessões futuras
            let key = alloc::format!("/ar/anchors/{}", id);
            let data = alloc::format!("{},{},{}", x, y, z);
            crate::p2p::dag::write(&key, data.into_bytes());
        }
        self.anchors.push(anchor);
        self.stats.anchors_created += 1;
        id
    }

    pub fn add_hologram(&mut self, content: HologramContent,
                        x: f32, y: f32, z: f32,
                        anchor_id: Option<u64>, tick: u64) -> u64 {
        let id = self.next_id();
        let h = Hologram::new(id, content, x, y, z, anchor_id, tick);
        crate::serial_println!("[AR] Hologram id={} criado pos=({:.1},{:.1},{:.1})",
            id, x, y, z);
        self.holograms.push(h);
        self.stats.holograms_active += 1;
        id
    }

    pub fn remove_hologram(&mut self, id: u64) {
        self.holograms.retain(|h| h.id != id);
        self.stats.holograms_active = self.holograms.len();
    }

    /// Atualiza gaze e verifica dwell clicks
    pub fn update_gaze(&mut self, dir: Vec3f) -> Option<u64> {
        self.gaze.direction = dir;
        // Raycasting simplificado: assume que o hologram mais próximo
        // na direção do olhar está focado
        let mut closest: Option<(u64, f32)> = None;
        for h in &self.holograms {
            if !h.visible { continue; }
            let p = &h.local_pose.position;
            // Produto escalar simplificado para verificar se está "na direção"
            let dot = dir.x * p.x + dir.y * p.y + dir.z * p.z;
            if dot > 0.8 {
                let dist = libm::sqrtf(p.x*p.x + p.y*p.y + p.z*p.z);
                if closest.is_none() || dist < closest.unwrap().1 {
                    closest = Some((h.id, dist));
                }
            }
        }
        // Atualiza foco
        for h in &mut self.holograms { h.gaze_focused = false; }
        if let Some((focused_id, _)) = closest {
            if self.gaze.focused_hologram == Some(focused_id) {
                self.gaze.dwell_ticks += 1;
            } else {
                self.gaze.focused_hologram = Some(focused_id);
                self.gaze.dwell_ticks = 0;
            }
            if let Some(h) = self.holograms.iter_mut().find(|h| h.id == focused_id) {
                h.gaze_focused = true;
            }
            // Dwell click
            if self.gaze.is_dwell_complete() {
                self.gaze.dwell_ticks = 0;
                self.stats.gaze_activations += 1;
                crate::serial_println!("[AR] Dwell click — hologram id={}", focused_id);
                return Some(focused_id);
            }
        } else {
            self.gaze.focused_hologram = None;
            self.gaze.dwell_ticks = 0;
        }
        None
    }

    /// Processa um gesto de mão
    pub fn handle_hand_gesture(&mut self, g: HandGesture) {
        crate::serial_println!("[AR] Gesto de mao: {}", g.as_str());
        self.hand_gestures.push(g);
        if self.hand_gestures.len() > 16 { self.hand_gestures.remove(0); }
        self.stats.gestures_processed += 1;
    }

    /// Tick de renderização AR
    pub fn render_tick(&mut self, tick: u64) {
        // Remove holograms expirados
        let before = self.holograms.len();
        self.holograms.retain(|h| {
            h.expires_at.map(|e| tick < e).unwrap_or(true)
        });
        let removed = before - self.holograms.len();
        if removed > 0 {
            self.stats.holograms_active = self.holograms.len();
        }
        self.stats.frames_rendered += 1;
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static SPATIAL: Spinlock<SpatialScene> =
    Spinlock::new(SpatialScene::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[AR] Interface holografica inicializada");
    crate::serial_println!("[AR] OpenXR 1832x1920@90Hz | Gaze | Hand tracking");
}

pub fn create_anchor(name: &str, x: f32, y: f32, z: f32, persistent: bool) -> u64 {
    SPATIAL.lock().create_anchor(name, x, y, z, persistent)
}

pub fn show_panel(title: &str, body: &str, x: f32, y: f32, z: f32,
                  anchor_id: Option<u64>) -> u64 {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    SPATIAL.lock().add_hologram(
        HologramContent::Panel {
            width: 0.4, height: 0.3,
            title: title.to_string(),
            body:  body.to_string(),
        },
        x, y, z, anchor_id, tick,
    )
}

pub fn show_toast(msg: &str, level: ToastLevel, duration_ticks: u64) -> u64 {
    let tick = crate::modules::scheduler::get_stats().current_tick;
    let mut scene = SPATIAL.lock();
    let id = scene.add_hologram(
        HologramContent::Toast {
            message: msg.to_string(),
            level,
            duration_ticks,
        },
        0.0, 0.1, -0.5, // ligeiramente abaixo e à frente
        None, tick,
    );
    if let Some(h) = scene.holograms.iter_mut().find(|h| h.id == id) {
        h.expires_at = Some(tick + duration_ticks);
    }
    id
}

pub fn show_dashboard(tick: u64) -> u64 {
    let s = crate::modules::scheduler::get_stats();
    let dag_s = crate::p2p::dag::stats();
    let metrics = alloc::vec![
        ("CPU tick".to_string(),    alloc::format!("{}", s.current_tick)),
        ("Processos".to_string(),   alloc::format!("{}", s.ready_in_queues)),
        ("DAG blocos".to_string(),  alloc::format!("{}", dag_s.total_blocks)),
        ("Dispositivos".to_string(),
            alloc::format!("{}", crate::modules::xdev::online_devices().len())),
    ];
    SPATIAL.lock().add_hologram(
        HologramContent::Dashboard {
            title: "SOC-D Status".to_string(),
            metrics,
        },
        0.0, 0.0, -0.8,
        None, tick,
    )
}

pub fn handle_hand_gesture(g: HandGesture) {
    SPATIAL.lock().handle_hand_gesture(g);
}

pub fn ar_tick(tick: u64) {
    SPATIAL.lock().render_tick(tick);
}

pub fn stats() -> SpatialStats {
    SPATIAL.lock().stats.clone()
}

// ─── Demonstração Fase 4.2 ───────────────────────────────────

pub fn run_demo() {
    crate::serial_println!("\n[FASE4.2] === Interface Holografica AR ===");

    let tick = crate::modules::scheduler::get_stats().current_tick;

    // Cria anchors no espaço
    let desk_anchor = create_anchor("secretaria", 0.0, -0.5, -1.0, true);
    let wall_anchor = create_anchor("parede-norte", 0.0,  0.0, -2.0, true);

    // Painéis holográficos flutuantes
    show_panel("SOC-D Dashboard",
        "Kernel v0.1.0 | 77 modulos | P2P ativo",
        0.0, 0.1, -0.8, None);

    show_panel("Agenda",
        "09:00 Reuniao P2P\n10:30 Review AR\n14:00 Deploy",
        0.3, 0.0, -0.9, Some(wall_anchor));

    show_panel("Ficheiros Recentes",
        "/home/doc.txt (sync)\n/sys/hostname\n/clipboard",
        -0.3, 0.0, -0.9, Some(wall_anchor));

    // Dashboard de métricas em AR
    show_dashboard(tick);

    // Toast de boas-vindas
    show_toast("Bem-vindo ao SOC-D AR!", ToastLevel::Success, 300);

    // Simula gaze input
    {
        let mut scene = SPATIAL.lock();
        scene.update_gaze(vec3(0.0, 0.1, -1.0));
    }

    // Simula gestos de mão
    handle_hand_gesture(HandGesture::Pinch {
        hand: Hand::Right, strength: 0.9
    });
    handle_hand_gesture(HandGesture::Point {
        hand: Hand::Right,
        direction: vec3(0.3, 0.0, -0.9),
    });

    let s = stats();
    crate::serial_println!("[FASE4.2] Anchors: {} | Holograms: {} | Gestos: {}",
        s.anchors_created, s.holograms_active, s.gestures_processed);
    crate::serial_println!("[FASE4.2] Use 'ar' no shell para estado da cena");
    crate::serial_println!("[FASE4.2] =====================================\n");
}
