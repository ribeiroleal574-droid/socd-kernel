extern crate alloc;
// ============================================================
// SOC-D — Ethernet, IP, TCP/UDP, Socket API, DHCP, DNS
// ============================================================

use alloc::{vec::Vec, string::{String, ToString}, collections::BTreeMap};
use spinning_top::Spinlock;
use super::{MacAddr, Ipv4Addr, SocketAddr, Protocol};

// ─── ETHERNET ────────────────────────────────────────────────────────────────

pub const ETH_TYPE_IPV4: u16 = 0x0800;
pub const ETH_TYPE_ARP:  u16 = 0x0806;
pub const ETH_TYPE_IPV6: u16 = 0x86DD;
pub const ETH_TYPE_VLAN: u16 = 0x8100;

#[derive(Debug, Clone)]
pub struct EthernetFrame {
    pub dst:      MacAddr,
    pub src:      MacAddr,
    pub ethertype: u16,
    pub payload:  Vec<u8>,
}

impl EthernetFrame {
    pub fn new(dst: MacAddr, src: MacAddr, ethertype: u16, payload: Vec<u8>) -> Self {
        Self { dst, src, ethertype, payload }
    }

    /// Serializa para bytes (sem FCS)
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(14 + self.payload.len());
        buf.extend_from_slice(&self.dst.0);
        buf.extend_from_slice(&self.src.0);
        buf.push((self.ethertype >> 8) as u8);
        buf.push(self.ethertype as u8);
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Parseia de bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 { return None; }
        let dst = MacAddr(data[0..6].try_into().ok()?);
        let src = MacAddr(data[6..12].try_into().ok()?);
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        Some(Self { dst, src, ethertype, payload: data[14..].to_vec() })
    }
}

// ─── ARP ──────────────────────────────────────────────────────────────────────

pub struct ArpTable {
    entries: BTreeMap<u32, MacAddr>, // IPv4 → MAC
}

impl ArpTable {
    const fn new() -> Self { Self { entries: BTreeMap::new() } }

    pub fn insert(&mut self, ip: Ipv4Addr, mac: MacAddr) {
        self.entries.insert(ip.to_u32(), mac);
    }

    pub fn lookup(&self, ip: &Ipv4Addr) -> Option<MacAddr> {
        self.entries.get(&ip.to_u32()).copied()
    }
}

static ARP_TABLE: Spinlock<ArpTable> = Spinlock::new(ArpTable::new());

pub fn arp_insert(ip: Ipv4Addr, mac: MacAddr) {
    ARP_TABLE.lock().insert(ip, mac);
}

pub fn arp_lookup(ip: &Ipv4Addr) -> Option<MacAddr> {
    ARP_TABLE.lock().lookup(ip)
}

// ─── IPV4 ─────────────────────────────────────────────────────────────────────

pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP:  u8 = 6;
pub const IP_PROTO_UDP:  u8 = 17;

#[derive(Debug, Clone)]
pub struct Ipv4Packet {
    pub version:    u8,
    pub ihl:        u8,
    pub dscp:       u8,
    pub ecn:        u8,
    pub total_len:  u16,
    pub id:         u16,
    pub flags:      u8,
    pub frag_offset: u16,
    pub ttl:        u8,
    pub protocol:   u8,
    pub checksum:   u16,
    pub src:        Ipv4Addr,
    pub dst:        Ipv4Addr,
    pub payload:    Vec<u8>,
}

impl Ipv4Packet {
    pub fn new(src: Ipv4Addr, dst: Ipv4Addr, proto: u8, payload: Vec<u8>) -> Self {
        let total_len = (20 + payload.len()) as u16;
        let mut pkt = Self {
            version: 4, ihl: 5, dscp: 0, ecn: 0,
            total_len, id: 0x1234, flags: 0x40, // Don't Fragment
            frag_offset: 0, ttl: 64, protocol: proto,
            checksum: 0, src, dst, payload,
        };
        pkt.checksum = pkt.compute_checksum();
        pkt
    }

    fn compute_checksum(&self) -> u16 {
        let header = [
            0x45u8, self.dscp << 2 | self.ecn,
            (self.total_len >> 8) as u8, self.total_len as u8,
            (self.id >> 8) as u8, self.id as u8,
            (self.flags as u16 * 0x2000 | self.frag_offset >> 8) as u8, self.frag_offset as u8,
            self.ttl, self.protocol, 0, 0,
            self.src.0[0], self.src.0[1], self.src.0[2], self.src.0[3],
            self.dst.0[0], self.dst.0[1], self.dst.0[2], self.dst.0[3],
        ];
        internet_checksum(&header)
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.payload.len());
        buf.push(0x45); // version=4, ihl=5
        buf.push(0);
        buf.push((self.total_len >> 8) as u8);
        buf.push(self.total_len as u8);
        buf.push((self.id >> 8) as u8); buf.push(self.id as u8);
        buf.push(0x40); buf.push(0); // DF flag
        buf.push(self.ttl);
        buf.push(self.protocol);
        buf.push((self.checksum >> 8) as u8); buf.push(self.checksum as u8);
        buf.extend_from_slice(&self.src.0);
        buf.extend_from_slice(&self.dst.0);
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 { return None; }
        let ihl = (data[0] & 0x0F) as usize * 4;
        if data.len() < ihl { return None; }
        Some(Self {
            version: data[0] >> 4,
            ihl: data[0] & 0x0F,
            dscp: data[1] >> 2, ecn: data[1] & 0x03,
            total_len: u16::from_be_bytes([data[2], data[3]]),
            id: u16::from_be_bytes([data[4], data[5]]),
            flags: data[6] >> 5,
            frag_offset: u16::from_be_bytes([data[6] & 0x1F, data[7]]),
            ttl: data[8], protocol: data[9],
            checksum: u16::from_be_bytes([data[10], data[11]]),
            src: Ipv4Addr([data[12], data[13], data[14], data[15]]),
            dst: Ipv4Addr([data[16], data[17], data[18], data[19]]),
            payload: data[ihl..].to_vec(),
        })
    }
}

fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i+1]]) as u32;
        i += 2;
    }
    if i < data.len() { sum += (data[i] as u32) << 8; }
    while sum >> 16 != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !(sum as u16)
}

// ─── UDP ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UdpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub length:   u16,
    pub checksum: u16,
    pub payload:  Vec<u8>,
}

impl UdpPacket {
    pub fn new(src_port: u16, dst_port: u16, payload: Vec<u8>) -> Self {
        let length = (8 + payload.len()) as u16;
        Self { src_port, dst_port, length, checksum: 0, payload }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + self.payload.len());
        buf.push((self.src_port >> 8) as u8); buf.push(self.src_port as u8);
        buf.push((self.dst_port >> 8) as u8); buf.push(self.dst_port as u8);
        buf.push((self.length >> 8) as u8);   buf.push(self.length as u8);
        buf.push(0); buf.push(0); // checksum
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 { return None; }
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            length:   u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
            payload:  data[8..].to_vec(),
        })
    }
}

// ─── TCP ──────────────────────────────────────────────────────────────────────

/// Flags TCP
pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const TCP_URG: u8 = 0x20;

#[derive(Debug, Clone)]
pub struct TcpSegment {
    pub src_port:   u16,
    pub dst_port:   u16,
    pub seq_num:    u32,
    pub ack_num:    u32,
    pub data_offset: u8,
    pub flags:      u8,
    pub window:     u16,
    pub checksum:   u16,
    pub urgent_ptr: u16,
    pub payload:    Vec<u8>,
}

impl TcpSegment {
    pub fn syn(src_port: u16, dst_port: u16, seq: u32) -> Self {
        Self {
            src_port, dst_port, seq_num: seq, ack_num: 0,
            data_offset: 5, flags: TCP_SYN, window: 65535,
            checksum: 0, urgent_ptr: 0, payload: Vec::new(),
        }
    }
    pub fn ack(src: u16, dst: u16, seq: u32, ack: u32, data: Vec<u8>) -> Self {
        Self {
            src_port: src, dst_port: dst, seq_num: seq, ack_num: ack,
            data_offset: 5, flags: TCP_ACK | if !data.is_empty() { TCP_PSH } else { 0 },
            window: 65535, checksum: 0, urgent_ptr: 0, payload: data,
        }
    }
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.payload.len());
        buf.push((self.src_port >> 8) as u8); buf.push(self.src_port as u8);
        buf.push((self.dst_port >> 8) as u8); buf.push(self.dst_port as u8);
        buf.extend_from_slice(&self.seq_num.to_be_bytes());
        buf.extend_from_slice(&self.ack_num.to_be_bytes());
        buf.push(self.data_offset << 4);
        buf.push(self.flags);
        buf.push((self.window >> 8) as u8); buf.push(self.window as u8);
        buf.push(0); buf.push(0); // checksum
        buf.push(0); buf.push(0); // urgent
        buf.extend_from_slice(&self.payload);
        buf
    }
}

/// Estado de uma conexão TCP
#[derive(Debug, Clone, PartialEq)]
pub enum TcpState {
    Closed, Listen, SynSent, SynReceived,
    Established, FinWait1, FinWait2,
    CloseWait, Closing, LastAck, TimeWait,
}

// ─── SOCKET API ──────────────────────────────────────────────────────────────

pub type SocketFd = u32;
static NEXT_FD_VAL: Spinlock<u32> = Spinlock::new(3); // 0,1,2 = stdin,stdout,stderr

#[derive(Debug, Clone)]
pub enum SocketKind { TcpClient, TcpServer, Udp, Raw }

#[derive(Debug, Clone)]
pub struct Socket {
    pub fd:       SocketFd,
    pub kind:     SocketKind,
    pub local:    Option<SocketAddr>,
    pub remote:   Option<SocketAddr>,
    pub state:    TcpState,
    pub rx_buf:   Vec<u8>,
    pub tx_buf:   Vec<u8>,
    pub nonblock: bool,
}

impl Socket {
    pub fn new(kind: SocketKind) -> Self {
        Self {
            fd: { let mut v = NEXT_FD_VAL.lock(); let old = *v; *v += 1; old },
            kind, local: None, remote: None,
            state: TcpState::Closed,
            rx_buf: Vec::new(), tx_buf: Vec::new(),
            nonblock: false,
        }
    }
}

pub struct SocketTable {
    sockets: BTreeMap<SocketFd, Socket>,
}

impl SocketTable {
    const fn new() -> Self { Self { sockets: BTreeMap::new() } }

    pub fn create(&mut self, kind: SocketKind) -> SocketFd {
        let sock = Socket::new(kind);
        let fd = sock.fd;
        self.sockets.insert(fd, sock);
        fd
    }

    pub fn connect(&mut self, fd: SocketFd, addr: SocketAddr) -> bool {
        if let Some(sock) = self.sockets.get_mut(&fd) {
            sock.remote = Some(addr);
            sock.state  = TcpState::SynSent;
            // Fase 5: enviar SYN via virtio-net
            sock.state  = TcpState::Established; // Simulado
            crate::serial_println!("[NET][SOCK] fd={} conectado a {:?}", fd, addr);
            return true;
        }
        false
    }

    pub fn send(&mut self, fd: SocketFd, data: &[u8]) -> usize {
        if let Some(sock) = self.sockets.get_mut(&fd) {
            if sock.state == TcpState::Established {
                sock.tx_buf.extend_from_slice(data);
                // Fase 5: flushear para virtio-net imediatamente
                return data.len();
            }
        }
        0
    }

    pub fn recv(&mut self, fd: SocketFd, buf: &mut [u8]) -> usize {
        if let Some(sock) = self.sockets.get_mut(&fd) {
            let n = buf.len().min(sock.rx_buf.len());
            buf[..n].copy_from_slice(&sock.rx_buf[..n]);
            sock.rx_buf.drain(..n);
            return n;
        }
        0
    }

    pub fn close(&mut self, fd: SocketFd) {
        if let Some(sock) = self.sockets.get_mut(&fd) {
            sock.state = TcpState::Closed;
        }
        self.sockets.remove(&fd);
    }
}

static SOCKET_TABLE: Spinlock<SocketTable> = Spinlock::new(SocketTable::new());

pub fn socket_create(proto: Protocol) -> SocketFd {
    let kind = match proto {
        Protocol::TCP => SocketKind::TcpClient,
        Protocol::UDP => SocketKind::Udp,
        _             => SocketKind::Raw,
    };
    SOCKET_TABLE.lock().create(kind)
}
pub fn socket_connect(fd: SocketFd, addr: SocketAddr) -> bool {
    SOCKET_TABLE.lock().connect(fd, addr)
}
pub fn socket_send(fd: SocketFd, data: &[u8]) -> usize {
    SOCKET_TABLE.lock().send(fd, data)
}
pub fn socket_recv(fd: SocketFd, buf: &mut [u8]) -> usize {
    SOCKET_TABLE.lock().recv(fd, buf)
}
pub fn socket_close(fd: SocketFd) {
    SOCKET_TABLE.lock().close(fd)
}

// ─── DHCP ────────────────────────────────────────────────────────────────────

pub fn init() {
    crate::serial_println!("[NET][ETH/IP/TCP] Protocolos de rede inicializados");
    // Pre-popula tabela ARP com gateway
    arp_insert(Ipv4Addr([10,0,2,2]), MacAddr([0x52,0x55,0x0A,0x00,0x02,0x02]));
    crate::serial_println!("[NET][ARP] Cache ARP pre-populado com gateway");
}

// ─── DNS ─────────────────────────────────────────────────────────────────────

/// Cache DNS simples
pub struct DnsCache {
    entries: BTreeMap<String, Ipv4Addr>,
}

impl DnsCache {
    const fn new() -> Self { Self { entries: BTreeMap::new() } }

    pub fn insert(&mut self, name: &str, ip: Ipv4Addr) {
        self.entries.insert(name.to_string(), ip);
    }

    pub fn lookup(&self, name: &str) -> Option<Ipv4Addr> {
        self.entries.get(name).copied()
    }
}

static DNS_CACHE: Spinlock<DnsCache> = Spinlock::new(DnsCache::new());

pub fn dns_insert(name: &str, ip: Ipv4Addr) {
    DNS_CACHE.lock().insert(name, ip);
}

pub fn dns_lookup(name: &str) -> Option<Ipv4Addr> {
    // Verifica cache primeiro
    if let Some(ip) = DNS_CACHE.lock().lookup(name) {
        return Some(ip);
    }
    // Fase 5: enviar query DNS UDP ao servidor configurado
    None
}
