// ============================================================
// SOC-D Kernel — Drivers
// ============================================================

pub mod vga;            // Output de texto VGA 80x25
pub mod vga_dashboard;  // Dashboard visual na janela QEMU
pub mod serial;         // UART serial (debug + input)
pub mod keyboard;       // PS/2 Teclado
pub mod serial_shell;   // Shell interativo via serial
