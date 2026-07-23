extern crate alloc;
extern crate libm;
// ============================================================
// SOC-D Kernel — Shell Desktop
// ============================================================
//
// O shell é a interface principal do usuário no SOC-D.
// Composto por:
//   - Taskbar (barra inferior): apps abertos, relógio, status
//   - Desktop: fundo + ícones de apps
//   - Launcher: grade de apps disponíveis
//   - Monitor do Sistema: janela com métricas em tempo real
//   - Notificações: sugestões da IA, alertas de segurança
//
// Layout Desktop:
//   ┌─────────────────────────────────────────────────┐
//   │                    DESKTOP                       │
//   │  [Ícones]                          [Monitor IA] │
//   │                                                  │
//   │  [Janela App 1]    [Janela App 2]               │
//   │                                                  │
//   ├─────────────────────────────────────────────────┤
//   │  TASKBAR: [Apps] ────────── [P2P][IA][Relógio] │
//   └─────────────────────────────────────────────────┘
// ============================================================

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use spinning_top::Spinlock;

use super::{
    Color, Rect,
    compositor::{self, Layer, SurfaceId},
    palette,
    render,
    widgets::*,
    SCREEN_WIDTH, SCREEN_HEIGHT,
};

const TASKBAR_HEIGHT: u32 = 36;
const DESKTOP_H: u32 = SCREEN_HEIGHT - TASKBAR_HEIGHT;

// ─── App registrado no shell ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub id:    u64,
    pub name:  String,
    pub icon:  char,
    pub color: Color,
    pub open:  bool,
    pub surface_id: Option<SurfaceId>,
}

// ─── Shell State ─────────────────────────────────────────────────────────────

pub struct DesktopShell {
    pub initialized:  bool,
    pub tick:         u64,

    /// Surface da taskbar (overlay)
    taskbar_surface:  Option<SurfaceId>,
    /// Surface do fundo (background)
    wallpaper_surface: Option<SurfaceId>,
    /// Surface do monitor do sistema
    monitor_surface:  Option<SurfaceId>,
    /// Surface do launcher
    launcher_surface: Option<SurfaceId>,
    launcher_open:    bool,

    /// Apps disponíveis
    apps: Vec<AppEntry>,

    /// Notificações pendentes da IA
    notifications: Vec<(String, u64)>, // (msg, tick_expiry)
}

impl DesktopShell {
    const fn new() -> Self {
        Self {
            initialized: false,
            tick: 0,
            taskbar_surface:  None,
            wallpaper_surface: None,
            monitor_surface:  None,
            launcher_surface: None,
            launcher_open:    false,
            apps: Vec::new(),
            notifications: Vec::new(),
        }
    }

    fn setup_apps(&mut self) {
        self.apps = alloc::vec![
            AppEntry { id: 1,  name: "Terminal".into(),    icon: '>', color: palette::TEAL,   open: false, surface_id: None },
            AppEntry { id: 2,  name: "Files".into(),       icon: 'F', color: palette::CYAN,   open: false, surface_id: None },
            AppEntry { id: 3,  name: "Settings".into(),    icon: 'S', color: palette::GRAY,   open: false, surface_id: None },
            AppEntry { id: 4,  name: "P2P Network".into(), icon: 'N', color: palette::PURPLE, open: false, surface_id: None },
            AppEntry { id: 5,  name: "IA Monitor".into(),  icon: 'I', color: palette::GREEN,  open: true,  surface_id: None },
            AppEntry { id: 6,  name: "Security".into(),    icon: 'K', color: palette::RED,    open: false, surface_id: None },
        ];
    }

    /// Renderiza o fundo (wallpaper) com gradiente e grid
    fn render_wallpaper(&self) {
        if let Some(sid) = self.wallpaper_surface {
            render::with_fb(|fb| {
                // Gradiente diagonal
                for y in 0..DESKTOP_H {
                    for x in 0..SCREEN_WIDTH {
                        let tx = x as f32 / SCREEN_WIDTH as f32;
                        let ty = y as f32 / DESKTOP_H as f32;
                        let r = (5.0  + tx * 8.0)  as u8;
                        let g = (10.0 + ty * 12.0) as u8;
                        let b = (20.0 + (tx + ty) * 15.0) as u8;
                        fb.put_pixel(x as i32, y as i32, Color::rgb(r, g, b));
                    }
                }

                // Grid sutil
                let grid_color = Color::argb(25, 0, 200, 160);
                for y in (0..DESKTOP_H).step_by(40) {
                    fb.draw_line(0, y as i32, SCREEN_WIDTH as i32, y as i32, grid_color);
                }
                for x in (0..SCREEN_WIDTH).step_by(40) {
                    fb.draw_line(x as i32, 0, x as i32, DESKTOP_H as i32, grid_color);
                }

                // Logo SOC-D no canto
                fb.draw_text_scaled("SOC-D", 20, 20, palette::TEAL_DIM, None, 2);
                fb.draw_text("Sistema Operacional Cognitivo Distribuido",
                    20, 38, palette::TEAL_DIM, None);
                fb.draw_text("Fase 3 — Interface Grafica Ativa",
                    20, 50, Color::argb(80, 0, 200, 160), None);
            });
        }
    }

    /// Renderiza a taskbar
    fn render_taskbar(&self) {
        render::with_fb(|fb| {
            let ty = DESKTOP_H as i32;
            let tw = SCREEN_WIDTH;
            let th = TASKBAR_HEIGHT;

            // Fundo da taskbar semi-transparente
            for y in ty..ty + th as i32 {
                for x in 0..tw {
                    let c = Color::argb(220, 8, 14, 28);
                    fb.put_pixel(x as i32, y, c);
                }
            }
            // Linha de separação
            fb.draw_line(0, ty, tw as i32, ty, palette::TEAL_DIM);

            // Botão launcher (≡)
            fb.fill_rect(Rect::new(4, ty + 4, 28, 28), palette::TEAL_DIM);
            fb.draw_rect_border(Rect::new(4, ty + 4, 28, 28), palette::TEAL, 1);
            fb.draw_text("=", 11, ty + 12, palette::WHITE, None);

            // Apps abertos na taskbar
            let mut ax = 40i32;
            for app in &self.apps {
                if !app.open { continue; }
                let btn_rect = Rect::new(ax, ty + 4, 90, 28);
                fb.fill_rect(btn_rect, Color::argb(180, 10, 20, 40));
                fb.draw_rect_border(btn_rect, app.color, 1);
                fb.draw_text(&app.name[..app.name.len().min(8)],
                    ax + 8, ty + 12, app.color, None);
                ax += 96;
            }

            // Status direita: P2P, IA, relógio
            let right = tw as i32 - 8;

            // Relógio simulado
            let tick_s = self.tick / 1000;
            let h = (tick_s / 3600) % 24;
            let m = (tick_s / 60) % 60;
            let s = tick_s % 60;
            let time_str = alloc::format!("{:02}:{:02}:{:02}", h, m, s);
            let tw_px = time_str.len() as i32 * 8;
            fb.draw_text(&time_str, right - tw_px, ty + 14, palette::WHITE, None);

            // Indicador P2P
            let (_, active) = crate::p2p::peer::count_peers();
            let p2p_str = alloc::format!("P2P:{}", active);
            let p2p_x = right - tw_px - p2p_str.len() as i32 * 8 - 16;
            fb.draw_text(&p2p_str, p2p_x, ty + 14,
                if active > 0 { palette::TEAL } else { palette::GRAY }, None);

            // Indicador IA
            let ia_stats = crate::ia::get_stats();
            let ia_str = alloc::format!("IA:{}", ia_stats.inferences_total);
            let ia_x = p2p_x - ia_str.len() as i32 * 8 - 16;
            fb.draw_text(&ia_str, ia_x, ty + 14, palette::PURPLE, None);
        });
    }

    /// Renderiza a janela do monitor do sistema
    fn render_system_monitor(&self, tick: u64) {
        let x = (SCREEN_WIDTH - 320) as i32 - 12;
        let y = 50i32;
        let w = 308u32;
        let h = 280u32;

        render::with_fb(|fb| {
            // Fundo da janela
            fb.fill_rect(Rect::new(x, y, w, h), Color::argb(230, 8, 16, 32));
            fb.draw_rect_border(Rect::new(x, y, w, h), palette::TEAL, 1);

            // Título
            fb.fill_rect(Rect::new(x, y, w, 22), Color::argb(255, 10, 22, 44));
            fb.draw_line(x, y + 22, x + w as i32, y + 22, palette::TEAL_DIM);
            fb.draw_text("Monitor do Sistema", x + 8, y + 7, palette::TEAL, None);

            // Botão fechar
            fb.fill_circle(x + w as i32 - 12, y + 11, 5, palette::RED);

            let mut cy = y + 32i32;
            let lx = x + 10;
            let bw = w - 20;

            // CPU
            fb.draw_text("CPU", lx, cy, palette::CYAN, None);
            let sched = crate::modules::scheduler::get_stats();
            let cpu_pct = if sched.running > 0 { 0.4f32 } else { 0.05f32 };
            self.draw_bar(fb, lx + 32, cy, bw - 32, cpu_pct, palette::TEAL);
            fb.draw_text(&alloc::format!("{:.0}%", cpu_pct * 100.0),
                lx + bw as i32 - 28, cy, palette::WHITE, None);
            cy += 18;

            // Memória
            fb.draw_text("MEM", lx, cy, palette::PURPLE, None);
            let (used, free) = crate::memory::heap::heap_stats();
            let total = crate::memory::heap::HEAP_SIZE;
            let mem_pct = used as f32 / total as f32;
            self.draw_bar(fb, lx + 32, cy, bw - 32, mem_pct, palette::PURPLE);
            fb.draw_text(&alloc::format!("{:.0}%", mem_pct * 100.0),
                lx + bw as i32 - 28, cy, palette::WHITE, None);
            cy += 22;

            // Processos
            fb.draw_text(&alloc::format!("PROCESSOS: {}  CTX: {}",
                sched.total_processes, sched.context_switches),
                lx, cy, palette::GRAY, None);
            cy += 18;

            // P2P
            let (known, active) = crate::p2p::peer::count_peers();
            fb.draw_text(&alloc::format!("P2P: {} ativos / {} conhecidos",
                active, known), lx, cy, palette::TEAL, None);
            cy += 18;

            // Criptografia
            let crypto = crate::p2p::crypto::get_stats();
            fb.draw_text(&alloc::format!("SESSOES CRIPTO: {}  MSGS: {}",
                crypto.active_sessions, crypto.total_messages),
                lx, cy, palette::CYAN, None);
            cy += 18;

            // IA
            let ia = crate::ia::get_stats();
            fb.draw_text(&alloc::format!("IA: {} inf  ACC: {}%",
                ia.inferences_total, ia.model_accuracy),
                lx, cy, palette::GREEN, None);
            cy += 18;

            // Segurança
            let sec = crate::security::sandbox::get_stats();
            fb.draw_text(&alloc::format!("SEC: {} sandbox  {} violacoes",
                sec.active_sandboxes, sec.total_violations),
                lx, cy, if sec.total_violations > 0 { palette::ORANGE } else { palette::GREEN },
                None);
            cy += 18;

            // TmpFS
            let fs_locked = crate::modules::tmpfs::TMPFS.lock();
            let fs_s = fs_locked.fs_stats();
            drop(fs_locked);
            fb.draw_text(&alloc::format!("FS: {} inodes  {} KB",
                fs_s.total_inodes, fs_s.total_bytes / 1024),
                lx, cy, palette::GRAY, None);
            cy += 18;

            // Uptime
            let uptime_s = tick / 1000;
            fb.draw_text(&alloc::format!("UPTIME: {}s  FRAMES: {}",
                uptime_s, crate::ui::UI_STATE.lock().frames_rendered),
                lx, cy, palette::GRAY, None);

            // Sugestões da IA
            let suggestions = crate::ia::suggest::get_suggestions();
            if !suggestions.is_empty() {
                cy += 22;
                fb.draw_rect_border(Rect::new(lx - 2, cy - 2, bw + 4, 14 + suggestions.len() as u32 * 12),
                    palette::ORANGE, 1);
                fb.draw_text("! SUGESTOES IA:", lx, cy, palette::ORANGE, None);
                cy += 12;
                for s in suggestions.iter().take(2) {
                    fb.draw_text(&s.title[..s.title.len().min(36)],
                        lx, cy, palette::WHITE, None);
                    cy += 12;
                }
            }
        });
    }

    fn draw_bar(&self, fb: &mut super::render::Framebuffer,
                x: i32, y: i32, w: u32, val: f32, color: Color) {
        fb.fill_rect(Rect::new(x, y, w, 10), Color::rgb(15, 25, 45));
        fb.draw_rect_border(Rect::new(x, y, w, 10), Color::rgb(0, 60, 50), 1);
        let fill = (w as f32 * val.clamp(0.0, 1.0)) as u32;
        if fill > 0 {
            fb.fill_rect(Rect::new(x + 1, y + 1, fill.saturating_sub(2).max(1), 8), color);
        }
    }

    /// Tick do shell — atualiza display
    pub fn tick(&mut self, current_tick: u64) {
        self.tick = current_tick;
        if !self.initialized { return; }

        // Renderiza componentes do shell diretamente no framebuffer
        self.render_wallpaper();
        self.render_taskbar();
        self.render_system_monitor(current_tick);

        // Expira notificações antigas
        self.notifications.retain(|(_, exp)| current_tick < *exp);
    }

    pub fn open_app(&mut self, app_id: u64) {
        if let Some(app) = self.apps.iter_mut().find(|a| a.id == app_id) {
            app.open = true;
            crate::serial_println!("[UI][SHELL] App aberto: {}", app.name);
        }
    }
}

static SHELL: Spinlock<DesktopShell> = Spinlock::new(DesktopShell::new());

pub fn init() {
    let mut shell = SHELL.lock();

    // Cria surfaces
    shell.wallpaper_surface = Some(compositor::create_surface(
        "wallpaper",
        Rect::new(0, 0, SCREEN_WIDTH, DESKTOP_H),
        Layer::Background,
        0,
    ));

    shell.taskbar_surface = Some(compositor::create_surface(
        "taskbar",
        Rect::new(0, DESKTOP_H as i32, SCREEN_WIDTH, TASKBAR_HEIGHT),
        Layer::Overlay,
        0,
    ));

    shell.setup_apps();
    shell.initialized = true;

    crate::serial_println!("[UI][SHELL] Desktop shell inicializado");
    crate::serial_println!("[UI][SHELL] {} apps registrados", shell.apps.len());
}

pub fn tick(current_tick: u64) {
    SHELL.lock().tick(current_tick);
}

pub fn open_app(id: u64) {
    SHELL.lock().open_app(id);
}
