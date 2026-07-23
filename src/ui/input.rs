extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D Kernel — Sistema de Input Unificado
// ============================================================
//
// Unifica todos os dispositivos de entrada:
//   - Teclado PS/2 (já implementado na Fase 1)
//   - Mouse PS/2 / USB (Fase 3)
//   - Touch (Fase 4 — dispositivos móveis)
//   - Gestos AR (Fase 4 — óculos)
//
// Abstração de eventos:
//   Cada dispositivo gera InputEvent com coordenadas normalizadas.
//   O dispatcher encaminha para a surface em foco no compositor.
// ============================================================

use spinning_top::Spinlock;

/// Evento de input normalizado
#[derive(Debug, Clone)]
pub enum InputEvent {
    KeyDown   { key: char, modifiers: Modifiers },
    KeyUp     { key: char, modifiers: Modifiers },
    MouseMove { x: i32, y: i32, dx: i32, dy: i32 },
    MouseDown { x: i32, y: i32, button: MouseButton },
    MouseUp   { x: i32, y: i32, button: MouseButton },
    Scroll    { x: i32, y: i32, delta: i32 },
    Touch     { id: u32, x: i32, y: i32, pressure: f32 },
    TouchEnd  { id: u32 },
    Gesture   { kind: GestureKind },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub ctrl:  bool,
    pub alt:   bool,
    pub shift: bool,
    pub super_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseButton { Left, Right, Middle }

#[derive(Debug, Clone)]
pub enum GestureKind {
    Swipe { dx: i32, dy: i32 },
    Pinch { scale: f32 },
    Rotate { angle: f32 },
}

/// Estado do mouse
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: [bool; 3],
}

/// Gerenciador de input
pub struct InputManager {
    pub event_queue: Vec<InputEvent>,
    pub mouse: MouseState,
    pub modifiers: Modifiers,
    pub initialized: bool,
}

impl InputManager {
    const fn new() -> Self {
        Self {
            event_queue: Vec::new(),
            mouse: MouseState { x: 512, y: 384, buttons: [false; 3] },
            modifiers: Modifiers { ctrl: false, alt: false, shift: false, super_key: false },
            initialized: false,
        }
    }

    /// Injeta um evento na fila
    pub fn push_event(&mut self, event: InputEvent) {
        self.event_queue.push(event);
        if self.event_queue.len() > 256 {
            self.event_queue.drain(0..64);
        }
    }

    /// Processa evento de movimento do mouse
    pub fn mouse_move(&mut self, dx: i32, dy: i32) {
        self.mouse.x = (self.mouse.x + dx)
            .clamp(0, super::SCREEN_WIDTH as i32 - 1);
        self.mouse.y = (self.mouse.y + dy)
            .clamp(0, super::SCREEN_HEIGHT as i32 - 1);

        super::compositor::move_surface(0, self.mouse.x, self.mouse.y); // cursor
        super::compositor::set_cursor_pos(self.mouse.x, self.mouse.y);

        self.push_event(InputEvent::MouseMove {
            x: self.mouse.x,
            y: self.mouse.y,
            dx, dy,
        });
    }

    /// Processa clique do mouse
    pub fn mouse_click(&mut self, button: MouseButton) {
        let (x, y) = (self.mouse.x, self.mouse.y);

        // Hit test: qual surface está sob o cursor?
        if let Some(sid) = super::compositor::surface_at(x, y) {
            super::compositor::focus_surface(sid);
        }

        self.push_event(InputEvent::MouseDown { x, y, button });
    }

    /// Drena a fila de eventos (retorna e limpa)
    pub fn drain_events(&mut self) -> Vec<InputEvent> {
        core::mem::take(&mut self.event_queue)
    }
}

static INPUT: Spinlock<InputManager> = Spinlock::new(InputManager::new());

pub fn init() {
    INPUT.lock().initialized = true;
    crate::serial_println!("[UI][INPUT] Sistema de input unificado ativo");
    crate::serial_println!("[UI][INPUT] Teclado PS/2: ativo | Mouse: simulado | Touch: Fase 4");
}

pub fn push_event(event: InputEvent) {
    INPUT.lock().push_event(event);
}

pub fn mouse_move(dx: i32, dy: i32) {
    INPUT.lock().mouse_move(dx, dy);
}

pub fn mouse_click(button: MouseButton) {
    INPUT.lock().mouse_click(button);
}
