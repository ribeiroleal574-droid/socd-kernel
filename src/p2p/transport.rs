extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D — Camada de Transporte P2P
// ============================================================
// Fase 2: Simulada em memória (sem driver de rede real)
// Fase 3: Integração com virtio-net / e1000 driver
//         Suporte TCP/UDP via stack lwIP ou smoltcp
// ============================================================

use spinning_top::Spinlock;

/// Pacote da camada de transporte
#[derive(Debug, Clone)]
pub struct TransportPacket {
    pub src_node: [u8; 32],
    pub dst_node: [u8; 32],
    pub payload: Vec<u8>,
    pub packet_id: u64,
}

/// Fila de transmissão simulada
pub struct TransportLayer {
    pub tx_queue: Vec<TransportPacket>,
    pub rx_queue: Vec<TransportPacket>,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    next_id: u64,
}

impl TransportLayer {
    const fn new() -> Self {
        Self {
            tx_queue: Vec::new(),
            rx_queue: Vec::new(),
            packets_sent: 0,
            packets_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            next_id: 1,
        }
    }

    /// Enfileira pacote para transmissão
    pub fn send(&mut self, dst: [u8; 32], payload: Vec<u8>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let len = payload.len();
        self.tx_queue.push(TransportPacket {
            src_node: crate::p2p::node::get_node_id(),
            dst_node: dst,
            payload,
            packet_id: id,
        });
        self.packets_sent += 1;
        self.bytes_sent += len as u64;
        id
    }

    /// Simula recepção de pacote (Fase 3: vem do driver de rede)
    pub fn simulate_receive(&mut self, src: [u8; 32], payload: Vec<u8>) {
        let len = payload.len();
        let id = self.next_id;
        self.next_id += 1;
        self.rx_queue.push(TransportPacket {
            src_node: src,
            dst_node: crate::p2p::node::get_node_id(),
            payload,
            packet_id: id,
        });
        self.packets_received += 1;
        self.bytes_received += len as u64;
    }

    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.packets_sent, self.packets_received,
         self.bytes_sent, self.bytes_received)
    }
}

static TRANSPORT: Spinlock<TransportLayer> = Spinlock::new(TransportLayer::new());

pub fn init() {
    crate::serial_println!("[P2P][TRANSPORT] Camada de transporte pronta (simulada)");
    crate::serial_println!("[P2P][TRANSPORT] Fase 3: integrar smoltcp/virtio-net");
}

pub fn send(dst: [u8; 32], payload: Vec<u8>) -> u64 {
    TRANSPORT.lock().send(dst, payload)
}

pub fn get_stats() -> (u64, u64, u64, u64) {
    TRANSPORT.lock().stats()
}
