// ============================================================
// SOC-D Kernel — Shell Serial Avançado (Fase 6.4)
// ============================================================
//
// Funcionalidades:
//   - Auto-complete com Tab (single e duplo-Tab para listar)
//   - Histórico de 50 comandos persistido no TmpFS
//   - Cursor movível com setas ← →
//   - Home/End para início/fim da linha
//   - Ctrl+A / Ctrl+E (compat. readline)
//   - Ctrl+W — apaga palavra anterior
//   - Ctrl+K — apaga até ao fim da linha
//   - Ctrl+U — apaga linha inteira
//   - Ctrl+R — pesquisa no histórico
//   - Prompt dinâmico com info do sistema
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    format,
};
use spinning_top::Spinlock;

// ─── Constantes ANSI ─────────────────────────────────────────
const ESC:          u8   = 0x1B;
const ANSI_RESET:  &str = "\x1B[0m";
const ANSI_BOLD:   &str = "\x1B[1m";
const ANSI_GREEN:  &str = "\x1B[32m";
const ANSI_CYAN:   &str = "\x1B[36m";
const ANSI_YELLOW: &str = "\x1B[33m";
const ANSI_RED:    &str = "\x1B[31m";
const ANSI_DIM:    &str = "\x1B[2m";

// ─── Lista completa de comandos (para auto-complete) ──────────
const COMMANDS: &[&str] = &[
    "help", "version", "status", "mem", "ps", "sched",
    "ls", "cat", "modules", "sandbox", "reboot", "clear",
    "exec", "kill", "ct", "dag", "sync", "devices",
    "handoff", "clipboard", "mobile", "theme", "ar",
    "cogn", "monitor", "top", "test", "threat", "privacy",
    "p2p", "peers", "ia", "suggest", "edge", "wasm",
    "xr", "quantum", "net", "syscall", "ui", "arm", "pci",
];

// ─── Estado do Shell ─────────────────────────────────────────
struct ShellState {
    /// Buffer da linha actual
    line_buf:       String,
    /// Posição do cursor dentro do buffer (0 = início)
    cursor_pos:     usize,
    /// Histórico de comandos
    history:        Vec<String>,
    /// Índice actual no histórico (None = linha nova)
    hist_idx:       Option<usize>,
    /// Estado da máquina de estados ANSI
    ansi_state:     AnsiState,
    /// Substate para sequências de 3+ bytes
    ansi_param:     u8,
    /// Shell foi inicializado
    initialized:    bool,
    /// Modo de pesquisa reversa (Ctrl+R)
    search_mode:    bool,
    /// Texto de pesquisa
    search_buf:     String,
    /// Último Tab pressionado (para duplo-Tab)
    last_was_tab:   bool,
    /// Contagem de comandos executados nesta sessão
    cmd_count:      u64,
}

#[derive(PartialEq)]
enum AnsiState {
    Normal,
    Esc,
    Bracket,
    BracketNum, // depois de [ e um dígito (ex: [3~)
}

impl ShellState {
    const fn new() -> Self {
        Self {
            line_buf:     String::new(),
            cursor_pos:   0,
            history:      Vec::new(),
            hist_idx:     None,
            ansi_state:   AnsiState::Normal,
            ansi_param:   0,
            initialized:  false,
            search_mode:  false,
            search_buf:   String::new(),
            last_was_tab: false,
            cmd_count:    0,
        }
    }
}

static SHELL: Spinlock<ShellState> = Spinlock::new(ShellState::new());

// ─── API Pública ─────────────────────────────────────────────

/// Exibe o banner e o primeiro prompt
pub fn print_welcome() {
    serial_raw("\r\n");
    serial_raw(&format!("{}{}╔══════════════════════════════════════════════╗{}\r\n",
        ANSI_BOLD, ANSI_CYAN, ANSI_RESET));
    serial_raw(&format!("{}{}║   SOC-D Shell v0.2 — Tab:complete ↑↓:hist  ║{}\r\n",
        ANSI_BOLD, ANSI_CYAN, ANSI_RESET));
    serial_raw(&format!("{}{}╚══════════════════════════════════════════════╝{}\r\n",
        ANSI_BOLD, ANSI_CYAN, ANSI_RESET));
    serial_raw("\r\n");

    // Carrega histórico do TmpFS
    load_history();

    SHELL.lock().initialized = true;
    print_prompt();
}

/// Chamado no kernel_loop — processa bytes disponíveis na UART
pub fn tick() {
    loop {
        if !crate::drivers::serial::data_ready() { break; }
        let byte = crate::drivers::serial::read_byte();
        process_byte(byte);
    }
}

// ─── Persistência do Histórico ────────────────────────────────

const HISTORY_PATH: &str = "/sys/shell_history.txt";
const MAX_HISTORY:  usize = 50;

fn save_history() {
    let sh = SHELL.lock();
    if sh.history.is_empty() { return; }
    let mut data = String::new();
    for line in &sh.history {
        data.push_str(line);
        data.push('\n');
    }
    drop(sh);
    let _ = crate::modules::tmpfs::write(HISTORY_PATH, data.as_bytes());
}

fn load_history() {
    if let Ok(data) = crate::modules::tmpfs::read(HISTORY_PATH) {
        if let Ok(text) = core::str::from_utf8(&data) {
            let mut sh = SHELL.lock();
            for line in text.lines() {
                let s = line.trim();
                if !s.is_empty() && sh.history.len() < MAX_HISTORY {
                    sh.history.push(s.to_string());
                }
            }
            let count = sh.history.len();
            drop(sh);
            if count > 0 {
                serial_raw(&format!("{}[shell]{} {} entradas do historico carregadas\r\n",
                    ANSI_DIM, ANSI_RESET, count));
            }
        }
    }
}

// ─── Auto-Complete ────────────────────────────────────────────

fn complete(prefix: &str) -> Vec<&'static str> {
    COMMANDS.iter()
        .copied()
        .filter(|cmd| cmd.starts_with(prefix))
        .collect()
}

fn handle_tab() {
    let (prefix, last_was_tab) = {
        let sh = SHELL.lock();
        (sh.line_buf.clone(), sh.last_was_tab)
    };

    let matches = complete(&prefix);

    match matches.len() {
        0 => {
            // Nenhuma sugestão — bell
            serial_raw("\x07");
            SHELL.lock().last_was_tab = false;
        }
        1 => {
            // Completa directamente
            let completion = matches[0];
            let suffix = &completion[prefix.len()..];
            {
                let mut sh = SHELL.lock();
                sh.line_buf.push_str(suffix);
                sh.cursor_pos = sh.line_buf.len();
                sh.last_was_tab = false;
            }
            serial_raw(suffix);
            serial_raw(" "); // espaço após completar
            SHELL.lock().line_buf.push(' ');
            SHELL.lock().cursor_pos += 1;
        }
        _ => {
            if last_was_tab {
                // Duplo-Tab: lista todas as opções
                serial_raw("\r\n");
                let mut line = String::new();
                for m in &matches {
                    line.push_str(&format!("  {:<12}", m));
                    if line.len() > 60 { serial_raw(&line); serial_raw("\r\n"); line.clear(); }
                }
                if !line.is_empty() { serial_raw(&line); serial_raw("\r\n"); }
                print_prompt();
                let buf = SHELL.lock().line_buf.clone();
                serial_raw(&buf);
                SHELL.lock().last_was_tab = false;
            } else {
                // Primeiro Tab: completa o prefixo comum
                let common = longest_common_prefix(&matches);
                if common.len() > prefix.len() {
                    let suffix = &common[prefix.len()..];
                    {
                        let mut sh = SHELL.lock();
                        sh.line_buf.push_str(suffix);
                        sh.cursor_pos = sh.line_buf.len();
                    }
                    serial_raw(suffix);
                }
                // Bell para indicar que há mais opções
                serial_raw("\x07");
                SHELL.lock().last_was_tab = true;
            }
        }
    }
}

fn longest_common_prefix(words: &[&str]) -> String {
    if words.is_empty() { return String::new(); }
    let first = words[0];
    let mut len = first.len();
    for word in &words[1..] {
        len = len.min(
            first.chars().zip(word.chars())
                .take_while(|(a, b)| a == b)
                .count()
        );
    }
    first[..len].to_string()
}

// ─── Processamento de Bytes ───────────────────────────────────

fn process_byte(byte: u8) {
    // Modo de pesquisa reversa (Ctrl+R)
    {
        let sh = SHELL.lock();
        if sh.search_mode {
            drop(sh);
            handle_search_byte(byte);
            return;
        }
    }

    let mut sh = SHELL.lock();

    match sh.ansi_state {
        AnsiState::Normal => {
            sh.last_was_tab = sh.last_was_tab && (byte == b'\t');
            match byte {
                b'\r' | b'\n' => {
                    drop(sh);
                    serial_raw("\r\n");
                    handle_enter();
                }
                0x7F | 0x08 => {
                    // Backspace
                    if sh.cursor_pos > 0 {
                        let pos = sh.cursor_pos - 1;
                        sh.line_buf.remove(pos);
                        sh.cursor_pos = pos;
                        drop(sh);
                        redraw_from_cursor(pos);
                    }
                }
                b'\t' => {
                    drop(sh);
                    handle_tab();
                }
                ESC => {
                    sh.ansi_state = AnsiState::Esc;
                    sh.last_was_tab = false;
                }
                0x01 => { // Ctrl+A — início da linha
                    let pos = sh.cursor_pos;
                    sh.cursor_pos = 0;
                    drop(sh);
                    if pos > 0 { serial_raw(&format!("\x1B[{}D", pos)); }
                }
                0x05 => { // Ctrl+E — fim da linha
                    let len = sh.line_buf.len();
                    let pos = sh.cursor_pos;
                    sh.cursor_pos = len;
                    drop(sh);
                    if pos < len { serial_raw(&format!("\x1B[{}C", len - pos)); }
                }
                0x0B => { // Ctrl+K — apaga até ao fim
                    let pos = sh.cursor_pos;
                    sh.line_buf.truncate(pos);
                    drop(sh);
                    serial_raw("\x1B[K"); // clear to end of line
                }
                0x15 => { // Ctrl+U — apaga linha inteira
                    let pos = sh.cursor_pos;
                    sh.line_buf.clear();
                    sh.cursor_pos = 0;
                    drop(sh);
                    if pos > 0 { serial_raw(&format!("\x1B[{}D", pos)); }
                    serial_raw("\x1B[K");
                }
                0x17 => { // Ctrl+W — apaga palavra anterior
                    let pos = sh.cursor_pos;
                    if pos > 0 {
                        let buf = sh.line_buf.clone();
                        let word_start = buf[..pos].rfind(' ')
                            .map(|i| i + 1).unwrap_or(0);
                        let removed = pos - word_start;
                        sh.line_buf.replace_range(word_start..pos, "");
                        sh.cursor_pos = word_start;
                        drop(sh);
                        serial_raw(&format!("\x1B[{}D", removed));
                        serial_raw("\x1B[K");
                        let buf2 = SHELL.lock().line_buf[word_start..].to_string();
                        serial_raw(&buf2);
                        if !buf2.is_empty() {
                            serial_raw(&format!("\x1B[{}D", buf2.len()));
                        }
                    }
                }
                0x03 => { // Ctrl+C
                    sh.line_buf.clear();
                    sh.cursor_pos = 0;
                    sh.hist_idx = None;
                    drop(sh);
                    serial_raw("^C\r\n");
                    print_prompt();
                }
                0x04 => { // Ctrl+D
                    drop(sh);
                    serial_raw(&format!("\r\n{}[INFO]{} Use 'reboot' para reiniciar.\r\n",
                        ANSI_YELLOW, ANSI_RESET));
                    print_prompt();
                }
                0x0C => { // Ctrl+L — limpa ecrã
                    sh.line_buf.clear();
                    sh.cursor_pos = 0;
                    drop(sh);
                    serial_raw("\x1B[2J\x1B[H");
                    print_prompt();
                }
                0x12 => { // Ctrl+R — pesquisa reversa
                    sh.search_mode = true;
                    sh.search_buf.clear();
                    drop(sh);
                    serial_raw(&format!("\r\n{}(pesquisa-reversa){} '': ",
                        ANSI_YELLOW, ANSI_RESET));
                }
                b' '..=b'~' => {
                    // Caractere imprimível — inserir na posição do cursor
                    if sh.line_buf.len() < 512 {
                        let pos = sh.cursor_pos;
                        sh.line_buf.insert(pos, byte as char);
                        sh.cursor_pos = pos + 1;
                        let suffix = sh.line_buf[pos..].to_string();
                        drop(sh);
                        serial_raw(&suffix);
                        let suf_len = suffix.len();
                        if suf_len > 1 {
                            serial_raw(&format!("\x1B[{}D", suf_len - 1));
                        }
                    }
                }
                _ => {}
            }
        }
        AnsiState::Esc => {
            if byte == b'[' {
                sh.ansi_state = AnsiState::Bracket;
                sh.ansi_param = 0;
            } else {
                sh.ansi_state = AnsiState::Normal;
            }
        }
        AnsiState::Bracket => {
            sh.ansi_state = AnsiState::Normal;
            match byte {
                b'A' => { drop(sh); history_up(); }   // ↑
                b'B' => { drop(sh); history_down(); } // ↓
                b'C' => {                              // →
                    let pos = sh.cursor_pos;
                    let len = sh.line_buf.len();
                    if pos < len {
                        sh.cursor_pos = pos + 1;
                        drop(sh);
                        serial_raw("\x1B[C");
                    }
                }
                b'D' => {                              // ←
                    let pos = sh.cursor_pos;
                    if pos > 0 {
                        sh.cursor_pos = pos - 1;
                        drop(sh);
                        serial_raw("\x1B[D");
                    }
                }
                b'H' | b'1' => {                      // Home
                    let pos = sh.cursor_pos;
                    sh.cursor_pos = 0;
                    drop(sh);
                    if pos > 0 { serial_raw(&format!("\x1B[{}D", pos)); }
                }
                b'F' | b'4' => {                      // End
                    let len = sh.line_buf.len();
                    let pos = sh.cursor_pos;
                    sh.cursor_pos = len;
                    drop(sh);
                    if pos < len { serial_raw(&format!("\x1B[{}C", len - pos)); }
                }
                b'3' => {                              // Delete (começa com [3)
                    sh.ansi_state = AnsiState::BracketNum;
                    sh.ansi_param = 3;
                }
                _ => {}
            }
        }
        AnsiState::BracketNum => {
            sh.ansi_state = AnsiState::Normal;
            if byte == b'~' && sh.ansi_param == 3 {
                // Delete key — apaga caractere à direita do cursor
                let pos = sh.cursor_pos;
                if pos < sh.line_buf.len() {
                    sh.line_buf.remove(pos);
                    let suffix = sh.line_buf[pos..].to_string();
                    drop(sh);
                    serial_raw("\x1B[K");
                    serial_raw(&suffix);
                    if !suffix.is_empty() {
                        serial_raw(&format!("\x1B[{}D", suffix.len()));
                    }
                }
            }
        }
    }
}

fn handle_search_byte(byte: u8) {
    let mut sh = SHELL.lock();
    match byte {
        b'\r' | b'\n' => {
            // Confirma pesquisa — usa a linha encontrada
            sh.search_mode = false;
            let found = find_in_history(&sh.search_buf, &sh.history);
            if let Some(cmd) = found {
                sh.line_buf = cmd.to_string();
                sh.cursor_pos = sh.line_buf.len();
                let line = sh.line_buf.clone();
                drop(sh);
                serial_raw("\r\n");
                print_prompt();
                serial_raw(&line);
            } else {
                sh.line_buf.clear();
                sh.cursor_pos = 0;
                drop(sh);
                serial_raw("\r\n");
                print_prompt();
            }
        }
        0x03 | ESC => {
            sh.search_mode = false;
            sh.search_buf.clear();
            drop(sh);
            serial_raw("\r\n");
            print_prompt();
        }
        0x7F | 0x08 => {
            sh.search_buf.pop();
            let buf = sh.search_buf.clone();
            let result = find_in_history(&buf, &sh.history)
                .unwrap_or("").to_string();
            drop(sh);
            serial_raw(&format!("\r{}(pesquisa-reversa){} '{}': {}",
                ANSI_YELLOW, ANSI_RESET, buf, result));
        }
        b' '..=b'~' => {
            sh.search_buf.push(byte as char);
            let buf = sh.search_buf.clone();
            let result = find_in_history(&buf, &sh.history)
                .unwrap_or("").to_string();
            drop(sh);
            serial_raw(&format!("\r{}(pesquisa-reversa){} '{}': {}",
                ANSI_YELLOW, ANSI_RESET, buf, result));
        }
        _ => {}
    }
}

fn find_in_history<'a>(query: &str, history: &'a [String]) -> Option<&'a str> {
    history.iter().rev()
        .find(|s| s.contains(query))
        .map(|s| s.as_str())
}

fn redraw_from_cursor(cursor_pos: usize) {
    let sh = SHELL.lock();
    let suffix = sh.line_buf[cursor_pos..].to_string();
    drop(sh);
    // Move cursor um passo para trás, apaga até ao fim, reescreve o resto
    serial_raw("\x08\x1B[K");
    serial_raw(&suffix);
    if !suffix.is_empty() {
        serial_raw(&format!("\x1B[{}D", suffix.len()));
    }
}

fn handle_enter() {
    let cmd = {
        let mut sh = SHELL.lock();
        let cmd = sh.line_buf.trim().to_string();
        if !cmd.is_empty() {
            if sh.history.last().map(|s| s.as_str()) != Some(&cmd) {
                if sh.history.len() >= MAX_HISTORY { sh.history.remove(0); }
                sh.history.push(cmd.clone());
            }
        }
        sh.line_buf.clear();
        sh.cursor_pos = 0;
        sh.hist_idx = None;
        sh.last_was_tab = false;
        sh.cmd_count += 1;
        cmd
    };

    if !cmd.is_empty() {
        crate::drivers::keyboard::execute_command_serial(&cmd);
        // Persiste histórico a cada 5 comandos
        if SHELL.lock().cmd_count % 5 == 0 {
            save_history();
        }
    }
    print_prompt();
}

fn history_up() {
    let mut sh = SHELL.lock();
    if sh.history.is_empty() { return; }
    let new_idx = match sh.hist_idx {
        None => sh.history.len() - 1,
        Some(0) => 0,
        Some(i) => i - 1,
    };
    sh.hist_idx = Some(new_idx);
    let entry = sh.history[new_idx].clone();
    let old_len = sh.line_buf.len();
    let old_pos = sh.cursor_pos;
    sh.line_buf = entry.clone();
    sh.cursor_pos = entry.len();
    drop(sh);
    // Move para início da linha, apaga, reescreve
    if old_pos > 0 { serial_raw(&format!("\x1B[{}D", old_pos)); }
    serial_raw("\x1B[K");
    serial_raw(&entry);
}

fn history_down() {
    let mut sh = SHELL.lock();
    if sh.history.is_empty() { return; }
    let old_pos = sh.cursor_pos;
    match sh.hist_idx {
        None | Some(0) => {
            sh.hist_idx = None;
            let old = sh.line_buf.clone();
            sh.line_buf.clear();
            sh.cursor_pos = 0;
            drop(sh);
            if old_pos > 0 { serial_raw(&format!("\x1B[{}D", old_pos)); }
            serial_raw("\x1B[K");
        }
        Some(i) => {
            let new_idx = i + 1;
            if new_idx >= sh.history.len() {
                sh.hist_idx = None;
                let old = sh.line_buf.clone();
                sh.line_buf.clear();
                sh.cursor_pos = 0;
                drop(sh);
                if old_pos > 0 { serial_raw(&format!("\x1B[{}D", old_pos)); }
                serial_raw("\x1B[K");
            } else {
                sh.hist_idx = Some(new_idx);
                let entry = sh.history[new_idx].clone();
                sh.line_buf = entry.clone();
                sh.cursor_pos = entry.len();
                drop(sh);
                if old_pos > 0 { serial_raw(&format!("\x1B[{}D", old_pos)); }
                serial_raw("\x1B[K");
                serial_raw(&entry);
            }
        }
    }
}

// ─── Prompt Dinâmico ─────────────────────────────────────────

fn print_prompt() {
    // Mostra heap% no prompt se > 50%
    let (used, free) = crate::memory::heap::heap_stats();
    let total = crate::memory::heap::HEAP_SIZE;
    let heap_pct = ((used as u64 * 100) / total as u64) as u8;

    if heap_pct > 50 {
        serial_raw(&format!("{}{}socd{}[heap:{}%]{}>{} ",
            ANSI_BOLD, ANSI_GREEN, ANSI_YELLOW, heap_pct, ANSI_GREEN, ANSI_RESET));
    } else {
        serial_raw(&format!("{}{}socd>{} ", ANSI_BOLD, ANSI_GREEN, ANSI_RESET));
    }
}

// ─── Utilitários ─────────────────────────────────────────────

fn serial_raw(s: &str) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        let _ = crate::drivers::serial::SERIAL1.lock().write_str(s);
    });
}

pub fn cmd_output(s: &str) {
    serial_raw(&format!("  {}\r\n", s));
}

pub fn cmd_error(s: &str) {
    serial_raw(&format!("  {}[ERRO]{} {}\r\n", ANSI_RED, ANSI_RESET, s));
}
