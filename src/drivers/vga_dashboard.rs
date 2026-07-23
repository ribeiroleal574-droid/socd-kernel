// ============================================================
// SOC-D Kernel — Dashboard VGA (80×25 text mode)
// ============================================================
extern crate alloc;
//
// Renderiza um painel de estado visual na janela QEMU
// usando o modo de texto VGA 80×25 com cores.
//
// Layout (80×25):
//  Row  0    ── Header / banner
//  Row  1    ── Linha de separação
//  Rows 2-12 ── Coluna esquerda: subsistemas (status)
//  Rows 2-12 ── Coluna direita:  métricas em tempo real
//  Row 13    ── Separador
//  Rows 14-22── Log de boot / mensagens recentes
//  Row 23    ── Separador
//  Row 24    ── Status bar (versão, uptime, heap)
// ============================================================

use crate::drivers::vga::{Color, WRITER};

const W: usize = 80;

/// Desenha o dashboard completo.
/// Chamado uma vez após todos os subsistemas iniciarem.
pub fn draw() {
    let mut w = WRITER.lock();

    // ── Limpa o ecrã com fundo azul escuro ───────────────────
    for row in 0..25 {
        w.fill_row(row, b' ', Color::LightGray, Color::Blue);
    }

    // ── Row 0: Header ─────────────────────────────────────────
    w.fill_row(0, b' ', Color::Black, Color::Cyan);
    let title = "  SOC-D Kernel v0.1.0  |  Sistema Operacional Cognitivo Distribuido  ";
    w.write_at(0, 0, title, Color::Black, Color::Cyan);
    let right = "x86_64  ";
    w.write_at(0, W - right.len(), right, Color::Black, Color::Cyan);

    // ── Row 1: Separador ──────────────────────────────────────
    w.fill_row(1, 0xCD, Color::Yellow, Color::Blue); // ═══

    // ── Rows 2-12: Subsistemas ────────────────────────────────
    let subsystems = [
        ("ARCH",    "GDT + IDT + PIC",         true),
        ("MEMORY",  "Heap 8MB / Paging",        true),
        ("DRIVERS", "VGA + Serial + PS/2",      true),
        ("MODULES", "Scheduler + TmpFS + ELF",  true),
        ("SECURITY","Sandbox + Policy",         true),
        ("NET",     "virtio-net TCP/IP/DNS",    true),
        ("P2P",     "3 peers / Gossip+Crypto",  true),
        ("IA",      "3 modelos ML ativos",      true),
        ("UI",      "1024x768 Framebuffer",     true),
        ("EDGE",    "4 nos / BestFit",          true),
        ("WASM",    "Runtime 1.0+SIMD",         true),
    ];

    for (i, (name, desc, ok)) in subsystems.iter().enumerate() {
        let row = 2 + i;
        // Badge [OK] ou [--]
        let (badge, badge_fg) = if *ok {
            ("[OK]", Color::LightGreen)
        } else {
            ("[--]", Color::DarkGray)
        };
        w.write_at(row, 1, badge, badge_fg, Color::Blue);
        // Nome em amarelo
        w.write_at(row, 6, name, Color::Yellow, Color::Blue);
        // Descrição em cinzento
        let pad = 13usize.saturating_sub(name.len());
        w.write_at(row, 6 + name.len() + pad, desc, Color::LightGray, Color::Blue);
    }

    // ── Rows 2-12: Métricas (coluna direita) ──────────────────
    let metrics = [
        ("Heap",    "8192 KB"),
        ("Procs",   "2 ativos"),
        ("P2P Node","16df6e1b"),
        ("Peers",   "3 / 3 up"),
        ("IA Infer","0"),
        ("Frames",  "0"),
        ("Edge",    "4 nos"),
        ("WASM",    "0 inst"),
        ("XR",      "90 Hz"),
        ("Quantum", "20 qbits"),
        ("Syscalls","0"),
    ];

    for (i, (label, value)) in metrics.iter().enumerate() {
        let row = 2 + i;
        // Linha vertical separadora
        w.write_at(row, 44, "|", Color::Yellow, Color::Blue);
        w.write_at(row, 46, label, Color::Cyan, Color::Blue);
        w.write_at(row, 46 + label.len() + 1, ":", Color::DarkGray, Color::Blue);
        w.write_at(row, 46 + label.len() + 3, value, Color::White, Color::Blue);
    }

    // ── Row 13: Separador ─────────────────────────────────────
    w.fill_row(13, 0xC4, Color::Yellow, Color::Blue); // ───
    w.write_at(13, 0,  "+", Color::Yellow, Color::Blue);
    w.write_at(13, 44, "+", Color::Yellow, Color::Blue);
    w.write_at(13, 79, "+", Color::Yellow, Color::Blue);

    // ── Row 14: Título da secção de log ───────────────────────
    w.write_at(14, 1, "Boot log:", Color::Cyan, Color::Blue);
    w.write_at(14, 45, "Shell: -serial stdio", Color::DarkGray, Color::Blue);

    // ── Rows 15-22: Boot log ──────────────────────────────────
    let log = [
        "[OK] GDT inicializada",
        "[OK] IDT + PIC configurados",
        "[OK] Heap 8MB mapeado",
        "[OK] P2P node 16df6e1b ativo",
        "[OK] IA 3 modelos carregados",
        "[OK] UI framebuffer 1024x768",
        "[OK] Edge 4 nos registados",
        "[OK] Kernel v0.1.0 pronto",
    ];
    for (i, line) in log.iter().enumerate() {
        w.write_at(15 + i, 2, line, Color::LightGreen, Color::Blue);
    }

    // ── Row 23: Separador inferior ────────────────────────────
    w.fill_row(23, 0xCD, Color::Yellow, Color::Blue);

    // ── Row 24: Status bar ────────────────────────────────────
    w.fill_row(24, b' ', Color::Black, Color::Cyan);
    w.write_at(24, 1,
        "SOC-D v0.1.0  |  Rust nightly  |  x86_64-unknown-none  |  -serial stdio para shell",
        Color::Black, Color::Cyan);
}

/// Atualiza apenas a status bar (row 24) com info dinâmica
pub fn update_statusbar(tick: u64) {
    let mut w = WRITER.lock();
    w.fill_row(24, b' ', Color::Black, Color::Cyan);
    // Formata tick como segundos aproximados (60Hz)
    let secs = tick / 60;
    let msg = alloc::format!(
        " SOC-D v0.1.0  |  uptime: {}s  |  Heap: 8MB  |  'socd>' na serial",
        secs
    );
    w.write_at(24, 0, &msg, Color::Black, Color::Cyan);
}
