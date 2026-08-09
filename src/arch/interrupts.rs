// SOC-D Kernel — IDT + Interrupt Handlers (x86_64 0.14)
extern crate alloc;

use crate::drivers::keyboard;
use crate::{println, serial_println};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spinning_top::Spinlock;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Spinlock<ChainedPics> = Spinlock::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer    = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

impl InterruptIndex {
    fn as_u8(self)    -> u8    { self as u8 }
    fn as_usize(self) -> usize { self as usize }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::arch::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("[BREAKPOINT] {:?}", stack_frame.instruction_pointer);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64,
) -> ! {
    panic!("DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();
    serial_println!("[PAGE FAULT] addr={:?} code={:?}", fault_addr, error_code);
    panic!("PAGE FAULT at {:?}", fault_addr);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64,
) {
    panic!("GPF code={:#x}\n{:#?}", error_code, stack_frame);
}

static TICK_COUNT: Spinlock<u64> = Spinlock::new(0);

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut ticks = TICK_COUNT.lock();
    *ticks += 1;
    drop(ticks);

    // Tick subsystems every N ticks
    let tick = *TICK_COUNT.lock();
    if tick % 1000 == 0 {
        crate::p2p::discovery::tick(tick);
        crate::p2p::gossip::tick(tick);
        crate::ia::tick(tick);
        crate::edge::tick(tick);
    }
    if tick % 16 == 0 {
        // ~60fps UI update
        crate::ui::shell::tick(tick);
    }
    if tick % 20 == 0 {
        // Sondagem da rede real (virtio-net) — mais frequente que os
        // outros subsistemas porque pacotes UDP P2P recebidos ficam
        // parados nos buffers RX até serem drenados.
        crate::p2p::transport::poll_receive();
    }

    // IMPORTANTE: o EOI tem de ser enviado ANTES de uma possível troca
    // de contexto. `preempt()` pode não "regressar" aqui durante vários
    // ticks (só volta quando esta tarefa for escolhida de novo) — se o
    // EOI ficasse pendente até depois disso, o PIC nunca mais deixava
    // passar interrupções de timer, travando a preempção por completo.
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }

    // Verifica preempção e, se necessário, troca de tarefa de facto
    // (troca real de pilha — ver arch::context_switch).
    crate::modules::scheduler::preempt();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    keyboard::handle_scancode(scancode);
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
}
