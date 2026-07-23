extern crate alloc;
use alloc::vec::Vec;
extern crate libm;
// ============================================================
// SOC-D Kernel — Backend de Renderização (Framebuffer)
// ============================================================
//
// Renderiza pixels diretamente no framebuffer linear.
//
// Fase 3 (atual): Framebuffer VESA/UEFI GOP
//   - O bootloader mapeia o framebuffer no espaço virtual
//   - Escrita direta via ponteiro (sem DMA)
//   - Double buffering: back buffer em RAM, flip atômico
//
// Fase 4: Vulkan via virtio-gpu
//   - Renderização acelerada por GPU
//   - Shaders SPIR-V
//   - Composição por GPU
//
// Primitivas implementadas:
//   fill_rect    — retângulo sólido
//   draw_rect    — borda de retângulo
//   draw_line    — linha (Bresenham)
//   draw_circle  — círculo (Midpoint)
//   draw_text    — texto bitmap (fonte 8x8)
//   blit         — copia buffer para framebuffer
//   gradient_rect — retângulo com gradiente linear
// ============================================================

use spinning_top::Spinlock;
use super::{Color, Rect, SCREEN_WIDTH, SCREEN_HEIGHT, FRAMEBUFFER_SIZE};

// ─── Fonte Bitmap 8×8 ────────────────────────────────────────────────────────
// Fonte CP437 simplificada — apenas ASCII imprimível (32–127)
// Cada caractere: 8 bytes, 1 bit por pixel, MSB primeiro

const FONT_WIDTH:  u32 = 8;
const FONT_HEIGHT: u32 = 8;

/// Dados da fonte bitmap 8x8 (subset ASCII)
/// Cada caractere ocupa 8 bytes (8 linhas × 8 pixels)
static FONT_8X8: &[u8] = &[
    // ' ' (32)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // '!' (33)
    0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00,
    // '"' (34)
    0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // '#' (35)
    0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x00,
    // '$' (36)
    0x0C, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x0C, 0x00,
    // '%' (37)
    0x00, 0x63, 0x33, 0x18, 0x0C, 0x66, 0x63, 0x00,
    // '&' (38)
    0x1C, 0x36, 0x1C, 0x6E, 0x3B, 0x33, 0x6E, 0x00,
    // '\'' (39)
    0x06, 0x06, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
    // '(' (40)
    0x18, 0x0C, 0x06, 0x06, 0x06, 0x0C, 0x18, 0x00,
    // ')' (41)
    0x06, 0x0C, 0x18, 0x18, 0x18, 0x0C, 0x06, 0x00,
    // '*' (42)
    0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00,
    // '+' (43)
    0x00, 0x0C, 0x0C, 0x3F, 0x0C, 0x0C, 0x00, 0x00,
    // ',' (44)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x06,
    // '-' (45)
    0x00, 0x00, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x00,
    // '.' (46)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00,
    // '/' (47)
    0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x01, 0x00,
    // '0' (48)
    0x3E, 0x63, 0x73, 0x7B, 0x6F, 0x67, 0x3E, 0x00,
    // '1' (49)
    0x0C, 0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x3F, 0x00,
    // '2' (50)
    0x1E, 0x33, 0x30, 0x1C, 0x06, 0x33, 0x3F, 0x00,
    // '3' (51)
    0x1E, 0x33, 0x30, 0x1C, 0x30, 0x33, 0x1E, 0x00,
    // '4' (52)
    0x38, 0x3C, 0x36, 0x33, 0x7F, 0x30, 0x78, 0x00,
    // '5' (53)
    0x3F, 0x03, 0x1F, 0x30, 0x30, 0x33, 0x1E, 0x00,
    // '6' (54)
    0x1C, 0x06, 0x03, 0x1F, 0x33, 0x33, 0x1E, 0x00,
    // '7' (55)
    0x3F, 0x33, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x00,
    // '8' (56)
    0x1E, 0x33, 0x33, 0x1E, 0x33, 0x33, 0x1E, 0x00,
    // '9' (57)
    0x1E, 0x33, 0x33, 0x3E, 0x30, 0x18, 0x0E, 0x00,
    // ':' (58)
    0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x00,
    // ';' (59)
    0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x06,
    // '<' (60)
    0x18, 0x0C, 0x06, 0x03, 0x06, 0x0C, 0x18, 0x00,
    // '=' (61)
    0x00, 0x00, 0x3F, 0x00, 0x00, 0x3F, 0x00, 0x00,
    // '>' (62)
    0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00,
    // '?' (63)
    0x1E, 0x33, 0x30, 0x18, 0x0C, 0x00, 0x0C, 0x00,
    // '@' (64)
    0x3E, 0x63, 0x7B, 0x7B, 0x7B, 0x03, 0x1E, 0x00,
    // 'A' (65)
    0x0C, 0x1E, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x00,
    // 'B' (66)
    0x3F, 0x66, 0x66, 0x3E, 0x66, 0x66, 0x3F, 0x00,
    // 'C' (67)
    0x3C, 0x66, 0x03, 0x03, 0x03, 0x66, 0x3C, 0x00,
    // 'D' (68)
    0x1F, 0x36, 0x66, 0x66, 0x66, 0x36, 0x1F, 0x00,
    // 'E' (69)
    0x7F, 0x46, 0x16, 0x1E, 0x16, 0x46, 0x7F, 0x00,
    // 'F' (70)
    0x7F, 0x46, 0x16, 0x1E, 0x16, 0x06, 0x0F, 0x00,
    // 'G' (71)
    0x3C, 0x66, 0x03, 0x03, 0x73, 0x66, 0x7C, 0x00,
    // 'H' (72)
    0x33, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x33, 0x00,
    // 'I' (73)
    0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00,
    // 'J' (74)
    0x78, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E, 0x00,
    // 'K' (75)
    0x67, 0x66, 0x36, 0x1E, 0x36, 0x66, 0x67, 0x00,
    // 'L' (76)
    0x0F, 0x06, 0x06, 0x06, 0x46, 0x66, 0x7F, 0x00,
    // 'M' (77)
    0x63, 0x77, 0x7F, 0x7F, 0x6B, 0x63, 0x63, 0x00,
    // 'N' (78)
    0x63, 0x67, 0x6F, 0x7B, 0x73, 0x63, 0x63, 0x00,
    // 'O' (79)
    0x1C, 0x36, 0x63, 0x63, 0x63, 0x36, 0x1C, 0x00,
    // 'P' (80)
    0x3F, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x0F, 0x00,
    // 'Q' (81)
    0x1E, 0x33, 0x33, 0x33, 0x3B, 0x1E, 0x38, 0x00,
    // 'R' (82)
    0x3F, 0x66, 0x66, 0x3E, 0x36, 0x66, 0x67, 0x00,
    // 'S' (83)
    0x1E, 0x33, 0x07, 0x0E, 0x38, 0x33, 0x1E, 0x00,
    // 'T' (84)
    0x3F, 0x2D, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00,
    // 'U' (85)
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00,
    // 'V' (86)
    0x33, 0x33, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00,
    // 'W' (87)
    0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00,
    // 'X' (88)
    0x63, 0x63, 0x36, 0x1C, 0x1C, 0x36, 0x63, 0x00,
    // 'Y' (89)
    0x33, 0x33, 0x33, 0x1E, 0x0C, 0x0C, 0x1E, 0x00,
    // 'Z' (90)
    0x7F, 0x63, 0x31, 0x18, 0x4C, 0x66, 0x7F, 0x00,
    // '[' (91)
    0x1E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x1E, 0x00,
    // '\' (92)
    0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00,
    // ']' (93)
    0x1E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1E, 0x00,
    // '^' (94)
    0x08, 0x1C, 0x36, 0x63, 0x00, 0x00, 0x00, 0x00,
    // '_' (95)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
];

/// Re-export público da fonte bitmap 8×8.
/// Usado por `widgets.rs`: `use super::render::FONT_8X8_EXPORT as FONT;`
pub static FONT_8X8_EXPORT: &[u8] = FONT_8X8;

fn glyph(c: u8) -> &'static [u8] {
    let idx = c.saturating_sub(32) as usize;
    let max = FONT_8X8.len() / 8;
    let idx = idx.min(max.saturating_sub(1));
    &FONT_8X8[idx * 8..(idx + 1) * 8]
}

// ─── Framebuffer ─────────────────────────────────────────────────────────────

/// Double buffer: back (escrita) + front (exibição)
pub struct Framebuffer {
    /// Back buffer — renderizamos aqui
    pub back:  Vec<u32>,
    /// Stride em pixels (pode diferir da largura por alinhamento)
    pub stride: u32,
    pub width:  u32,
    pub height: u32,
    /// Endereço físico do framebuffer (mapeado pelo bootloader)
    pub phys_addr: u64,
}

impl Framebuffer {
    fn new() -> Self {
        Self {
            back:  alloc::vec![0u32; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
            stride: SCREEN_WIDTH,
            width:  SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
            phys_addr: 0xFD00_0000, // Endereço típico do GOP framebuffer
        }
    }

    /// Escreve um pixel no back buffer
    #[inline]
    pub fn put_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = (y as u32 * self.stride + x as u32) as usize;
        if idx < self.back.len() {
            self.back[idx] = color.0;
        }
    }

    /// Lê um pixel do back buffer
    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> Color {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return Color(0);
        }
        let idx = (y as u32 * self.stride + x as u32) as usize;
        Color(self.back.get(idx).copied().unwrap_or(0))
    }

    /// Preenche um retângulo com cor sólida
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let x0 = rect.x.max(0) as u32;
        let y0 = rect.y.max(0) as u32;
        let x1 = (rect.x + rect.w as i32).min(self.width as i32) as u32;
        let y1 = (rect.y + rect.h as i32).min(self.height as i32) as u32;

        for y in y0..y1 {
            let row_start = (y * self.stride) as usize;
            let row_end   = (row_start + x1 as usize).min(self.back.len());
            let col_start = row_start + x0 as usize;
            let back_len = self.back.len();
            let end = row_end.min(back_len);
            if col_start < end {
                for px in &mut self.back[col_start..end] {
                    *px = color.0;
                }
            }
        }
    }

    /// Desenha borda de retângulo
    pub fn draw_rect_border(&mut self, rect: Rect, color: Color, thickness: u32) {
        for t in 0..thickness {
            let t = t as i32;
            // Top
            self.fill_rect(Rect::new(rect.x, rect.y + t, rect.w, 1), color);
            // Bottom
            self.fill_rect(Rect::new(rect.x, rect.y + rect.h as i32 - 1 - t, rect.w, 1), color);
            // Left
            self.fill_rect(Rect::new(rect.x + t, rect.y, 1, rect.h), color);
            // Right
            self.fill_rect(Rect::new(rect.x + rect.w as i32 - 1 - t, rect.y, 1, rect.h), color);
        }
    }

    /// Linha de Bresenham
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1i32 } else { -1 };
        let sy = if y0 < y1 { 1i32 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);

        loop {
            self.put_pixel(x, y, color);
            if x == x1 && y == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy { err += dy; x += sx; }
            if e2 <= dx { err += dx; y += sy; }
        }
    }

    /// Círculo — algoritmo do ponto médio
    pub fn draw_circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        let (mut x, mut y, mut p) = (0i32, r, 1 - r);
        while x <= y {
            for &(px, py) in &[
                (cx+x, cy+y), (cx-x, cy+y), (cx+x, cy-y), (cx-x, cy-y),
                (cx+y, cy+x), (cx-y, cy+x), (cx+y, cy-x), (cx-y, cy-x),
            ] { self.put_pixel(px, py, color); }
            x += 1;
            if p < 0 { p += 2*x + 1; } else { y -= 1; p += 2*(x-y) + 1; }
        }
    }

    /// Círculo preenchido
    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        for dy in -r..=r {
            let dx = libm::sqrtf((r*r - dy*dy) as f32) as i32;
            self.fill_rect(Rect::new(cx - dx, cy + dy, (dx * 2) as u32, 1), color);
        }
    }

    /// Gradiente horizontal
    pub fn gradient_rect_h(&mut self, rect: Rect, c0: Color, c1: Color) {
        let w = rect.w as i32;
        for dx in 0..w {
            let t = dx as f32 / w.max(1) as f32;
            let r = (c0.r() as f32 * (1.0 - t) + c1.r() as f32 * t) as u8;
            let g = (c0.g() as f32 * (1.0 - t) + c1.g() as f32 * t) as u8;
            let b = (c0.b() as f32 * (1.0 - t) + c1.b() as f32 * t) as u8;
            self.fill_rect(Rect::new(rect.x + dx, rect.y, 1, rect.h), Color::rgb(r, g, b));
        }
    }

    /// Renderiza um caractere bitmap
    pub fn draw_char(&mut self, c: char, x: i32, y: i32, fg: Color, bg: Option<Color>) {
        let code = c as u8;
        if code < 32 || code > 126 { return; }
        let rows = glyph(code);
        for (row, &bits) in rows.iter().enumerate() {
            for col in 0..8u32 {
                let set = (bits >> (7 - col)) & 1 != 0;
                if set {
                    self.put_pixel(x + col as i32, y + row as i32, fg);
                } else if let Some(bg) = bg {
                    self.put_pixel(x + col as i32, y + row as i32, bg);
                }
            }
        }
    }

    /// Renderiza uma string
    pub fn draw_text(&mut self, text: &str, x: i32, y: i32, fg: Color, bg: Option<Color>) -> i32 {
        let mut cx = x;
        for c in text.chars() {
            if c == '\n' {
                return cx;
            }
            self.draw_char(c, cx, y, fg, bg);
            cx += FONT_WIDTH as i32;
        }
        cx
    }

    /// Renderiza string com escala (pixels por pixel da fonte)
    pub fn draw_text_scaled(&mut self, text: &str, x: i32, y: i32,
                             fg: Color, bg: Option<Color>, scale: u32) -> i32 {
        let mut cx = x;
        for c in text.chars() {
            let code = c as u8;
            if code < 32 || code > 126 { cx += (FONT_WIDTH * scale) as i32; continue; }
            let rows = glyph(code);
            for (row, &bits) in rows.iter().enumerate() {
                for col in 0..8u32 {
                    let set = (bits >> (7 - col)) & 1 != 0;
                    let color = if set { fg } else if let Some(b) = bg { b } else { continue };
                    self.fill_rect(Rect::new(
                        cx + (col * scale) as i32,
                        y + (row as u32 * scale) as i32,
                        scale, scale,
                    ), color);
                }
            }
            cx += (FONT_WIDTH * scale) as i32;
        }
        cx
    }

    /// Copia back buffer para o framebuffer físico
    pub fn flip(&self) {
        // Fase 3: copiar para phys_addr via DMA ou memcpy
        // Por agora: simulado (o VGA está em 0xB8000, framebuffer em endereço diferente)
        // Em hardware real com UEFI GOP:
        //   unsafe { core::ptr::copy_nonoverlapping(
        //       self.back.as_ptr(),
        //       self.phys_addr as *mut u32,
        //       self.back.len()
        //   ); }
    }

    /// Limpa o buffer com a cor de fundo
    pub fn clear(&mut self, color: Color) {
        self.back.fill(color.0);
    }

    /// Largura em pixels do texto
    pub fn text_width(text: &str) -> u32 {
        text.len() as u32 * FONT_WIDTH
    }
}

static FRAMEBUFFER: Spinlock<Framebuffer> = Spinlock::new(Framebuffer {
    back: Vec::new(),
    stride: SCREEN_WIDTH,
    width: SCREEN_WIDTH,
    height: SCREEN_HEIGHT,
    phys_addr: 0xFD00_0000,
});

pub fn init() {
    let mut fb = FRAMEBUFFER.lock();
    *fb = Framebuffer::new();
    fb.clear(super::palette::BACKGROUND);
    crate::serial_println!("[UI][RENDER] Framebuffer {}x{} inicializado ({} KB)",
        SCREEN_WIDTH, SCREEN_HEIGHT,
        FRAMEBUFFER_SIZE / 1024);
}

/// Apresenta o frame renderizado
pub fn present() {
    FRAMEBUFFER.lock().flip();
}

/// Executa operações de desenho no framebuffer
pub fn with_fb<F: FnOnce(&mut Framebuffer)>(f: F) {
    f(&mut FRAMEBUFFER.lock());
}


/// Retorna referência à fonte 8x8 (usada pelo compositor e widgets)
pub fn get_font() -> &'static [u8] {
    FONT_8X8
}

