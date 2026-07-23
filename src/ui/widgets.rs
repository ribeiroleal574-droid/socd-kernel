extern crate alloc;
// ============================================================
// SOC-D Kernel — Engine de Widgets
// ============================================================
//
// Sistema declarativo de widgets para a UI do SOC-D.
//
// Hierarquia:
//   WidgetTree
//     └── Panel (container com layout)
//           ├── Label
//           ├── Button
//           ├── ProgressBar
//           ├── TextInput
//           ├── Icon
//           └── Panel (aninhado)
//
// Layout: Flexbox simplificado
//   Direction: Row | Column
//   Align: Start | Center | End | SpaceBetween
//   Gap: pixels entre widgets
//   Padding: interno ao container
//
// Renderização:
//   Cada widget sabe renderizar a si mesmo num Surface
//   O layout resolve posições antes da renderização
// ============================================================

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use spinning_top::Spinlock;

use super::{Color, Rect, palette, compositor};

// ─── IDs ─────────────────────────────────────────────────────────────────────

pub type WidgetId = u64;
static NEXT_WIDGET_ID: AtomicU64 = AtomicU64::new(1);
fn alloc_widget_id() -> WidgetId {
    NEXT_WIDGET_ID.fetch_add(1, Ordering::Relaxed)
}

// ─── Eventos ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum WidgetEvent {
    Click   { widget_id: WidgetId, x: i32, y: i32 },
    KeyDown { widget_id: WidgetId, key: char },
    Focus   { widget_id: WidgetId },
    Blur    { widget_id: WidgetId },
    Changed { widget_id: WidgetId, value: String },
}

// ─── Layout ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexDirection { Row, Column }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexAlign { Start, Center, End, SpaceBetween, SpaceAround }

#[derive(Debug, Clone, Copy)]
pub struct FlexLayout {
    pub direction: FlexDirection,
    pub align:     FlexAlign,
    pub gap:       u32,
    pub padding:   u32,
}

impl Default for FlexLayout {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Column,
            align: FlexAlign::Start,
            gap: 8,
            padding: 12,
        }
    }
}

// ─── Widget Trait ────────────────────────────────────────────────────────────

pub trait Widget: Send + Sync {
    fn id(&self)       -> WidgetId;
    fn bounds(&self)   -> Rect;
    fn set_bounds(&mut self, rect: Rect);
    fn visible(&self)  -> bool  { true }
    fn min_size(&self) -> (u32, u32) { (0, 0) }

    /// Renderiza o widget no buffer fornecido
    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, buf_h: u32);

    /// Processa evento de input
    fn handle_event(&mut self, _event: &WidgetEvent) -> bool { false }
}

// ─── Label ───────────────────────────────────────────────────────────────────

pub struct Label {
    id:     WidgetId,
    bounds: Rect,
    pub text:   String,
    pub color:  Color,
    pub scale:  u32,
    pub align:  FlexAlign,
}

impl Label {
    pub fn new(text: &str, color: Color) -> Self {
        Self {
            id: alloc_widget_id(),
            bounds: Rect::default(),
            text: text.to_string(),
            color,
            scale: 1,
            align: FlexAlign::Start,
        }
    }
    pub fn scaled(mut self, scale: u32) -> Self { self.scale = scale; self }
    pub fn centered(mut self) -> Self { self.align = FlexAlign::Center; self }
}

impl Widget for Label {
    fn id(&self)       -> WidgetId { self.id }
    fn bounds(&self)   -> Rect { self.bounds }
    fn set_bounds(&mut self, r: Rect) { self.bounds = r; }
    fn min_size(&self) -> (u32, u32) {
        (self.text.len() as u32 * 8 * self.scale, 8 * self.scale)
    }

    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, _buf_h: u32) {
        render_text_to_buf(buf, buf_w, &self.text,
            self.bounds.x, self.bounds.y, self.color, self.scale);
    }
}

// ─── Button ──────────────────────────────────────────────────────────────────

pub struct Button {
    id:       WidgetId,
    bounds:   Rect,
    pub label:    String,
    pub color:    Color,
    pub bg:       Color,
    pub hovered:  bool,
    pub pressed:  bool,
    pub on_click: Option<alloc::string::String>, // Identificador de ação
}

impl Button {
    pub fn new(label: &str) -> Self {
        Self {
            id: alloc_widget_id(),
            bounds: Rect::default(),
            label: label.to_string(),
            color: palette::WHITE,
            bg: palette::TEAL_DIM,
            hovered: false,
            pressed: false,
            on_click: None,
        }
    }
    pub fn action(mut self, action: &str) -> Self {
        self.on_click = Some(action.to_string()); self
    }
}

impl Widget for Button {
    fn id(&self)       -> WidgetId { self.id }
    fn bounds(&self)   -> Rect { self.bounds }
    fn set_bounds(&mut self, r: Rect) { self.bounds = r; }
    fn min_size(&self) -> (u32, u32) {
        (self.label.len() as u32 * 8 + 24, 28)
    }

    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, buf_h: u32) {
        let bg = if self.pressed {
            palette::TEAL
        } else if self.hovered {
            Color::rgb(0, 150, 120)
        } else {
            self.bg
        };

        // Fundo
        fill_rect_buf(buf, buf_w, buf_h, self.bounds, bg);

        // Borda
        draw_border_buf(buf, buf_w, buf_h, self.bounds, palette::TEAL, 1);

        // Label centralizado
        let lw = self.label.len() as i32 * 8;
        let lx = self.bounds.x + (self.bounds.w as i32 - lw) / 2;
        let ly = self.bounds.y + (self.bounds.h as i32 - 8) / 2;
        render_text_to_buf(buf, buf_w, &self.label, lx, ly, self.color, 1);
    }

    fn handle_event(&mut self, event: &WidgetEvent) -> bool {
        match event {
            WidgetEvent::Click { widget_id, .. } if *widget_id == self.id => {
                self.pressed = true;
                true
            }
            _ => false,
        }
    }
}

// ─── ProgressBar ─────────────────────────────────────────────────────────────

pub struct ProgressBar {
    id:        WidgetId,
    bounds:    Rect,
    pub value: f32,  // 0.0–1.0
    pub color: Color,
    pub label: Option<String>,
    pub show_percent: bool,
}

impl ProgressBar {
    pub fn new(value: f32, color: Color) -> Self {
        Self {
            id: alloc_widget_id(),
            bounds: Rect::default(),
            value: value.clamp(0.0, 1.0),
            color,
            label: None,
            show_percent: true,
        }
    }
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string()); self
    }
}

impl Widget for ProgressBar {
    fn id(&self)     -> WidgetId { self.id }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, r: Rect) { self.bounds = r; }
    fn min_size(&self) -> (u32, u32) { (120, 16) }

    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, buf_h: u32) {
        // Track (fundo)
        fill_rect_buf(buf, buf_w, buf_h, self.bounds,
            Color::rgb(20, 30, 50));
        draw_border_buf(buf, buf_w, buf_h, self.bounds,
            Color::rgb(0, 80, 64), 1);

        // Fill
        let fill_w = (self.bounds.w as f32 * self.value) as u32;
        if fill_w > 0 {
            let fill_rect = Rect::new(
                self.bounds.x + 1,
                self.bounds.y + 1,
                (fill_w - 2).max(1),
                self.bounds.h.saturating_sub(2),
            );
            fill_rect_buf(buf, buf_w, buf_h, fill_rect, self.color);

            // Brilho no topo
            let glow = Rect::new(fill_rect.x, fill_rect.y, fill_rect.w, 2);
            fill_rect_buf(buf, buf_w, buf_h, glow,
                Color::argb(120, 255, 255, 255));
        }

        // Label + percentual
        if self.show_percent {
            let pct = alloc::format!("{:.0}%", self.value * 100.0);
            let tx = self.bounds.x + (self.bounds.w as i32 - pct.len() as i32 * 8) / 2;
            let ty = self.bounds.y + (self.bounds.h as i32 - 8) / 2;
            render_text_to_buf(buf, buf_w, &pct, tx, ty, palette::WHITE, 1);
        }

        if let Some(lbl) = &self.label {
            let lx = self.bounds.x;
            let ly = self.bounds.y - 12;
            render_text_to_buf(buf, buf_w, lbl, lx, ly, palette::GRAY, 1);
        }
    }
}

// ─── TextInput ───────────────────────────────────────────────────────────────

pub struct TextInput {
    id:        WidgetId,
    bounds:    Rect,
    pub value: String,
    pub placeholder: String,
    pub focused: bool,
    pub cursor_pos: usize,
    pub password: bool,
}

impl TextInput {
    pub fn new(placeholder: &str) -> Self {
        Self {
            id: alloc_widget_id(),
            bounds: Rect::default(),
            value: String::new(),
            placeholder: placeholder.to_string(),
            focused: false,
            cursor_pos: 0,
            password: false,
        }
    }
}

impl Widget for TextInput {
    fn id(&self)     -> WidgetId { self.id }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, r: Rect) { self.bounds = r; }
    fn min_size(&self) -> (u32, u32) { (160, 24) }

    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, buf_h: u32) {
        let bg = if self.focused { Color::rgb(10, 25, 50) } else { Color::rgb(8, 16, 32) };
        fill_rect_buf(buf, buf_w, buf_h, self.bounds, bg);

        let border_color = if self.focused { palette::TEAL } else { palette::TEAL_DIM };
        draw_border_buf(buf, buf_w, buf_h, self.bounds, border_color, 1);

        let text = if self.value.is_empty() {
            &self.placeholder
        } else {
            &self.value
        };

        let display: String = if self.password && !self.value.is_empty() {
            "•".repeat(self.value.len())
        } else {
            text.clone().into()
        };

        let tc = if self.value.is_empty() { palette::GRAY } else { palette::WHITE };
        let tx = self.bounds.x + 6;
        let ty = self.bounds.y + (self.bounds.h as i32 - 8) / 2;
        render_text_to_buf(buf, buf_w, &display, tx, ty, tc, 1);

        // Cursor piscante (simplificado)
        if self.focused {
            let cx = tx + self.cursor_pos as i32 * 8;
            fill_rect_buf(buf, buf_w, buf_h,
                Rect::new(cx, ty, 1, 8), palette::TEAL);
        }
    }

    fn handle_event(&mut self, event: &WidgetEvent) -> bool {
        match event {
            WidgetEvent::KeyDown { widget_id, key } if *widget_id == self.id => {
                match key {
                    '\x08' => { // Backspace
                        if !self.value.is_empty() {
                            self.value.pop();
                            self.cursor_pos = self.cursor_pos.saturating_sub(1);
                        }
                    }
                    c => {
                        self.value.push(*c);
                        self.cursor_pos += 1;
                    }
                }
                true
            }
            WidgetEvent::Focus { widget_id } if *widget_id == self.id => {
                self.focused = true; true
            }
            WidgetEvent::Blur { widget_id } if *widget_id == self.id => {
                self.focused = false; true
            }
            _ => false,
        }
    }
}

// ─── Panel (Container) ───────────────────────────────────────────────────────

pub struct Panel {
    id:       WidgetId,
    bounds:   Rect,
    pub bg:   Option<Color>,
    pub border: Option<(Color, u32)>,
    pub layout: FlexLayout,
    pub children: Vec<Box<dyn Widget>>,
    pub title: Option<String>,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            id: alloc_widget_id(),
            bounds: Rect::default(),
            bg: Some(palette::SURFACE),
            border: None,
            layout: FlexLayout::default(),
            children: Vec::new(),
            title: None,
        }
    }

    pub fn with_bg(mut self, color: Color) -> Self { self.bg = Some(color); self }
    pub fn with_border(mut self, color: Color, width: u32) -> Self {
        self.border = Some((color, width)); self
    }
    pub fn with_layout(mut self, layout: FlexLayout) -> Self { self.layout = layout; self }
    pub fn with_title(mut self, title: &str) -> Self { self.title = Some(title.to_string()); self }
    pub fn row(mut self) -> Self { self.layout.direction = FlexDirection::Row; self }

    pub fn add<W: Widget + 'static>(&mut self, widget: W) {
        self.children.push(Box::new(widget));
    }

    /// Resolve posições dos filhos com base no layout
    pub fn layout_children(&mut self) {
        let pad = self.layout.padding as i32;
        let gap = self.layout.gap as i32;
        let title_offset = if self.title.is_some() { 20i32 } else { 0 };

        let available_w = self.bounds.w as i32 - pad * 2;
        let available_h = self.bounds.h as i32 - pad * 2 - title_offset;

        match self.layout.direction {
            FlexDirection::Column => {
                let mut y = self.bounds.y + pad + title_offset;
                for child in &mut self.children {
                    let (min_w, min_h) = child.min_size();
                    let w = min_w.max(available_w as u32).min(available_w as u32);
                    let h = min_h;
                    child.set_bounds(Rect::new(self.bounds.x + pad, y, w, h));
                    y += h as i32 + gap;
                }
            }
            FlexDirection::Row => {
                let mut x = self.bounds.x + pad;
                for child in &mut self.children {
                    let (min_w, min_h) = child.min_size();
                    let h = min_h.max(available_h as u32).min(available_h as u32);
                    child.set_bounds(Rect::new(x, self.bounds.y + pad + title_offset, min_w, h));
                    x += min_w as i32 + gap;
                }
            }
        }
    }
}

impl Widget for Panel {
    fn id(&self)     -> WidgetId { self.id }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, r: Rect) { self.bounds = r; }

    fn render(&self, buf: &mut Vec<u32>, buf_w: u32, buf_h: u32) {
        if let Some(bg) = self.bg {
            fill_rect_buf(buf, buf_w, buf_h, self.bounds, bg);
        }
        if let Some((color, width)) = self.border {
            draw_border_buf(buf, buf_w, buf_h, self.bounds, color, width);
        }
        if let Some(title) = &self.title {
            render_text_to_buf(buf, buf_w, title,
                self.bounds.x + 8,
                self.bounds.y + 6,
                palette::TEAL, 1);
        }
        for child in &self.children {
            child.render(buf, buf_w, buf_h);
        }
    }

    fn handle_event(&mut self, event: &WidgetEvent) -> bool {
        self.children.iter_mut().any(|c| c.handle_event(event))
    }
}

// ─── Helpers de Renderização em Buffer ───────────────────────────────────────

fn fill_rect_buf(buf: &mut Vec<u32>, bw: u32, bh: u32, rect: Rect, color: Color) {
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x + rect.w as i32).min(bw as i32) as u32;
    let y1 = (rect.y + rect.h as i32).min(bh as i32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * bw + x) as usize;
            if idx < buf.len() { buf[idx] = color.0; }
        }
    }
}

fn draw_border_buf(buf: &mut Vec<u32>, bw: u32, bh: u32, rect: Rect, color: Color, t: u32) {
    for i in 0..t {
        let i = i as i32;
        fill_rect_buf(buf, bw, bh, Rect::new(rect.x, rect.y + i, rect.w, 1), color);
        fill_rect_buf(buf, bw, bh,
            Rect::new(rect.x, rect.y + rect.h as i32 - 1 - i, rect.w, 1), color);
        fill_rect_buf(buf, bw, bh, Rect::new(rect.x + i, rect.y, 1, rect.h), color);
        fill_rect_buf(buf, bw, bh,
            Rect::new(rect.x + rect.w as i32 - 1 - i, rect.y, 1, rect.h), color);
    }
}

fn render_text_to_buf(buf: &mut Vec<u32>, bw: u32, text: &str,
                       x: i32, y: i32, color: Color, scale: u32) {
    use super::render::FONT_8X8_EXPORT as FONT;
    let max_idx = FONT.len() / 8;
    for (i, c) in text.chars().enumerate() {
        let code = c as u8;
        if code < 32 || code > 126 { continue; }
        let gi = ((code - 32) as usize).min(max_idx.saturating_sub(1));
        let rows = &FONT[gi * 8..(gi + 1) * 8];
        let cx = x + i as i32 * 8 * scale as i32;
        for (row, &bits) in rows.iter().enumerate() {
            for col in 0..8u32 {
                if (bits >> (7 - col)) & 1 != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = cx + (col * scale + sx) as i32;
                            let py = y + (row as u32 * scale + sy) as i32;
                            if px >= 0 && py >= 0 {
                                let idx = (py as u32 * bw + px as u32) as usize;
                                if idx < buf.len() { buf[idx] = color.0; }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── Inicialização ────────────────────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[UI][WIDGETS] Engine de widgets inicializada");
}
