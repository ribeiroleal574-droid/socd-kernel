extern crate alloc;
use alloc::string::ToString;
// ============================================================
// SOC-D Kernel — Descoberta de Nós (mDNS Real, RFC 6762)
// ============================================================
//
// Descoberta real de outros nós SOC-D via UDP multicast, tal como um
// mDNS/Bonjour convencional:
//   - UDP multicast 224.0.0.251:5353
//   - Service type: _socd._tcp.local
//   - Codificação/descodificação real do formato de mensagem DNS
//     (RFC 1035 §4.1, sem suporte a compressão de nomes — não é
//     necessário: só falamos com outras instâncias deste mesmo
//     kernel, que nunca emitem ponteiros de compressão)
//
// Fluxo:
//   1. Ao iniciar: envia mDNS Query (_socd._tcp.local) + o nosso
//      próprio anúncio, ambos por multicast
//   2. `poll_receive()` (chamado do kernel_loop, contexto de tarefa
//      normal — nunca da interrupção do timer) processa pacotes
//      recebidos:
//        - Query de outro nó → respondemos com o nosso anúncio
//        - Anúncio de outro nó → regista/actualiza o PeerTable real
//   3. A cada REDISCOVERY_INTERVAL_TICKS: reenvia a query
// ============================================================

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;
use crate::net::{MacAddr, Ipv4Addr};
use crate::net::ethernet::{EthernetFrame, Ipv4Packet, UdpPacket, ETH_TYPE_IPV4, IP_PROTO_UDP};

/// Tipo de serviço mDNS do SOC-D
pub const SOCD_SERVICE_TYPE: &str = "_socd._tcp.local";

/// Porta padrão do protocolo SOC-D (usada para a ligação P2P em si)
pub const SOCD_PORT: u16 = 7700;

/// Porta mDNS padrão (RFC 6762) — só para descoberta
pub const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_IP: Ipv4Addr = Ipv4Addr([224, 0, 0, 251]);
/// MAC multicast Ethernet correspondente a 224.0.0.251
/// (01:00:5E + 23 bits baixos do endereço IP multicast — RFC 1112)
const MDNS_MULTICAST_MAC: MacAddr = MacAddr([0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]);

/// Intervalo de re-descoberta (em ticks do timer ~1ms = 30s)
pub const REDISCOVERY_INTERVAL_TICKS: u64 = 30_000;

// ─── Estrutura de Pacotes mDNS (RFC 1035 / RFC 6762) ──────────────────────────

/// Cabeçalho de um pacote mDNS (RFC 6762)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MdnsHeader {
    /// Transaction ID (0 para mDNS)
    pub id: u16,
    /// Flags: QR(1) Opcode(4) AA(1) TC(1) RD(1) RA(1) Z(3) RCODE(4)
    pub flags: u16,
    /// Número de questões
    pub qdcount: u16,
    /// Número de respostas
    pub ancount: u16,
    /// Número de registros de autoridade
    pub nscount: u16,
    /// Número de registros adicionais
    pub arcount: u16,
}

impl MdnsHeader {
    /// Query mDNS padrão
    pub fn query() -> Self {
        Self { id: 0, flags: 0x0000, qdcount: 1, ancount: 0, nscount: 0, arcount: 0 }
    }
    /// Response mDNS (Authoritative Answer)
    pub fn response() -> Self {
        Self { id: 0, flags: 0x8400, qdcount: 0, ancount: 1, nscount: 0, arcount: 1 }
    }

    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.id.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(&self.qdcount.to_be_bytes());
        buf.extend_from_slice(&self.ancount.to_be_bytes());
        buf.extend_from_slice(&self.nscount.to_be_bytes());
        buf.extend_from_slice(&self.arcount.to_be_bytes());
    }

    fn decode(d: &[u8], off: &mut usize) -> Option<Self> {
        if *off + 12 > d.len() { return None; }
        let r16 = |o: usize| u16::from_be_bytes([d[o], d[o + 1]]);
        let h = Self {
            id: r16(*off), flags: r16(*off + 2), qdcount: r16(*off + 4),
            ancount: r16(*off + 6), nscount: r16(*off + 8), arcount: r16(*off + 10),
        };
        *off += 12;
        Some(h)
    }
}

/// Tipo de registro DNS
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum DnsRecordType {
    A     = 1,    // IPv4
    AAAA  = 28,   // IPv6
    PTR   = 12,   // Ponteiro (descoberta de serviço)
    SRV   = 33,   // Localização de serviço
    TXT   = 16,   // Texto (metadados)
}

impl DnsRecordType {
    fn from_u16(v: u16) -> Option<Self> {
        match v {
            1  => Some(Self::A),
            28 => Some(Self::AAAA),
            12 => Some(Self::PTR),
            33 => Some(Self::SRV),
            16 => Some(Self::TXT),
            _  => None,
        }
    }
}

/// Codifica um nome DNS em labels (ex: "a.b.local" → [1]a[1]b[5]local[0]).
/// Sem suporte a compressão — não é necessário entre instâncias deste
/// kernel (nunca emitimos nem esperamos ponteiros de compressão).
fn encode_name(buf: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        if label.is_empty() { continue; }
        let bytes = &label.as_bytes()[..label.len().min(63)];
        buf.push(bytes.len() as u8);
        buf.extend_from_slice(bytes);
    }
    buf.push(0);
}

/// Descodifica um nome DNS. Devolve `None` (em vez de interpretar mal)
/// se encontrar um ponteiro de compressão (bits 0xC0) — não suportado.
fn decode_name(d: &[u8], off: &mut usize) -> Option<String> {
    let mut labels: Vec<String> = Vec::new();
    loop {
        if *off >= d.len() { return None; }
        let len = d[*off] as usize;
        if len == 0 { *off += 1; break; }
        if len & 0xC0 != 0 { return None; } // compressão não suportada
        *off += 1;
        if *off + len > d.len() { return None; }
        labels.push(core::str::from_utf8(&d[*off..*off + len]).ok()?.to_string());
        *off += len;
    }
    Some(labels.join("."))
}

/// Registro DNS genérico
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub rtype: DnsRecordType,
    pub ttl: u32,
    pub data: Vec<u8>,
}

impl DnsRecord {
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_name(buf, &self.name);
        buf.extend_from_slice(&(self.rtype as u16).to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes()); // class IN
        buf.extend_from_slice(&self.ttl.to_be_bytes());
        buf.extend_from_slice(&(self.data.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.data);
    }

    fn decode(d: &[u8], off: &mut usize) -> Option<Self> {
        let name = decode_name(d, off)?;
        if *off + 10 > d.len() { return None; }
        let rtype = DnsRecordType::from_u16(u16::from_be_bytes([d[*off], d[*off + 1]]))?;
        *off += 2;
        *off += 2; // class (ignorado)
        let ttl = u32::from_be_bytes([d[*off], d[*off + 1], d[*off + 2], d[*off + 3]]);
        *off += 4;
        let rdlen = u16::from_be_bytes([d[*off], d[*off + 1]]) as usize;
        *off += 2;
        if *off + rdlen > d.len() { return None; }
        let data = d[*off..*off + rdlen].to_vec();
        *off += rdlen;
        Some(Self { name, rtype, ttl, data })
    }
}

/// Questão mDNS
#[derive(Debug, Clone)]
pub struct MdnsQuestion {
    pub name: String,
    pub qtype: DnsRecordType,
    pub unicast_response: bool,
}

impl MdnsQuestion {
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_name(buf, &self.name);
        buf.extend_from_slice(&(self.qtype as u16).to_be_bytes());
        let qclass: u16 = if self.unicast_response { 0x8001 } else { 0x0001 };
        buf.extend_from_slice(&qclass.to_be_bytes());
    }

    fn decode(d: &[u8], off: &mut usize) -> Option<Self> {
        let name = decode_name(d, off)?;
        if *off + 4 > d.len() { return None; }
        let qtype = DnsRecordType::from_u16(u16::from_be_bytes([d[*off], d[*off + 1]]))?;
        let qclass = u16::from_be_bytes([d[*off + 2], d[*off + 3]]);
        *off += 4;
        Some(Self { name, qtype, unicast_response: qclass & 0x8000 != 0 })
    }
}

/// Pacote mDNS completo
#[derive(Debug, Clone)]
pub struct MdnsPacket {
    pub header: MdnsHeader,
    pub questions: Vec<MdnsQuestion>,
    pub answers: Vec<DnsRecord>,
    pub additional: Vec<DnsRecord>,
}

impl MdnsPacket {
    /// Cria uma query de descoberta de serviço SOC-D
    pub fn discovery_query() -> Self {
        Self {
            header: MdnsHeader::query(),
            questions: alloc::vec![MdnsQuestion {
                name: SOCD_SERVICE_TYPE.into(),
                qtype: DnsRecordType::PTR,
                unicast_response: false,
            }],
            answers: Vec::new(),
            additional: Vec::new(),
        }
    }

    /// Cria um pacote de anúncio de presença
    pub fn announcement(
        node_name: &str,
        node_id_hex: &str,
        port: u16,
    ) -> Self {
        let service_name = alloc::format!("{}.{}", node_name, SOCD_SERVICE_TYPE);

        Self {
            header: MdnsHeader::response(),
            questions: Vec::new(),
            answers: alloc::vec![
                DnsRecord {
                    name: SOCD_SERVICE_TYPE.into(),
                    rtype: DnsRecordType::PTR,
                    ttl: 4500,
                    data: service_name.as_bytes().to_vec(),
                }
            ],
            additional: alloc::vec![
                DnsRecord {
                    name: service_name.clone(),
                    rtype: DnsRecordType::SRV,
                    ttl: 120,
                    data: alloc::format!("0 0 {} {}.local", port, node_name)
                              .as_bytes().to_vec(),
                },
                DnsRecord {
                    name: service_name,
                    rtype: DnsRecordType::TXT,
                    ttl: 4500,
                    data: alloc::format!(
                        "socd=1\nid={}\nproto={}",
                        node_id_hex,
                        super::node::PROTOCOL_VERSION
                    ).as_bytes().to_vec(),
                },
            ],
        }
    }

    /// Serializa para o formato de mensagem DNS (RFC 1035 §4.1)
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let header = MdnsHeader {
            qdcount: self.questions.len() as u16,
            ancount: self.answers.len() as u16,
            nscount: 0,
            arcount: self.additional.len() as u16,
            ..self.header
        };
        header.encode(&mut buf);
        for q in &self.questions { q.encode(&mut buf); }
        for a in &self.answers { a.encode(&mut buf); }
        for a in &self.additional { a.encode(&mut buf); }
        buf
    }

    /// Descodifica uma mensagem DNS recebida da rede
    pub fn parse(d: &[u8]) -> Option<Self> {
        let mut off = 0usize;
        let header = MdnsHeader::decode(d, &mut off)?;

        let mut questions = Vec::new();
        for _ in 0..header.qdcount {
            questions.push(MdnsQuestion::decode(d, &mut off)?);
        }
        let mut answers = Vec::new();
        for _ in 0..header.ancount {
            answers.push(DnsRecord::decode(d, &mut off)?);
        }
        // Regista de autoridade (nscount) — ignorados, mas têm de ser
        // consumidos do cursor para os "additional" ficarem no offset certo
        for _ in 0..header.nscount {
            DnsRecord::decode(d, &mut off)?;
        }
        let mut additional = Vec::new();
        for _ in 0..header.arcount {
            additional.push(DnsRecord::decode(d, &mut off)?);
        }

        Some(Self { header, questions, answers, additional })
    }
}

// ─── Motor de Descoberta ─────────────────────────────────────────────────────

/// Estado do motor de descoberta
pub struct DiscoveryEngine {
    pub running: bool,
    pub last_query_tick: u64,
    pub queries_sent: u64,
    pub responses_received: u64,
    /// Log de eventos de descoberta
    pub event_log: Vec<DiscoveryEvent>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryEvent {
    pub tick: u64,
    pub kind: DiscoveryEventKind,
    pub peer_name: String,
}

#[derive(Debug, Clone)]
pub enum DiscoveryEventKind {
    QuerySent,
    AnnouncementSent,
    ResponseReceived,
    NewPeerFound,
    PeerUpdated,
    PeerLost,
}

impl DiscoveryEngine {
    const fn new() -> Self {
        Self {
            running: false,
            last_query_tick: 0,
            queries_sent: 0,
            responses_received: 0,
            event_log: Vec::new(),
        }
    }

    fn log(&mut self, tick: u64, kind: DiscoveryEventKind, peer_name: &str) {
        self.event_log.push(DiscoveryEvent { tick, kind, peer_name: peer_name.to_string() });
        if self.event_log.len() > 50 {
            let drain_count = self.event_log.len() - 50;
            self.event_log.drain(0..drain_count);
        }
    }

    /// Processa um tick do sistema — verifica se deve re-descobrir
    pub fn tick(&mut self, current_tick: u64) {
        if !self.running { return; }

        let elapsed = current_tick.saturating_sub(self.last_query_tick);
        if elapsed >= REDISCOVERY_INTERVAL_TICKS {
            self.send_discovery_query(current_tick);
        }
    }

    /// Envia a query de descoberta real (UDP multicast 224.0.0.251:5353)
    fn send_discovery_query(&mut self, tick: u64) {
        let pkt = MdnsPacket::discovery_query();
        self.queries_sent += 1;
        self.last_query_tick = tick;
        self.log(tick, DiscoveryEventKind::QuerySent, "multicast");

        if send_multicast(&pkt) {
            crate::serial_println!("[P2P][DISC] Query #{} enviada (multicast real)", self.queries_sent);
        } else {
            crate::serial_println!("[P2P][DISC] Query #{}: sem rede disponivel ainda", self.queries_sent);
        }
    }

    /// Envia o nosso próprio anúncio (UDP multicast)
    fn send_announcement(&mut self, tick: u64) {
        let Some((name, node_id, _proto)) = super::node::get_info() else { return; };
        let node_id_hex = hex_encode(&node_id);
        let pkt = MdnsPacket::announcement(&name, &node_id_hex, SOCD_PORT);
        if send_multicast(&pkt) {
            self.log(tick, DiscoveryEventKind::AnnouncementSent, &name);
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(core::char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(core::char::from_digit((b & 0xF) as u32, 16).unwrap());
    }
    s
}

fn hex_decode(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 { return None; }
    let mut out = [0u8; 32];
    let bytes = s.as_bytes();
    for i in 0..32 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

/// Envia um pacote mDNS por UDP multicast real (224.0.0.251:5353)
fn send_multicast(pkt: &MdnsPacket) -> bool {
    send_unicast(pkt, MDNS_MULTICAST_IP, MDNS_PORT, MDNS_MULTICAST_MAC)
}

/// Envia um pacote mDNS por UDP para um destino específico
fn send_unicast(pkt: &MdnsPacket, dst_ip: Ipv4Addr, dst_port: u16, dst_mac: MacAddr) -> bool {
    let Some(src_ip) = crate::net::get_primary_ip() else { return false; };
    let payload = pkt.serialize();
    let udp = UdpPacket::new(MDNS_PORT, dst_port, payload);
    let ip = Ipv4Packet::new(src_ip, dst_ip, IP_PROTO_UDP, udp.serialize());
    let src_mac = MacAddr(crate::net::virtio_real::mac());
    let frame = EthernetFrame::new(dst_mac, src_mac, ETH_TYPE_IPV4, ip.serialize());
    crate::net::virtio_real::transmit(frame.serialize())
}

/// Processa um pacote mDNS recebido: responde a queries de outros nós,
/// e regista/actualiza peers reais a partir de anúncios recebidos.
fn handle_packet(pkt: &MdnsPacket, src_ip: Ipv4Addr, src_port: u16) {
    // Alguém está a perguntar pelo nosso serviço → respondemos
    let is_query_for_us = pkt.questions.iter()
        .any(|q| q.name == SOCD_SERVICE_TYPE && q.qtype == DnsRecordType::PTR);
    if is_query_for_us {
        if let Some((name, node_id, _proto)) = super::node::get_info() {
            let node_id_hex = hex_encode(&node_id);
            let response = MdnsPacket::announcement(&name, &node_id_hex, SOCD_PORT);
            send_unicast(&response, src_ip, src_port, MacAddr::BROADCAST);
        }
    }

    // Extrai um anúncio de nó destas respostas (se as houver)
    let mut port: Option<u16> = None;
    let mut node_id: Option<[u8; 32]> = None;
    let mut node_name: Option<String> = None;

    for rec in pkt.answers.iter().chain(pkt.additional.iter()) {
        match rec.rtype {
            DnsRecordType::SRV => {
                if let Ok(s) = core::str::from_utf8(&rec.data) {
                    // formato: "0 0 <porta> <nome>.local"
                    let parts: Vec<&str> = s.split_whitespace().collect();
                    if parts.len() >= 3 {
                        port = parts[2].parse::<u16>().ok();
                    }
                }
            }
            DnsRecordType::TXT => {
                if let Ok(s) = core::str::from_utf8(&rec.data) {
                    for line in s.split('\n') {
                        if let Some(hex) = line.strip_prefix("id=") {
                            node_id = hex_decode(hex);
                        }
                    }
                    // nome do serviço: "<nome-do-no>._socd._tcp.local"
                    if let Some(prefix) = rec.name.strip_suffix(&alloc::format!(".{}", SOCD_SERVICE_TYPE)) {
                        node_name = Some(prefix.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    if let (Some(id), Some(p)) = (node_id, port) {
        // Não registamos a nós próprios
        if id != super::node::get_node_id() {
            let name = node_name.unwrap_or_else(|| "socd-peer".to_string());
            let address = alloc::format!("{}.{}.{}.{}", src_ip.0[0], src_ip.0[1], src_ip.0[2], src_ip.0[3]);
            super::peer::add_discovered(id, [0u8; 32], &name, &address, p, false);

            let mut disc = DISCOVERY.lock();
            disc.responses_received += 1;
            let tick = disc.last_query_tick;
            disc.log(tick, DiscoveryEventKind::ResponseReceived, &name);
        }
    }
}

static DISCOVERY: Spinlock<DiscoveryEngine> = Spinlock::new(DiscoveryEngine::new());

pub fn init() {
    let mut disc = DISCOVERY.lock();
    disc.running = true;
    disc.send_discovery_query(0);
    disc.send_announcement(0);
    drop(disc);
    crate::serial_println!("[P2P][DISC] Motor de descoberta ativo (mDNS real, {}:{})",
        "224.0.0.251", MDNS_PORT);
}

pub fn tick(current_tick: u64) {
    DISCOVERY.lock().tick(current_tick);
}

/// Trata um datagrama UDP já desencapsulado, endereçado à porta mDNS
/// (chamado pelo dispatcher central — ver net::poll_and_dispatch).
pub fn handle_udp(payload: &[u8], src_ip: Ipv4Addr, src_port: u16) {
    let Some(pkt) = MdnsPacket::parse(payload) else { return; };
    handle_packet(&pkt, src_ip, src_port);
}

pub fn get_stats() -> (u64, u64) {
    let d = DISCOVERY.lock();
    (d.queries_sent, d.responses_received)
}
