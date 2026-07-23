// ============================================================
// SOC-D Kernel — Driver Serial UART 16550
// ============================================================
//
// A porta serial é o principal canal de debug do kernel.
// O QEMU pode redirecionar a saída serial para o terminal
// do host com: -serial stdio
//
// Porta COM1: base 0x3F8
// Baud rate: 115200 (configurado no init)
//
// Vantagem sobre VGA: não depende de display, funciona
// antes do VGA inicializar, e pode ser capturado em CI/testes.
// ============================================================

use lazy_static::lazy_static;
use spinning_top::Spinlock;
use uart_16550::SerialPort;

lazy_static! {
    // Porta serial COM1 (endereço base 0x3F8)
    pub static ref SERIAL1: Spinlock<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Spinlock::new(serial_port)
    };
}

/// Inicializa a porta serial COM1.
pub fn init() {
    // Força a inicialização lazy do SERIAL1
    let _ = SERIAL1.lock();
}

// ─── Macros de Print Serial ──────────────────────────────────────────────────

/// Imprime na serial sem newline
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::drivers::serial::_serial_print(format_args!($($arg)*));
    };
}

/// Imprime na serial com newline
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (
        $crate::serial_print!(concat!($fmt, "\n"), $($arg)*)
    );
}

#[doc(hidden)]
pub fn _serial_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Falha ao escrever na serial");
    });
}

// ─── Leitura Serial (para o shell interativo) ────────────────────────────────

/// Verifica se há um byte disponível na UART (line status register bit 0)
pub fn data_ready() -> bool {
    unsafe {
        let lsr: u8 = x86_64::instructions::port::Port::<u8>::new(0x3F8 + 5).read();
        (lsr & 0x01) != 0
    }
}

/// Lê um byte da UART (não bloqueia — verificar data_ready() primeiro)
pub fn read_byte() -> u8 {
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3F8).read()
    }
}
