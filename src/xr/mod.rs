extern crate alloc;
extern crate libm;
// ============================================================
// SOC-D Kernel — OpenXR Runtime (AR/VR) — Fase 4
// ============================================================
//
// O módulo XR implementa o runtime OpenXR do SOC-D,
// permitindo que aplicações renderizem em dispositivos
// de Realidade Aumentada e Virtual.
//
// OpenXR é o padrão Khronos para XR:
//   - API unificada para Meta Quest, HoloLens, Valve Index, etc.
//   - Gerencia sessões XR, espaços de referência e poses
//   - Abstrai os diferentes SDKs de hardware
//
// Pipeline XR do SOC-D:
//   1. XrInstance  — inicialização do runtime
//   2. XrSystem    — hardware XR disponível
//   3. XrSession   — sessão de renderização ativa
//   4. XrSwapchain — buffers de imagem para os olhos
//   5. Frame loop  — begin_frame → render → end_frame
//   6. Poses       — posição/orientação do HMD e controllers
//
// Espaços de referência:
//   Stage   — coordenadas do mundo real (chão = origem)
//   Local   — relativo à posição de início da sessão
//   View    — relativo à câmera/HMD (eye space)
//   Grip    — relativo ao controller
//
// Fase 4 (atual):
//   - Estruturas e API completa OpenXR-compatible
//   - Sessão simulada (sem hardware real)
//   - Poses sintéticas (testes e desenvolvimento)
//   - Sistema de overlay AR (projeção sobre câmera)
//
// Fase 5: Driver real para hardware XR via virtio-xr
// ============================================================

use alloc::{string::{String, ToString}, vec::Vec};
use spinning_top::Spinlock;

// ─── Tipos Matemáticos XR ────────────────────────────────────────────────────

/// Quaternion para representar rotações (x, y, z, w)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternionf {
    pub x: f32, pub y: f32, pub z: f32, pub w: f32,
}

impl Quaternionf {
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub fn from_axis_angle(ax: f32, ay: f32, az: f32, angle_rad: f32) -> Self {
        let s = libm::sinf(angle_rad / 2.0);
        let c = libm::cosf(angle_rad / 2.0);
        let len = libm::sqrtf(ax*ax + ay*ay + az*az).max(0.0001);
        Self { x: ax/len*s, y: ay/len*s, z: az/len*s, w: c }
    }

    pub fn magnitude(&self) -> f32 {
        libm::sqrtf(self.x*self.x + self.y*self.y + self.z*self.z + self.w*self.w)
    }

    pub fn normalize(&self) -> Self {
        let m = self.magnitude().max(0.0001);
        Self { x: self.x/m, y: self.y/m, z: self.z/m, w: self.w/m }
    }

    /// Multiplica dois quaternions (composição de rotações)
    pub fn mul(&self, rhs: &Self) -> Self {
        Self {
            x: self.w*rhs.x + self.x*rhs.w + self.y*rhs.z - self.z*rhs.y,
            y: self.w*rhs.y - self.x*rhs.z + self.y*rhs.w + self.z*rhs.x,
            z: self.w*rhs.z + self.x*rhs.y - self.y*rhs.x + self.z*rhs.w,
            w: self.w*rhs.w - self.x*rhs.x - self.y*rhs.y - self.z*rhs.z,
        }.normalize()
    }
}

/// Vetor 3D
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3f { pub x: f32, pub y: f32, pub z: f32 }

impl Vec3f {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }
    pub fn length(&self) -> f32 { libm::sqrtf(self.x*self.x + self.y*self.y + self.z*self.z) }
    pub fn add(&self, o: &Self) -> Self { Self::new(self.x+o.x, self.y+o.y, self.z+o.z) }
    pub fn scale(&self, s: f32) -> Self { Self::new(self.x*s, self.y*s, self.z*s) }
}

/// Pose no espaço 3D (posição + orientação)
#[derive(Debug, Clone, Copy)]
pub struct XrPose {
    pub position:    Vec3f,
    pub orientation: Quaternionf,
}

impl XrPose {
    pub const IDENTITY: Self = Self {
        position: Vec3f::ZERO,
        orientation: Quaternionf::IDENTITY,
    };
}

/// Campo de visão (frustum) para um olho
#[derive(Debug, Clone, Copy)]
pub struct XrFov {
    pub angle_left:  f32,  // Radianos
    pub angle_right: f32,
    pub angle_up:    f32,
    pub angle_down:  f32,
}

impl XrFov {
    /// FOV típico de HMD: 90° horizontal, 90° vertical
    pub fn hmd_default() -> Self {
        let half = core::f32::consts::PI / 4.0; // 45° = π/4
        Self {
            angle_left:  -half,
            angle_right:  half,
            angle_up:     half,
            angle_down:  -half,
        }
    }

    /// FOV de passthrough AR (câmera do dispositivo)
    pub fn ar_passthrough() -> Self {
        let h = 0.6; // ~69° horizontal
        let v = 0.45;
        Self { angle_left: -h, angle_right: h, angle_up: v, angle_down: -v }
    }
}

// ─── Handles OpenXR ──────────────────────────────────────────────────────────

pub type XrHandle = u64;

/// Estado da sessão XR
#[derive(Debug, Clone, PartialEq)]
pub enum XrSessionState {
    Idle,
    Ready,
    Synchronized,
    Visible,
    Focused,
    Stopping,
    LossPending,
    Exiting,
}

/// Tipo de sistema XR
#[derive(Debug, Clone, PartialEq)]
pub enum XrSystemType {
    /// Headset VR (Meta Quest, Valve Index)
    HeadMountedVR,
    /// Óculos AR (HoloLens, Magic Leap)
    AugmentedReality,
    /// Passthrough (câmera com overlay)
    Passthrough,
    /// Simulado (desenvolvimento/testes)
    Simulated,
}

/// Informações do sistema XR disponível
#[derive(Debug, Clone)]
pub struct XrSystemInfo {
    pub system_id:   XrHandle,
    pub system_type: XrSystemType,
    pub name:        String,
    /// Resolução por olho
    pub eye_width:   u32,
    pub eye_height:  u32,
    /// Taxa de refresh em Hz
    pub refresh_rate: f32,
    /// Suporta mão esquerda?
    pub has_left_controller:  bool,
    /// Suporta mão direita?
    pub has_right_controller: bool,
    /// Suporta rastreamento de mãos?
    pub has_hand_tracking: bool,
    /// Suporta eye-tracking?
    pub has_eye_tracking: bool,
    /// Suporta passthrough?
    pub has_passthrough: bool,
}

/// Estado de um controller XR
#[derive(Debug, Clone, Default)]
pub struct XrControllerState {
    pub pose:       XrPose,
    pub grip:       f32,   // 0–1
    pub trigger:    f32,   // 0–1
    pub thumbstick: (f32, f32), // X, Y
    pub button_a:   bool,
    pub button_b:   bool,
    pub menu:       bool,
}

impl Default for XrPose {
    fn default() -> Self { Self::IDENTITY }
}

// ─── Frame XR ────────────────────────────────────────────────────────────────

/// View (câmera) de um olho
#[derive(Debug, Clone, Copy)]
pub struct XrView {
    pub pose: XrPose,
    pub fov:  XrFov,
}

/// Estado de um frame XR
#[derive(Debug, Clone)]
pub struct XrFrameState {
    /// Timestamp do frame em nanosegundos
    pub predicted_display_time: u64,
    /// O frame deve ser renderizado?
    pub should_render: bool,
    /// Views para cada olho [left, right]
    pub views: [XrView; 2],
    /// Pose do HMD neste frame
    pub hmd_pose: XrPose,
    /// State dos controllers
    pub left_controller:  XrControllerState,
    pub right_controller: XrControllerState,
}

/// Resultado da submissão de um layer de composição
#[derive(Debug, Clone, PartialEq)]
pub enum XrEndFrameResult {
    Success,
    LayerRejected,
    SessionLost,
}

// ─── Runtime XR ──────────────────────────────────────────────────────────────

pub struct XrRuntime {
    pub initialized:    bool,
    pub session_state:  XrSessionState,
    pub system:         Option<XrSystemInfo>,
    pub frame_count:    u64,
    pub session_handle: XrHandle,
    /// Pose sintética do HMD (para simulação)
    sim_hmd_yaw:   f32,
    sim_hmd_pitch: f32,
    sim_hmd_pos:   Vec3f,
}

impl XrRuntime {
    const fn new() -> Self {
        Self {
            initialized:    false,
            session_state:  XrSessionState::Idle,
            system:         None,
            frame_count:    0,
            session_handle: 0,
            sim_hmd_yaw:    0.0,
            sim_hmd_pitch:  0.0,
            sim_hmd_pos:    Vec3f::ZERO,
        }
    }

    /// Cria instância XR e detecta hardware disponível
    pub fn create_instance(&mut self) -> XrHandle {
        self.initialized = true;
        1001 // Handle da instância
    }

    /// Obtém o sistema XR disponível (HMD ou AR)
    pub fn get_system(&mut self, instance: XrHandle) -> Option<XrHandle> {
        if instance == 0 { return None; }

        // Em hardware real: enumeraria dispositivos via /dev/xr ou vendor SDK
        // Por agora: sempre retorna um sistema simulado
        self.system = Some(XrSystemInfo {
            system_id:   2001,
            system_type: XrSystemType::Simulated,
            name:        "SOC-D XR Simulator v1.0".into(),
            eye_width:   1832,  // Meta Quest 2 resolution
            eye_height:  1920,
            refresh_rate: 90.0,
            has_left_controller:  true,
            has_right_controller: true,
            has_hand_tracking:    false,
            has_eye_tracking:     false,
            has_passthrough:      true,
        });

        Some(2001)
    }

    /// Cria uma sessão XR
    pub fn create_session(&mut self, _system_id: XrHandle) -> XrHandle {
        self.session_handle = 3001;
        self.session_state  = XrSessionState::Ready;
        crate::serial_println!("[XR] Sessao criada (handle={})", self.session_handle);
        3001
    }

    /// Begin frame — retorna estado do frame a renderizar
    pub fn begin_frame(&mut self, tick: u64) -> XrFrameState {
        self.session_state = XrSessionState::Focused;

        // Simula movimento do HMD (oscila suavemente)
        let t = tick as f32 * 0.001;
        self.sim_hmd_yaw   = libm::sinf(t * 0.3) * 0.2;  // ±11° de yaw
        self.sim_hmd_pitch = libm::sinf(t * 0.15) * 0.1; // ±5.7° de pitch
        self.sim_hmd_pos.y = 1.65 + libm::sinf(t * 0.5) * 0.02; // Altura ~1.65m

        let hmd_rotation = Quaternionf::from_axis_angle(0.0, 1.0, 0.0, self.sim_hmd_yaw)
            .mul(&Quaternionf::from_axis_angle(1.0, 0.0, 0.0, self.sim_hmd_pitch));

        let hmd_pose = XrPose {
            position: self.sim_hmd_pos,
            orientation: hmd_rotation,
        };

        let eye_separation = 0.063; // 63mm IPD típico
        let fov = XrFov::hmd_default();

        XrFrameState {
            predicted_display_time: tick * 1_000_000, // ns
            should_render: self.session_state == XrSessionState::Focused,
            views: [
                XrView {
                    pose: XrPose {
                        position: Vec3f::new(
                            hmd_pose.position.x - eye_separation / 2.0,
                            hmd_pose.position.y,
                            hmd_pose.position.z,
                        ),
                        orientation: hmd_rotation,
                    },
                    fov,
                },
                XrView {
                    pose: XrPose {
                        position: Vec3f::new(
                            hmd_pose.position.x + eye_separation / 2.0,
                            hmd_pose.position.y,
                            hmd_pose.position.z,
                        ),
                        orientation: hmd_rotation,
                    },
                    fov,
                },
            ],
            hmd_pose,
            left_controller:  XrControllerState {
                pose: XrPose {
                    position: Vec3f::new(-0.25, 1.2, -0.4),
                    orientation: Quaternionf::IDENTITY,
                },
                ..Default::default()
            },
            right_controller: XrControllerState {
                pose: XrPose {
                    position: Vec3f::new(0.25, 1.2, -0.4),
                    orientation: Quaternionf::IDENTITY,
                },
                ..Default::default()
            },
        }
    }

    /// End frame — submete layers renderizados ao compositor XR
    pub fn end_frame(&mut self, _frame: &XrFrameState) -> XrEndFrameResult {
        self.frame_count += 1;
        XrEndFrameResult::Success
    }

    /// Destrói a sessão XR
    pub fn destroy_session(&mut self) {
        self.session_state  = XrSessionState::Exiting;
        self.session_handle = 0;
        crate::serial_println!("[XR] Sessao destruida");
    }

    pub fn get_stats(&self) -> XrStats {
        XrStats {
            initialized: self.initialized,
            session_state: alloc::format!("{:?}", self.session_state),
            frame_count: self.frame_count,
            system_name: self.system.as_ref().map(|s| s.name.clone()),
            hmd_pos: self.sim_hmd_pos,
            hmd_yaw_deg: self.sim_hmd_yaw * 57.2957795f32,
        }
    }
}

#[derive(Debug, Clone)]
pub struct XrStats {
    pub initialized: bool,
    pub session_state: String,
    pub frame_count: u64,
    pub system_name: Option<String>,
    pub hmd_pos: Vec3f,
    pub hmd_yaw_deg: f32,
}

static XR_RUNTIME: Spinlock<XrRuntime> = Spinlock::new(XrRuntime::new());

pub fn init() {
    let mut rt = XR_RUNTIME.lock();
    let inst = rt.create_instance();
    if let Some(sys) = rt.get_system(inst) {
        let _sess = rt.create_session(sys);
        if let Some(ref info) = rt.system {
            crate::serial_println!("[XR] Runtime OpenXR inicializado");
            crate::serial_println!("[XR] Sistema: {}", info.name);
            crate::serial_println!("[XR] Resolucao: {}x{} por olho @ {}Hz",
                info.eye_width, info.eye_height, info.refresh_rate);
        }
    }
}

pub fn begin_frame(tick: u64) -> XrFrameState {
    XR_RUNTIME.lock().begin_frame(tick)
}

pub fn end_frame(frame: &XrFrameState) -> XrEndFrameResult {
    XR_RUNTIME.lock().end_frame(frame)
}

pub fn get_stats() -> XrStats {
    XR_RUNTIME.lock().get_stats()
}
