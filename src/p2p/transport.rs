extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D — Camada de Transporte P2P (UDP real)
// ============================================================
// Envia/recebe pacotes P2P a sério via UDP/IPv4/Ethernet sobre o
// driver virtio-net real (ver net::virtio_real), em vez de apenas
// enfileirar em memória.
//
// Formato do datagrama UDP (payload, depois do cabeçalho UDP):
//   [ node_id do remetente : 32 bytes ][ payload da aplicação P2P ]
//
// O node_id vai à frente porque, ao contrário de TCP, UDP não tem
// "ligação" — quem recebe precisa de saber já no próprio datagrama
// quem o node lógico remetente é (o IP:porta de origem por si só não
// chega, já que a identidade P2P é o node_id de 32 bytes, não o IP).
// ============================================================

use spinning_top::Spinlock;
use crate::net::{MacAddr, Ipv4Addr};
use crate::net::ethernet::{EthernetFrame, Ipv4Packet, UdpPacket, ETH_TYPE_IPV4, IP_PROTO_UDP, arp_lookup};

/// Porta UDP usada pelo protocolo P2P (mesma da descoberta mDNS-like)
pub const P2P_UDP_PORT: u16 = crate::p2p::discovery::SOCD_PORT;

/// Pacote da camada de transporte
#[derive(Debug, Clone)]
pub struct TransportPacket {
    pub src_node: [u8; 32],
    pub dst_node: [u8; 32],
    pub payload: Vec<u8>,
    pub packet_id: u64,
}

pub struct TransportLayer {
    pub tx_queue: Vec<TransportPacket>,
    pub rx_queue: Vec<TransportPacket>,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// Pacotes que não foram enviados a sério na rede porque o
    /// destino não tinha endereço IP conhecido no PeerTable (ex:
    /// peers de demonstração sem endereço real resolvível)
    pub wire_send_failures: u64,
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
            wire_send_failures: 0,
            next_id: 1,
        }
    }

    /// Envia um pacote P2P a sério: resolve o peer de destino no
    /// PeerTable, constrói UDP/IPv4/Ethernet, e transmite via
    /// virtio-net real. Mantém sempre o bookkeeping local (tx_queue,
    /// stats) mesmo quando o envio na rede falha, para não quebrar
    /// quem já depende dessas estatísticas.
    pub fn send(&mut self, dst: [u8; 32], payload: Vec<u8>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let len = payload.len();

        self.tx_queue.push(TransportPacket {
            src_node: crate::p2p::node::get_node_id(),
            dst_node: dst,
            payload: payload.clone(),
            packet_id: id,
        });
        self.packets_sent += 1;
        self.bytes_sent += len as u64;

        if !self.send_on_wire(dst, &payload) {
            self.wire_send_failures += 1;
        }

        id
    }

    /// Constrói e transmite o datagrama UDP real. Devolve false sem
    /// nada de errado se simplesmente não soubermos o endereço do
    /// peer (ex: destino ainda não descoberto) — não é um erro fatal.
    fn send_on_wire(&self, dst: [u8; 32], payload: &[u8]) -> bool {
        let Some(peer) = crate::p2p::peer::lookup(&dst) else { return false; };
        let Some(dst_ip) = Ipv4Addr::parse(&peer.address) else { return false; };
        let Some(src_ip) = crate::net::get_primary_ip() else { return false; };

        let mut wire_payload = Vec::with_capacity(32 + payload.len());
        wire_payload.extend_from_slice(&crate::p2p::node::get_node_id());
        wire_payload.extend_from_slice(payload);

        let udp = UdpPacket::new(P2P_UDP_PORT, peer.port, wire_payload);
        let ip  = Ipv4Packet::new(src_ip, dst_ip, IP_PROTO_UDP, udp.serialize());

        // Sem ARP resolvido ainda para este IP, usa broadcast L2 — o
        // destino (ou qualquer switch/hub simples) mesmo assim recebe
        // a trama; um ARP real fica para uma fase seguinte.
        let dst_mac = arp_lookup(&dst_ip).unwrap_or(MacAddr::BROADCAST);
        let src_mac = MacAddr(crate::net::virtio_real::mac());
        let frame = EthernetFrame::new(dst_mac, src_mac, ETH_TYPE_IPV4, ip.serialize());

        crate::net::virtio_real::transmit(frame.serialize())
    }

    /// Regista um pacote genuinamente recebido da rede (chamado por
    /// `poll_receive`, que faz o parsing real dos frames do driver).
    fn on_received(&mut self, src: [u8; 32], payload: Vec<u8>) {
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
    crate::serial_println!("[P2P][TRANSPORT] Camada de transporte UDP real pronta (porta {})", P2P_UDP_PORT);
}

pub fn send(dst: [u8; 32], payload: Vec<u8>) -> u64 {
    TRANSPORT.lock().send(dst, payload)
}

/// Drena os frames recebidos pelo driver virtio-net (poll, sem
/// bloquear) e entrega ao rx_queue os que forem pacotes P2P válidos
/// (UDP, porta P2P_UDP_PORT, com pelo menos 32 bytes de node_id).
/// Chamado periodicamente a partir do timer (ver arch::interrupts).
pub fn poll_receive() {
    let frames = crate::net::virtio_real::receive();
    if frames.is_empty() { return; }

    let mut delivered: Vec<([u8; 32], Vec<u8>)> = Vec::new();

    for raw in frames {
        let Some(eth) = EthernetFrame::parse(&raw) else { continue };
        if eth.ethertype != ETH_TYPE_IPV4 { continue; }
        let Some(ip) = Ipv4Packet::parse(&eth.payload) else { continue };
        if ip.protocol != IP_PROTO_UDP { continue; }
        let Some(udp) = UdpPacket::parse(&ip.payload) else { continue };
        if udp.dst_port != P2P_UDP_PORT { continue; }
        if udp.payload.len() < 32 { continue; }

        let mut src_node = [0u8; 32];
        src_node.copy_from_slice(&udp.payload[..32]);
        let payload = udp.payload[32..].to_vec();
        delivered.push((src_node, payload));
    }

    if !delivered.is_empty() {
        let mut t = TRANSPORT.lock();
        for (src, payload) in delivered {
            t.on_received(src, payload);
        }
    }
}

pub fn get_stats() -> (u64, u64, u64, u64) {
    TRANSPORT.lock().stats()
}

/// Nº de pacotes que não puderam ser enviados na rede real (peer
/// desconhecido ou sem IP resolvível) — útil para diagnóstico.
pub fn get_wire_send_failures() -> u64 {
    TRANSPORT.lock().wire_send_failures
}
