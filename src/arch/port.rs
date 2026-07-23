// ============================================================
// SOC-D Kernel — Acesso a Portas de I/O
// ============================================================
//
// Em x86, além da memória, o hardware expõe "portas" de I/O
// acessadas via instruções especiais IN/OUT.
// Exemplos: porta 0x60 (teclado PS/2), 0x3F8 (serial COM1)
//
// A crate x86_64 já fornece Port<T>, mas este módulo
// encapsula helpers específicos do SOC-D.
// ============================================================

use x86_64::instructions::port::Port;

/// Lê um byte de uma porta de I/O
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let mut p = Port::new(port);
    p.read()
}

/// Escreve um byte em uma porta de I/O
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    let mut p: Port<u8> = Port::new(port);
    p.write(value);
}

/// Pequeno delay usando I/O — útil para hardware antigo que
/// precisa de tempo para processar comandos
#[inline]
pub unsafe fn io_wait() {
    // Porta 0x80 é usada tradicionalmente para delays de I/O
    // (normalmente usada pelo BIOS para POST codes)
    outb(0x80, 0);
}
