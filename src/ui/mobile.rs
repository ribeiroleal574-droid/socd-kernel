// ============================================================
// SOC-D Kernel — UI Mobile Adaptativa (Fase 4.1)
// ============================================================
//
// Sistema de UI adaptativa que ajusta o layout conforme o
// dispositivo de destino: Desktop, Mobile, Tablet, AR, TV.
//
// Princípios:
//   - Layout fluido baseado em "flex units" (não pixels fixos)
//   - Touch-first: gestos tap, swipe, pinch, rotate
//   - Orientação dinâmica: portrait ↔ landscape
//   - Temas adaptativos: light / dark / oled / high-contrast
//   - Componentes reutilizáveis entre todos os form factors
//
// Pipeline de renderização mobile:
//
//   Input (touch/gesture) → Layout Engine → Render Pass
//         ↓                      ↓               ↓
//   GestureRecognizer     FlexContainer     FrameBuffer
//         ↓                      ↓               ↓
//   EventQueue            WidgetTree        Compositor
//
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;

// ─── Form Factor ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum FormFactor {
    Desktop  { width: u32, height: u32 },
    Laptop   { width: u32, height: u32 },
    Tablet   { width: u32, height: u32, portrait: bool },
    Mobile   { width: u32, height: u32, portrait: bool },
    Tv       { width: u32, height: u32 },
    Ar,
    Vr,
}

impl FormFactor {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            FormFactor::Desktop { width, height }  => (*width, *height),
            FormFactor::Laptop  { width, height }  => (*width, *height),
            FormFactor::Tablet  { width, height, portrait } => {
                if *portrait { (*width, *height) } else { (*height, *width) }
            }
            FormFactor::Mobile  { width, height, portrait } => {
                if *portrait { (*width, *height) } else { (*height, *width) }
            }
            FormFactor::Tv      { width, height }  => (*width, *height),
            FormFactor::Ar | FormFactor::Vr        => (1832, 1920),
        }
    }

    pub fn is_touch(&self) -> bool {
        matches!(self, FormFactor::Mobile{..} | FormFactor::Tablet{..} | FormFactor::Ar | FormFactor::Vr)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FormFactor::Desktop{..} => "desktop",
            FormFactor::Laptop{..}  => "laptop",
            FormFactor::Tablet{..}  => "tablet",
            FormFactor::Mobile{..}  => "mobile",
            FormFactor::Tv{..}      => "tv",
            FormFactor::Ar          => "ar",
            FormFactor::Vr          => "vr",
        }
    }

    /// Tamanho base da fonte em pixels
    pub fn base_font_size(&self) -> u32 {
        match self {
            FormFactor::Mobile{..}  => 16,
            FormFactor::Tablet{..}  => 18,
            FormFactor::Desktop{..} | FormFactor::Laptop{..} => 14,
            FormFactor::Tv{..}      => 32,
            FormFactor::Ar | FormFactor::Vr => 24,
        }
    }

    /// Padding base em pixels
    pub fn base_padding(&self) -> u32 {
        match self {
            FormFactor::Mobile{..}  => 16,
            FormFactor::Tablet{..}  => 20,
            FormFactor::Desktop{..} | FormFactor::Laptop{..} => 8,
            FormFactor::Tv{..}      => 40,
            FormFactor::Ar | FormFactor::Vr => 12,
        }
    }
}

// ─── Tema ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Theme {
    Dark,
    Light,
    Oled,
    HighContrast,
    ArTransparent,
}

#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub background:  u32,
    pub surface:     u32,
    pub primary:     u32,
    pub accent:      u32,
    pub text:        u32,
    pub text_dim:    u32,
    pub border:      u32,
    pub danger:      u32,
    pub success:     u32,
}

impl ThemeColors {
    pub fn dark() -> Self {
        Self {
            background: 0xFF1A1A2E,
            surface:    0xFF16213E,
            primary:    0xFF0F3460,
            accent:     0xFFE94560,
            text:       0xFFEEEEEE,
            text_dim:   0xFF888888,
            border:     0xFF2A2A4A,
            danger:     0xFFFF4444,
            success:    0xFF44FF88,
        }
    }

    pub fn light() -> Self {
        Self {
            background: 0xFFF5F5F5,
            surface:    0xFFFFFFFF,
            primary:    0xFF2196F3,
            accent:     0xFFFF5722,
            text:       0xFF212121,
            text_dim:   0xFF757575,
            border:     0xFFE0E0E0,
            danger:     0xFFF44336,
            success:    0xFF4CAF50,
        }
    }

    pub fn oled() -> Self {
        Self {
            background: 0xFF000000,
            surface:    0xFF0A0A0A,
            primary:    0xFF1565C0,
            accent:     0xFF00E5FF,
            text:       0xFFFFFFFF,
            text_dim:   0xFF666666,
            border:     0xFF111111,
            danger:     0xFFFF1744,
            success:    0xFF00E676,
        }
    }

    pub fn ar_transparent() -> Self {
        Self {
            background: 0x00000000, // Totalmente transparente
            surface:    0x99000020, // Semi-transparente azul escuro
            primary:    0xCC00AAFF,
            accent:     0xFFFFDD00,
            text:       0xFFFFFFFF,
            text_dim:   0xAAFFFFFF,
            border:     0x8800AAFF,
            danger:     0xCCFF3333,
            success:    0xCC33FF88,
        }
    }
}

// ─── Layout Engine ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum LayoutDirection {
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Debug, Clone)]
pub enum Alignment {
    Start,
    Center,
    End,
    Stretch,
    SpaceBetween,
    SpaceAround,
}

#[derive(Debug, Clone)]
pub enum SizeUnit {
    /// Pixels absolutos
    Px(u32),
    /// Percentagem do contentor pai
    Pct(u32),
    /// Flex ratio (como CSS flex-grow)
    Flex(u32),
    /// Fit ao conteúdo
    Wrap,
}

#[derive(Debug, Clone)]
pub struct FlexContainer {
    pub id:        u64,
    pub direction: LayoutDirection,
    pub align:     Alignment,
    pub justify:   Alignment,
    pub width:     SizeUnit,
    pub height:    SizeUnit,
    pub padding:   u32,
    pub gap:       u32,
    pub children:  Vec<FlexContainer>,
    pub widget:    Option<MobileWidget>,
}

// ─── Widgets Mobile ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum MobileWidget {
    Text { content: String, size: u32, bold: bool },
    Button { label: String, action: String },
    Image { path: String, width: u32, height: u32 },
    Input { placeholder: String, value: String },
    Card { title: String, body: String },
    List { items: Vec<String> },
    Progress { value: u32, max: u32 },
    Toggle { label: String, checked: bool },
    StatusBar { title: String, battery: u8, time: String },
    NavBar { items: Vec<String>, active: usize },
    Notification { title: String, body: String, level: NotifLevel },
}

#[derive(Debug, Clone)]
pub enum NotifLevel { Info, Warning, Error, Success }

// ─── Gestos Touch ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Gesture {
    Tap     { x: i32, y: i32 },
    DoubleTap { x: i32, y: i32 },
    LongPress { x: i32, y: i32, duration_ms: u32 },
    Swipe   { dx: i32, dy: i32, velocity: f32 },
    Pinch   { scale: f32, cx: i32, cy: i32 },
    Rotate  { angle: f32, cx: i32, cy: i32 },
    Pan     { dx: i32, dy: i32 },
}

impl Gesture {
    pub fn as_str(&self) -> &'static str {
        match self {
            Gesture::Tap{..}       => "tap",
            Gesture::DoubleTap{..} => "double-tap",
            Gesture::LongPress{..} => "long-press",
            Gesture::Swipe{..}     => "swipe",
            Gesture::Pinch{..}     => "pinch",
            Gesture::Rotate{..}    => "rotate",
            Gesture::Pan{..}       => "pan",
        }
    }
}

// ─── Layout Adaptativo ───────────────────────────────────────

pub struct AdaptiveLayout {
    pub form_factor: FormFactor,
    pub theme:       Theme,
    pub colors:      ThemeColors,
    pub root:        Option<FlexContainer>,
    pub gestures:    Vec<Gesture>,
    pub next_id:     u64,
    pub stats:       LayoutStats,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutStats {
    pub layouts_computed: u64,
    pub gestures_handled: u64,
    pub widgets_rendered: u64,
    pub theme_changes:    u64,
}

impl AdaptiveLayout {
    pub const fn new() -> Self {
        Self {
            form_factor: FormFactor::Desktop { width: 1024, height: 768 },
            theme:       Theme::Dark,
            colors:      ThemeColors {
                background: 0xFF1A1A2E,
                surface:    0xFF16213E,
                primary:    0xFF0F3460,
                accent:     0xFFE94560,
                text:       0xFFEEEEEE,
                text_dim:   0xFF888888,
                border:     0xFF2A2A4A,
                danger:     0xFFFF4444,
                success:    0xFF44FF88,
            },
            root:   None,
            gestures: Vec::new(),
            next_id:  1,
            stats:    LayoutStats {
                layouts_computed: 0,
                gestures_handled: 0,
                widgets_rendered: 0,
                theme_changes:    0,
            },
        }
    }

    /// Adapta o layout ao novo form factor
    pub fn adapt(&mut self, ff: FormFactor) {
        let (w, h) = ff.dimensions();
        crate::serial_println!("[UI-MOBILE] Adaptando para {} ({}x{})",
            ff.as_str(), w, h);
        self.form_factor = ff.clone();
        // Ajusta tema para AR
        if ff == FormFactor::Ar || ff == FormFactor::Vr {
            self.set_theme(Theme::ArTransparent);
        }
        self.rebuild_layout();
        self.stats.layouts_computed += 1;
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.colors = match &theme {
            Theme::Dark          => ThemeColors::dark(),
            Theme::Light         => ThemeColors::light(),
            Theme::Oled          => ThemeColors::oled(),
            Theme::ArTransparent => ThemeColors::ar_transparent(),
            Theme::HighContrast  => ThemeColors::light(), // simplificado
        };
        crate::serial_println!("[UI-MOBILE] Tema: {:?}", theme);
        self.theme = theme;
        self.stats.theme_changes += 1;
    }

    /// Reconstrói o layout root para o form factor atual
    fn rebuild_layout(&mut self) {
        let padding = self.form_factor.base_padding();
        let ff_str = self.form_factor.as_str();

        // Layout raiz: coluna vertical com padding
        let root = FlexContainer {
            id: self.next_id(),
            direction: LayoutDirection::Column,
            align:     Alignment::Stretch,
            justify:   Alignment::Start,
            width:     SizeUnit::Pct(100),
            height:    SizeUnit::Pct(100),
            padding,
            gap: padding / 2,
            children: self.build_children(),
            widget: None,
        };
        self.root = Some(root);
    }

    fn build_children(&mut self) -> Vec<FlexContainer> {
        let font = self.form_factor.base_font_size();
        let mut children = Vec::new();

        // Status bar (mobile/tablet)
        if matches!(self.form_factor, FormFactor::Mobile{..} | FormFactor::Tablet{..}) {
            children.push(FlexContainer {
                id: self.next_id(), direction: LayoutDirection::Row,
                align: Alignment::Center, justify: Alignment::SpaceBetween,
                width: SizeUnit::Pct(100), height: SizeUnit::Px(44),
                padding: 8, gap: 0,
                children: Vec::new(),
                widget: Some(MobileWidget::StatusBar {
                    title: "SOC-D".to_string(),
                    battery: 85,
                    time: "09:41".to_string(),
                }),
            });
        }

        // Área de conteúdo principal
        children.push(FlexContainer {
            id: self.next_id(), direction: LayoutDirection::Column,
            align: Alignment::Stretch, justify: Alignment::Start,
            width: SizeUnit::Pct(100), height: SizeUnit::Flex(1),
            padding: 0, gap: 8,
            children: Vec::new(),
            widget: Some(MobileWidget::Card {
                title: "SOC-D Dashboard".to_string(),
                body: alloc::format!(
                    "Form factor: {} | Tema: {:?} | Font: {}px",
                    self.form_factor.as_str(), self.theme, font
                ),
            }),
        });

        // Nav bar (mobile/tablet/tv)
        if !matches!(self.form_factor, FormFactor::Desktop{..} | FormFactor::Laptop{..}) {
            children.push(FlexContainer {
                id: self.next_id(), direction: LayoutDirection::Row,
                align: Alignment::Center, justify: Alignment::SpaceAround,
                width: SizeUnit::Pct(100), height: SizeUnit::Px(56),
                padding: 0, gap: 0,
                children: Vec::new(),
                widget: Some(MobileWidget::NavBar {
                    items: alloc::vec![
                        "Home".to_string(), "Apps".to_string(),
                        "Files".to_string(), "Settings".to_string(),
                    ],
                    active: 0,
                }),
            });
        }
        children
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Processa um gesto touch
    pub fn handle_gesture(&mut self, gesture: Gesture) {
        crate::serial_println!("[UI-MOBILE] Gesto: {} {:?}", gesture.as_str(), gesture);
        self.gestures.push(gesture);
        if self.gestures.len() > 32 { self.gestures.remove(0); }
        self.stats.gestures_handled += 1;
    }

    /// Conta widgets na árvore
    fn count_widgets(node: &FlexContainer) -> u64 {
        let mut n = if node.widget.is_some() { 1 } else { 0 };
        for child in &node.children { n += Self::count_widgets(child); }
        n
    }

    pub fn widget_count(&self) -> u64 {
        self.root.as_ref().map(|r| Self::count_widgets(r)).unwrap_or(0)
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static MOBILE_UI: Spinlock<AdaptiveLayout> =
    Spinlock::new(AdaptiveLayout::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[UI-MOBILE] Motor de UI adaptativa inicializado");
    crate::serial_println!("[UI-MOBILE] Suporte: desktop | mobile | tablet | tv | ar | vr");
}

pub fn adapt(ff: FormFactor) {
    MOBILE_UI.lock().adapt(ff);
}

pub fn set_theme(theme: Theme) {
    MOBILE_UI.lock().set_theme(theme);
}

pub fn handle_gesture(g: Gesture) {
    MOBILE_UI.lock().handle_gesture(g);
}

pub fn stats() -> LayoutStats {
    MOBILE_UI.lock().stats.clone()
}

pub fn run_demo() {
    crate::serial_println!("\n[FASE4.1] === UI Mobile Adaptativa ===");

    // Demonstra adaptação para diferentes form factors
    adapt(FormFactor::Mobile { width: 1080, height: 2340, portrait: true });
    adapt(FormFactor::Tablet { width: 2048, height: 1536, portrait: false });
    adapt(FormFactor::Tv     { width: 3840, height: 2160 });
    adapt(FormFactor::Ar);

    // Volta para desktop
    adapt(FormFactor::Desktop { width: 1024, height: 768 });
    set_theme(Theme::Dark);

    // Simula gestos touch
    handle_gesture(Gesture::Tap { x: 540, y: 1200 });
    handle_gesture(Gesture::Swipe { dx: -300, dy: 0, velocity: 1.5 });
    handle_gesture(Gesture::Pinch { scale: 1.5, cx: 540, cy: 960 });

    let s = stats();
    crate::serial_println!("[FASE4.1] Layouts: {} | Gestos: {} | Temas: {}",
        s.layouts_computed, s.gestures_handled, s.theme_changes);
    crate::serial_println!("[FASE4.1] ==============================\n");
}
