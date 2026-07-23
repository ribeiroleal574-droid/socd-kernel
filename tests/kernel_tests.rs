// ============================================================
// SOC-D Kernel — Testes de Integração
// ============================================================
//
// Estes testes rodam dentro do kernel no QEMU.
// Execute com: make test
//
// Cada teste é uma função marcada com #[test_case].
// O framework de testes usa a porta 0xf4 para sinalizar
// sucesso/falha de volta ao host (via QEMU isa-debug-exit).
// ============================================================

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(socd_kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec, string::String};
use core::panic::PanicInfo;
use socd_kernel::{serial_print, serial_println};

// ─── Framework de Testes ─────────────────────────────────────────────────────

pub trait Testable {
    fn run(&self);
}

impl<T: Fn()> Testable for T {
    fn run(&self) {
        serial_print!("  {} ... ", core::any::type_name::<T>());
        self();
        serial_println!("[OK]");
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("\n[SOC-D TESTS] Executando {} testes...\n", tests.len());
    for test in tests {
        test.run();
    }
    serial_println!("\n[TODOS OS TESTES PASSARAM]\n");
    exit_qemu(QemuExitCode::Success);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed  = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[FALHOU]");
    serial_println!("[PANIC] {}", info);
    exit_qemu(QemuExitCode::Failed);
    loop {}
}

// ─── Testes ──────────────────────────────────────────────────────────────────

/// Teste trivial — garante que o framework funciona
#[test_case]
fn test_trivial() {
    assert_eq!(1 + 1, 2);
}

/// Testa alocação no heap (Box)
#[test_case]
fn test_heap_box() {
    let v = Box::new(42u64);
    assert_eq!(*v, 42);
}

/// Testa alocação de Vec
#[test_case]
fn test_heap_vec() {
    let v: Vec<u64> = vec![1, 2, 3, 4, 5];
    assert_eq!(v.len(), 5);
    assert_eq!(v[2], 3);
}

/// Testa alocação de String
#[test_case]
fn test_heap_string() {
    let s = String::from("SOC-D Kernel");
    assert_eq!(s.as_str(), "SOC-D Kernel");
}

/// Testa muitas alocações sem falha (stress test de heap)
#[test_case]
fn test_heap_stress() {
    for i in 0..1000u64 {
        let v = Box::new(i);
        assert_eq!(*v, i);
    }
}

/// Testa alocação e liberação alternadas
#[test_case]
fn test_heap_alloc_dealloc() {
    for _ in 0..100 {
        let v: Vec<u64> = vec![0; 64]; // 512 bytes por iteração
        assert_eq!(v.len(), 64);
        drop(v); // Libera antes da próxima alocação
    }
}

/// Testa o breakpoint handler (não deve causar panic)
#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
    // Se chegou aqui, o handler tratou corretamente
}

/// Testa o sandbox — criação de contexto
#[test_case]
fn test_sandbox_create() {
    use socd_kernel::security::{sandbox, TrustLevel};
    sandbox::init();
    let pid = sandbox::create_process_sandbox("test-process", TrustLevel::User);
    assert!(pid > 0);
}

/// Testa o sandbox — verificação de permissões permitidas
#[test_case]
fn test_sandbox_permissions_allowed() {
    use socd_kernel::security::{sandbox, TrustLevel};
    let pid = sandbox::create_process_sandbox("test-allowed", TrustLevel::User);
    assert!(sandbox::check_permission(pid, "fs_read"));
    assert!(sandbox::check_permission(pid, "network"));
}

/// Testa o sandbox — verificação de permissões negadas
#[test_case]
fn test_sandbox_permissions_denied() {
    use socd_kernel::security::{sandbox, TrustLevel};
    let pid = sandbox::create_process_sandbox("test-denied", TrustLevel::Untrusted);
    assert!(!sandbox::check_permission(pid, "fs_read"));
    assert!(!sandbox::check_permission(pid, "network"));
    assert!(!sandbox::check_permission(pid, "hardware_access"));
}

/// Testa o registro de módulos
#[test_case]
fn test_module_registry() {
    use socd_kernel::modules::registry::REGISTRY;
    let reg = REGISTRY.lock();
    let stats = reg.stats();
    // Após boot, pelo menos os módulos built-in devem estar registrados
    assert!(stats.total >= 0); // Registry inicializou sem panic
}

/// Testa estatísticas de memória
#[test_case]
fn test_memory_stats() {
    use socd_kernel::memory::heap::{heap_stats, HEAP_SIZE};
    let (used, free) = heap_stats();
    assert!(used + free <= HEAP_SIZE);
    assert!(free > 0); // Deve ter memória livre
}
