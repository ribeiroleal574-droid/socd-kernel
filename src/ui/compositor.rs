extern crate alloc;
extern crate libm;
// ============================================================
// SOC-D Kernel — Compositor de Superfícies (Wayland-inspired)
// ============================================================
//
// O compositor gerencia todas as superfícies visíveis na tela.
// Inspirado no protocolo Wayland mas simplificado para bare metal.
//
// Conceitos:
//   Surface    — área retangular desenhável por um app/widget
//   Layer      — agrupamento de superfícies por profundidade:
//                Background → Desktop → Windows → Overlay → Cursor
//   Z-order    — ordem de composição (fundo → frente)
//   Damage     — regiões que precisam ser re-renderizadas
//   Compositor — combina todas as superfícies no framebuffer final
//
// Pipeline de composição:
//   1. Para cada layer (Background → Cursor)
//   2.   Para cada surface no layer (Z-order)
//   3.     Se surface dirty: renderiza widgets da surface
//   4.     Compõe surface no framebuffer (alpha blending)
//   5. Flip: copia framebuffer para display
// ============================================================

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use spinning_top::Spinlock;

use super::{Color, Rect, palette, render};

// ─── Identificadores ──────────────────────────────────────────────────────────

pub type SurfaceId = u64;
static NEXT_SURFACE_ID: AtomicU64 = AtomicU64::new(1);
fn alloc_surface_id() -> SurfaceId {
    NEXT_SURFACE_ID.fetch_add(1, Ordering::Relaxed)
}

// ─── Camadas de Composição ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Background = 0,
    Desktop    = 1,
    Windows    = 2,
    Floating   = 3,  // Diálogos, tooltips
    Overlay    = 4,  // Notificações, taskbar
    Cursor     = 5,
}

// ─── Superfície ───────────────────────────────────────────────────────────────

/// Estado de uma superfície no compositor
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceState {
    /// Criada mas não mapeada (invisível)
    Unmapped,
    /// Visível e sendo composta
    Mapped,
    /// Minimizada (oculta mas existe)
    Minimized,
    /// Destruída (aguarda coleta)
    Destroyed,
}

/// Uma superfície gerenciada pelo compositor
#[derive(Debug, Clone)]
pub struct Surface {
    pub id:       SurfaceId,
    pub title:    String,
    pub rect:     Rect,
    pub layer:    Layer,
    pub z_index:  i32,
    pub state:    SurfaceState,
    pub alpha:    u8,          // 0=transparente, 255=opaco
    pub dirty:    bool,        // Precisa ser re-renderizada?
    pub focused:  bool,
    /// Conteúdo da superfície (buffer de pixels ARGB)
    pub buffer:   Vec<u32>,
    /// Owner da superfície (PID ou widget ID)
    pub owner_id: u64,
    /// Decorações de janela (barra de título, bordas)?
    pub decorated: bool,
    /// Pode ser redimensionada?
    pub resizable: bool,
    /// Pode ser movida?
    pub movable: bool,
}

impl Surface {
    pub fn new(title: &str, rect: Rect, layer: Layer, owner_id: u64) -> Self {
        let buf_size = (rect.w * rect.h) as usize;
        Self {
            id: alloc_surface_id(),
            title: title.to_string(),
            rect,
            layer,
            z_index: 0,
            state: SurfaceState::Unmapped,
            alpha: 255,
            dirty: true,
            focused: false,
            buffer: alloc::vec![palette::SURFACE.0; buf_size.max(1)],
            owner_id,
            decorated: layer == Layer::Windows,
            resizable: layer == Layer::Windows,
            movable: true,
        }
    }

    /// Preenche o buffer da superfície com uma cor
    pub fn clear(&mut self, color: Color) {
        self.buffer.fill(color.0);
        self.dirty = true;
    }

    /// Escreve um pixel na superfície (coordenadas locais)
    pub fn put_pixel(&mut self, lx: i32, ly: i32, color: Color) {
        if lx < 0 || ly < 0 || lx >= self.rect.w as i32 || ly >= self.rect.h as i32 {
            return;
        }
        let idx = (ly as u32 * self.rect.w + lx as u32) as usize;
        if idx < self.buffer.len() {
            self.buffer[idx] = color.0;
            self.dirty = true;
        }
    }

    /// Move a superfície para nova posição
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
        self.dirty = true;
    }

    /// Resize (recria buffer)
    pub fn resize(&mut self, w: u32, h: u32) {
        self.rect.w = w;
        self.rect.h = h;
        self.buffer.resize((w * h) as usize, palette::SURFACE.0);
        self.dirty = true;
    }
}

// ─── Decorator de Janela ─────────────────────────────────────────────────────

fn draw_window_decoration(surface: &mut Surface) {
    if !surface.decorated { return; }

    let w = surface.rect.w;
    let title_h = 24u32;

    // Barra de título
    let title_color = if surface.focused {
        palette::WINDOW_TITLE
    } else {
        Color::argb(220, 8, 14, 28)
    };

    // Preenche barra de título
    for y in 0..title_h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if idx < surface.buffer.len() {
                surface.buffer[idx] = title_color.0;
            }
        }
    }

    // Título da janela (texto)
    let title = surface.title.clone();
    let tx = 8i32;
    let ty = (title_h / 2 - 4) as i32;
    draw_text_on_surface(surface, &title, tx, ty,
        if surface.focused { palette::WHITE } else { palette::GRAY });

    // Botões: fechar (vermelho), minimizar (laranja), maximizar (verde)
    let btn_y = (title_h / 2 - 5) as i32;
    let btn_w = surface.rect.w as i32 - 8;
    draw_circle_on_surface(surface, btn_w - 0, btn_y + 5, 5, palette::RED);
    draw_circle_on_surface(surface, btn_w - 16, btn_y + 5, 5, palette::ORANGE);
    draw_circle_on_surface(surface, btn_w - 32, btn_y + 5, 5, palette::GREEN);

    // Borda da janela
    let border_color = if surface.focused { palette::WINDOW_BORDER } else { palette::TEAL_DIM };
    for x in 0..w {
        // Topo
        if (x as usize) < surface.buffer.len() {
            surface.buffer[x as usize] = border_color.0;
        }
        // Base
        let bot = ((surface.rect.h - 1) * w + x) as usize;
        if bot < surface.buffer.len() {
            surface.buffer[bot] = border_color.0;
        }
    }
    for y in 0..surface.rect.h {
        // Esquerda
        let left = (y * w) as usize;
        if left < surface.buffer.len() { surface.buffer[left] = border_color.0; }
        // Direita
        let right = (y * w + w - 1) as usize;
        if right < surface.buffer.len() { surface.buffer[right] = border_color.0; }
    }
}

fn draw_text_on_surface(surface: &mut Surface, text: &str, x: i32, y: i32, color: Color) {
    let w = surface.rect.w as i32;
    let h = surface.rect.h as i32;
    for (i, c) in text.chars().enumerate() {
        let cx = x + i as i32 * 8;
        if cx >= w { break; }
        let code = c as u8;
        if code < 32 || code > 126 { continue; }

        // Inline glyph rendering
        let glyph_data: &[u8] = crate::ui::render::get_font();
        let glyph_idx = (code - 32) as usize;
        let max_idx = glyph_data.len() / 8;
        let glyph_idx = glyph_idx.min(max_idx.saturating_sub(1));
        let rows = &glyph_data[glyph_idx * 8..(glyph_idx + 1) * 8];

        for (row, &bits) in rows.iter().enumerate() {
            let py = y + row as i32;
            if py < 0 || py >= h { continue; }
            for col in 0..8i32 {
                let px = cx + col;
                if px < 0 || px >= w { continue; }
                if (bits >> (7 - col as u8)) & 1 != 0 {
                    let idx = (py as u32 * surface.rect.w + px as u32) as usize;
                    if idx < surface.buffer.len() {
                        surface.buffer[idx] = color.0;
                    }
                }
            }
        }
    }
}

fn draw_circle_on_surface(surface: &mut Surface, cx: i32, cy: i32, r: i32, color: Color) {
    let w = surface.rect.w as i32;
    let h = surface.rect.h as i32;
    for dy in -r..=r {
        let dx = libm::sqrtf((r*r - dy*dy) as f32) as i32;
        for px in (cx - dx)..=(cx + dx) {
            let py = cy + dy;
            if px >= 0 && px < w && py >= 0 && py < h {
                let idx = (py as u32 * surface.rect.w + px as u32) as usize;
                if idx < surface.buffer.len() {
                    surface.buffer[idx] = color.0;
                }
            }
        }
    }
}

// ─── Compositor ──────────────────────────────────────────────────────────────

pub struct Compositor {
    surfaces: BTreeMap<SurfaceId, Surface>,
    damage_regions: Vec<Rect>,
    frames_composed: u64,
    cursor_pos: (i32, i32),
}

impl Compositor {
    const fn new() -> Self {
        Self {
            surfaces: BTreeMap::new(),
            damage_regions: Vec::new(),
            frames_composed: 0,
            cursor_pos: (512, 384),
        }
    }

    /// Cria e registra uma nova superfície
    pub fn create_surface(&mut self, title: &str, rect: Rect, layer: Layer, owner: u64) -> SurfaceId {
        let mut surface = Surface::new(title, rect, layer, owner);
        surface.state = SurfaceState::Mapped;
        let id = surface.id;
        crate::serial_println!("[UI][COMP] Surface criada: '{}' id={} {:?}", title, id, rect);
        self.surfaces.insert(id, surface);
        self.invalidate_all();
        id
    }

    /// Destroi uma superfície
    pub fn destroy_surface(&mut self, id: SurfaceId) {
        if let Some(s) = self.surfaces.remove(&id) {
            crate::serial_println!("[UI][COMP] Surface destruida: '{}' id={}", s.title, id);
            self.invalidate_all();
        }
    }

    /// Move uma superfície
    pub fn move_surface(&mut self, id: SurfaceId, x: i32, y: i32) {
        if let Some(s) = self.surfaces.get_mut(&id) {
            s.move_to(x, y);
            self.invalidate_all();
        }
    }

    /// Foca uma superfície (traz para frente no layer)
    pub fn focus_surface(&mut self, id: SurfaceId) {
        // Desfoca todas
        for s in self.surfaces.values_mut() {
            s.focused = false;
        }
        if let Some(s) = self.surfaces.get_mut(&id) {
            s.focused = true;
            s.z_index += 1;
            s.dirty = true;
        }
    }

    /// Atualiza posição do cursor
    pub fn set_cursor(&mut self, x: i32, y: i32) {
        self.cursor_pos = (x, y);
        self.damage_regions.push(Rect::new(x - 10, y - 10, 20, 20));
    }

    /// Marca toda a tela como inválida
    pub fn invalidate_all(&mut self) {
        use super::{SCREEN_WIDTH, SCREEN_HEIGHT};
        self.damage_regions.push(Rect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT));
        for s in self.surfaces.values_mut() {
            s.dirty = true;
        }
    }

    /// Composição principal — uma frame completa
    pub fn composite_frame(&mut self) {
        render::with_fb(|fb| {
            // Limpa com cor de fundo
            fb.clear(palette::BACKGROUND);

            // Ordena surfaces por layer + z_index
            let mut order: Vec<SurfaceId> = self.surfaces.keys().copied().collect();
            order.sort_by_key(|&id| {
                self.surfaces.get(&id)
                    .map(|s| (s.layer as i32) * 1000 + s.z_index)
                    .unwrap_or(0)
            });

            // Compõe cada surface
            for id in order {
                let surface = match self.surfaces.get_mut(&id) {
                    Some(s) => s,
                    None => continue,
                };

                if surface.state != SurfaceState::Mapped { continue; }

                // Re-decora janelas sujas
                if surface.dirty && surface.decorated {
                    draw_window_decoration(surface);
                }
                surface.dirty = false;

                // Copia buffer da surface para o framebuffer
                let sx = surface.rect.x;
                let sy = surface.rect.y;
                let sw = surface.rect.w as i32;
                let sh = surface.rect.h as i32;
                let alpha = surface.alpha;

                for ly in 0..sh {
                    for lx in 0..sw {
                        let si = (ly as u32 * surface.rect.w + lx as u32) as usize;
                        if si >= surface.buffer.len() { continue; }

                        let pixel = Color(surface.buffer[si]);
                        let screen_x = sx + lx;
                        let screen_y = sy + ly;

                        let final_color = if alpha == 255 {
                            pixel
                        } else {
                            // Alpha blending com o que já está no framebuffer
                            let bg = fb.get_pixel(screen_x, screen_y);
                            let blended = Color::argb(alpha,
                                pixel.r(), pixel.g(), pixel.b());
                            blended.blend_over(bg)
                        };

                        fb.put_pixel(screen_x, screen_y, final_color);
                    }
                }
            }

            // Desenha cursor
            let (cx, cy) = self.cursor_pos;
            // Seta do cursor (simples)
            fb.draw_line(cx, cy, cx + 10, cy + 8, palette::WHITE);
            fb.draw_line(cx, cy, cx,      cy + 12, palette::WHITE);
            fb.draw_line(cx + 5, cy + 9, cx, cy + 12, palette::WHITE);
        });

        self.damage_regions.clear();
        self.frames_composed += 1;
    }

    /// Retorna surface sob o ponto (x, y) — usada para hit testing
    pub fn surface_at(&self, x: i32, y: i32) -> Option<SurfaceId> {
        // Busca de frente para trás (maior z primeiro)
        let mut candidates: Vec<&Surface> = self.surfaces.values()
            .filter(|s| s.state == SurfaceState::Mapped && s.rect.contains(x, y))
            .collect();
        candidates.sort_by(|a, b| {
            let a_z = (a.layer as i32) * 1000 + a.z_index;
            let b_z = (b.layer as i32) * 1000 + b.z_index;
            b_z.cmp(&a_z)
        });
        candidates.first().map(|s| s.id)
    }

    pub fn stats(&self) -> CompositorStats {
        CompositorStats {
            total_surfaces: self.surfaces.len(),
            mapped: self.surfaces.values().filter(|s| s.state == SurfaceState::Mapped).count(),
            frames_composed: self.frames_composed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompositorStats {
    pub total_surfaces: usize,
    pub mapped: usize,
    pub frames_composed: u64,
}

static COMPOSITOR: Spinlock<Compositor> = Spinlock::new(Compositor::new());

pub fn init() {
    crate::serial_println!("[UI][COMP] Compositor inicializado");
}

pub fn composite_frame() {
    COMPOSITOR.lock().composite_frame();
}

pub fn create_surface(title: &str, rect: Rect, layer: Layer, owner: u64) -> SurfaceId {
    COMPOSITOR.lock().create_surface(title, rect, layer, owner)
}

pub fn destroy_surface(id: SurfaceId) {
    COMPOSITOR.lock().destroy_surface(id);
}

pub fn move_surface(id: SurfaceId, x: i32, y: i32) {
    COMPOSITOR.lock().move_surface(id, x, y);
}

pub fn focus_surface(id: SurfaceId) {
    COMPOSITOR.lock().focus_surface(id);
}

pub fn surface_at(x: i32, y: i32) -> Option<SurfaceId> {
    COMPOSITOR.lock().surface_at(x, y)
}

pub fn stats() -> CompositorStats {
    COMPOSITOR.lock().stats()
}

pub fn set_cursor_pos(x: i32, y: i32) {
    COMPOSITOR.lock().set_cursor(x, y);
}
