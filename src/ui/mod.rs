extern crate alloc;
// ============================================================
// SOC-D Kernel — Módulo de Interface Gráfica (Fase 3)
// ============================================================
//
// Implementa o stack gráfico completo do SOC-D:
//
//   ┌──────────────────────────────────────────────────┐
//   │              Apps (WASM / Native)                │
//   ├──────────────────────────────────────────────────┤
//   │         Shell SOC-D (desktop/mobile/AR)          │
//   ├──────────────────────────────────────────────────┤
//   │          Widgets & Layout Engine                 │
//   ├──────────────────────────────────────────────────┤
//   │        Compositor Wayland (superfícies)          │
//   ├──────────────────────────────────────────────────┤
//   │     Render Backend (Framebuffer / Vulkan)        │
//   ├──────────────────────────────────────────────────┤
//   │       Input (teclado, mouse, touch, AR)          │
//   └──────────────────────────────────────────────────┘
//
// Fase 3 (atual):
//   - Compositor de superfícies com z-ordering
//   - Framebuffer linear (320x240 bare metal)
//   - Shell desktop com taskbar e janelas
//   - Engine de widgets (botão, label, progress, input)
//   - Input unificado (teclado + mouse simulado)
//   - Layout: flexbox simplificado
//
// Fase 4:
//   - Vulkan via virtio-gpu
//   - OpenXR para AR/VR
//   - GPU acceleration
// ============================================================

pub mod compositor;  // Gerencia superfícies e z-ordering
pub mod shell;       // Desktop shell (taskbar, launchers)
pub mod widgets;     // Engine de widgets UI
pub mod input;       // Input unificado (teclado, mouse, touch)
pub mod render;      // Backend de renderização (framebuffer)
pub mod mobile;      // UI Mobile adaptativa (Fase 4.1)
pub mod ar;          // Interface holográfica AR (Fase 4.2)

use spinning_top::Spinlock;

/// Resolução padrão do framebuffer (bare metal)
pub const SCREEN_WIDTH:  u32 = 1024;
pub const SCREEN_HEIGHT: u32 = 768;

/// Profundidade de cor: 32bpp ARGB
pub const COLOR_DEPTH: u32 = 32;

/// Bytes por pixel
pub const BYTES_PER_PIXEL: u32 = 4;

/// Tamanho do framebuffer em bytes
pub const FRAMEBUFFER_SIZE: usize =
    (SCREEN_WIDTH * SCREEN_HEIGHT * BYTES_PER_PIXEL) as usize;

/// Estado global do subsistema de UI
pub static UI_STATE: Spinlock<UiState> = Spinlock::new(UiState::new());

pub struct UiState {
    pub initialized: bool,
    pub frames_rendered: u64,
    pub last_frame_tick: u64,
    pub fps_target: u32,
    pub active_windows: usize,
    pub focused_window: Option<u64>,
    pub mode: DisplayMode,
}

/// Modo de exibição atual
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayMode {
    /// Desktop tradicional (PC)
    Desktop,
    /// Interface mobile (touch-first)
    Mobile,
    /// Realidade aumentada (Fase 4)
    AR,
    /// Somente terminal (fallback)
    Terminal,
}

impl UiState {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            frames_rendered: 0,
            last_frame_tick: 0,
            fps_target: 60,
            active_windows: 0,
            focused_window: None,
            mode: DisplayMode::Desktop,
        }
    }
}

/// Inicializa todo o stack de UI
pub fn init() {
    render::init();
    input::init();
    compositor::init();
    widgets::init();
    shell::init();

    let mut state = UI_STATE.lock();
    state.initialized = true;
    state.mode = DisplayMode::Desktop;

    crate::serial_println!("[UI] Stack grafico inicializado ({}x{} {}bpp)",
        SCREEN_WIDTH, SCREEN_HEIGHT, COLOR_DEPTH);
}

/// Tick de renderização — chamado pelo timer a ~60fps
pub fn render_tick(current_tick: u64) {
    let fps_interval = 1000 / 60; // ~16ms entre frames a 60fps (1 tick = ~1ms)
    {
        let state = UI_STATE.lock();
        if !state.initialized { return; }
        let elapsed = current_tick.saturating_sub(state.last_frame_tick);
        if elapsed < fps_interval as u64 { return; }
    }

    // Pipeline de renderização
    compositor::composite_frame();
    render::present();

    let mut state = UI_STATE.lock();
    state.frames_rendered += 1;
    state.last_frame_tick = current_tick;
}

/// Cor ARGB empacotada em u32
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self::argb(255, r, g, b) }
    pub fn r(self) -> u8 { ((self.0 >> 16) & 0xFF) as u8 }
    pub fn g(self) -> u8 { ((self.0 >>  8) & 0xFF) as u8 }
    pub fn b(self) -> u8 { (self.0 & 0xFF) as u8 }
    pub fn a(self) -> u8 { ((self.0 >> 24) & 0xFF) as u8 }

    /// Blend alpha sobre fundo
    pub fn blend_over(self, bg: Color) -> Color {
        let alpha = self.a() as u32;
        let inv   = 255 - alpha;
        Color::rgb(
            ((self.r() as u32 * alpha + bg.r() as u32 * inv) / 255) as u8,
            ((self.g() as u32 * alpha + bg.g() as u32 * inv) / 255) as u8,
            ((self.b() as u32 * alpha + bg.b() as u32 * inv) / 255) as u8,
        )
    }
}

// ─── Paleta de Cores SOC-D ────────────────────────────────────────────────────
pub mod palette {
    use super::Color;
    pub const BACKGROUND:    Color = Color::rgb(  5,  10,  20); // #050A14
    pub const SURFACE:       Color = Color::rgb( 10,  20,  40); // #0A1428
    pub const SURFACE2:      Color = Color::rgb( 15,  30,  60); // #0F1E3C
    pub const TEAL:          Color = Color::rgb(  0, 200, 160); // #00C8A0
    pub const TEAL_DIM:      Color = Color::rgb(  0, 100,  80); // Teal escuro
    pub const CYAN:          Color = Color::rgb(100, 220, 255); // #64DCFF
    pub const PURPLE:        Color = Color::rgb(200, 100, 248); // #C864F8
    pub const ORANGE:        Color = Color::rgb(255, 179,  71); // #FFB347
    pub const RED:           Color = Color::rgb(255,  77, 109); // #FF4D6D
    pub const GREEN:         Color = Color::rgb(168, 255, 120); // #A8FF78
    pub const WHITE:         Color = Color::rgb(224, 232, 255); // #E0E8FF
    pub const GRAY:          Color = Color::rgb(120, 140, 180); // #788CB4
    pub const TRANSPARENT:   Color = Color(0x00000000);
    pub const TASKBAR_BG:    Color = Color::argb(220,  8,  16,  32);
    pub const WINDOW_BORDER: Color = Color::rgb(  0, 180, 144);
    pub const WINDOW_TITLE:  Color = Color::argb(240, 10,  20,  40);
}

/// Retângulo 2D
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self { Self { x, y, w, h } }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w as i32 &&
        py >= self.y && py < self.y + self.h as i32
    }

    pub fn right(&self)  -> i32 { self.x + self.w as i32 }
    pub fn bottom(&self) -> i32 { self.y + self.h as i32 }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()  && self.right()  > other.x &&
        self.y < other.bottom() && self.bottom() > other.y
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 > x1 && y2 > y1 {
            Some(Rect::new(x1, y1, (x2 - x1) as u32, (y2 - y1) as u32))
        } else {
            None
        }
    }
}

/// Ponto 2D
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point { pub x: i32, pub y: i32 }

impl Point {
    pub const fn new(x: i32, y: i32) -> Self { Self { x, y } }
}

/// Tamanho 2D
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size { pub w: u32, pub h: u32 }

impl Size {
    pub const fn new(w: u32, h: u32) -> Self { Self { w, h } }
}
