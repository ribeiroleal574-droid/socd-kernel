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
    pub fn syn_ack(src: u16, dst: u16, seq: u32, ack: u32) -> Self {
        Self {
            src_port: src, dst_port: dst, seq_num: seq, ack_num: ack,
            data_offset: 5, flags: TCP_SYN | TCP_ACK, window: 65535,
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
    pub fn fin_ack(src: u16, dst: u16, seq: u32, ack: u32) -> Self {
        Self {
            src_port: src, dst_port: dst, seq_num: seq, ack_num: ack,
            data_offset: 5, flags: TCP_FIN | TCP_ACK, window: 65535,
            checksum: 0, urgent_ptr: 0, payload: Vec::new(),
        }
    }
    pub fn rst(src: u16, dst: u16, seq: u32) -> Self {
        Self {
            src_port: src, dst_port: dst, seq_num: seq, ack_num: 0,
            data_offset: 5, flags: TCP_RST, window: 0,
            checksum: 0, urgent_ptr: 0, payload: Vec::new(),
        }
    }

    /// Serializa sem checksum (todos os bytes de checksum a 0) — usar
    /// só para cálculo interno do próprio checksum.
    fn serialize_raw(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.payload.len());
        buf.push((self.src_port >> 8) as u8); buf.push(self.src_port as u8);
        buf.push((self.dst_port >> 8) as u8); buf.push(self.dst_port as u8);
        buf.extend_from_slice(&self.seq_num.to_be_bytes());
        buf.extend_from_slice(&self.ack_num.to_be_bytes());
        buf.push(self.data_offset << 4);
        buf.push(self.flags);
        buf.push((self.window >> 8) as u8); buf.push(self.window as u8);
        buf.push(0); buf.push(0); // checksum (preenchido depois)
        buf.push(0); buf.push(0); // urgent
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Serializa com o checksum TCP real, calculado sobre o
    /// pseudo-cabeçalho IPv4 (RFC 793 §3.1) — obrigatório para o
    /// segmento não ser descartado por qualquer stack TCP real (ao
    /// contrário do UDP, onde checksum=0 é uma opção válida).
    pub fn serialize(&self, src_ip: Ipv4Addr, dst_ip: Ipv4Addr) -> Vec<u8> {
        let mut buf = self.serialize_raw();

        let mut pseudo = Vec::with_capacity(12 + buf.len());
        pseudo.extend_from_slice(&src_ip.0);
        pseudo.extend_from_slice(&dst_ip.0);
        pseudo.push(0);
        pseudo.push(6); // protocolo TCP
        pseudo.extend_from_slice(&(buf.len() as u16).to_be_bytes());
        pseudo.extend_from_slice(&buf);

        let checksum = internet_checksum(&pseudo);
        buf[16] = (checksum >> 8) as u8;
        buf[17] = checksum as u8;
        buf
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 { return None; }
        let data_offset = data[12] >> 4;
        let header_len = (data_offset as usize) * 4;
        if header_len < 20 || data.len() < header_len { return None; }
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            seq_num: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ack_num: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            data_offset,
            flags: data[13],
            window: u16::from_be_bytes([data[14], data[15]]),
            checksum: u16::from_be_bytes([data[16], data[17]]),
            urgent_ptr: u16::from_be_bytes([data[18], data[19]]),
            payload: data[header_len..].to_vec(),
        })
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
    /// Próximo número de sequência NOSSO a usar (só TCP)
    pub seq_num:  u32,
    /// Último número de sequência do peer que já vimos (para ACKs) —
    /// na prática, o próximo byte que esperamos receber (só TCP)
    pub ack_num:  u32,
}

impl Socket {
    pub fn new(kind: SocketKind) -> Self {
        Self {
            fd: { let mut v = NEXT_FD_VAL.lock(); let old = *v; *v += 1; old },
            kind, local: None, remote: None,
            state: TcpState::Closed,
            rx_buf: Vec::new(), tx_buf: Vec::new(),
            nonblock: false,
            seq_num: 0x1000_0000, // ISN inicial arbitrário (não aleatório — simplificação)
            ack_num: 0,
        }
    }
}

pub struct SocketTable {
    sockets: BTreeMap<SocketFd, Socket>,
    /// Próxima porta efémera a atribuir em bind()/connect() automático
    next_ephemeral_port: u16,
}

impl SocketTable {
    const fn new() -> Self { Self { sockets: BTreeMap::new(), next_ephemeral_port: 49152 } }

    pub fn create(&mut self, kind: SocketKind) -> SocketFd {
        let sock = Socket::new(kind);
        let fd = sock.fd;
        self.sockets.insert(fd, sock);
        fd
    }

    fn alloc_ephemeral_port(&mut self) -> u16 {
        let p = self.next_ephemeral_port;
        self.next_ephemeral_port = if p >= 65000 { 49152 } else { p + 1 };
        p
    }

    /// Associa o socket a uma porta local específica (necessário para
    /// receber dados — sem bind, um socket UDP/TCP não tem porta
    /// local até `connect()` lhe atribuir uma efémera automaticamente)
    pub fn bind(&mut self, fd: SocketFd, addr: SocketAddr) -> bool {
        if let Some(sock) = self.sockets.get_mut(&fd) {
            sock.local = Some(addr);
            true
        } else {
            false
        }
    }

    /// Procura o socket dono de um segmento/datagrama recebido — por
    /// (porta local, [porta+IP remoto para TCP, já ligado]).
    fn find_owner(&mut self, local_port: u16, remote: SocketAddr) -> Option<&mut Socket> {
        self.sockets.values_mut().find(|s| {
            let local_matches = s.local.map(|l| l.port()) == Some(local_port);
            if !local_matches { return false; }
            match s.kind {
                // TCP cliente: já sabemos com quem falamos — tem de bater certo
                SocketKind::TcpClient => s.remote == Some(remote) || s.remote.is_none(),
                // UDP/Raw: qualquer remetente na porta certa serve
                _ => true,
            }
        })
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
}

static SOCKET_TABLE: Spinlock<SocketTable> = Spinlock::new(SocketTable::new());

/// Timeout (em ticks do timer, ~1ms cada) para operações bloqueantes
/// de rede — handshake TCP, espera por ACK, fecho. Evita esperar para
/// sempre por uma resposta que nunca chega (peer em baixo, sem rede).
const NET_TIMEOUT_TICKS: u64 = 5000; // ~5s

/// Espera (cedendo a CPU a outras tarefas, nunca busy-loop puro) até
/// `cond` devolver `Some`, ou até passar `NET_TIMEOUT_TICKS`. Nunca
/// segura nenhum lock durante a espera — quem chamar `cond` tem de
/// bloquear/libertar o SOCKET_TABLE sozinho a cada tentativa.
fn wait_for<T>(mut cond: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = crate::arch::interrupts::current_tick() + NET_TIMEOUT_TICKS;
    loop {
        if let Some(v) = cond() { return Some(v); }
        if crate::arch::interrupts::current_tick() >= deadline { return None; }
        crate::modules::scheduler::yield_now();
    }
}

pub fn socket_create(proto: Protocol) -> SocketFd {
    let kind = match proto {
        Protocol::TCP => SocketKind::TcpClient,
        Protocol::UDP => SocketKind::Udp,
        _             => SocketKind::Raw,
    };
    SOCKET_TABLE.lock().create(kind)
}

pub fn socket_bind(fd: SocketFd, addr: SocketAddr) -> bool {
    SOCKET_TABLE.lock().bind(fd, addr)
}

/// Liga o socket a um destino. UDP: só regista o destino por omissão
/// (sem pacotes na rede — semântica normal de connect() em UDP). TCP:
/// faz o handshake de 3 vias real (SYN → SYN-ACK → ACK), bloqueando
/// até NET_TIMEOUT_TICKS.
pub fn socket_connect(fd: SocketFd, addr: SocketAddr) -> bool {
    let SocketAddr::V4(dst_ip, dst_port) = addr else { return false; };
    let Some(src_ip) = crate::net::get_primary_ip() else { return false; };

    let (kind, local_port, isn) = {
        let mut t = SOCKET_TABLE.lock();
        if !t.sockets.contains_key(&fd) { return false; }
        let needs_port = t.sockets.get(&fd).map(|s| s.local.is_none()).unwrap_or(false);
        if needs_port {
            let port = t.alloc_ephemeral_port();
            if let Some(s) = t.sockets.get_mut(&fd) {
                s.local = Some(SocketAddr::V4(src_ip, port));
            }
        }
        let Some(sock) = t.sockets.get_mut(&fd) else { return false; };
        sock.remote = Some(addr);
        (sock.kind.clone(), sock.local.unwrap().port(), sock.seq_num)
    };

    if !matches!(kind, SocketKind::TcpClient) {
        // UDP: liga sem tocar na rede
        SOCKET_TABLE.lock().sockets.get_mut(&fd).map(|s| s.state = TcpState::Established);
        return true;
    }

    // TCP: handshake real
    SOCKET_TABLE.lock().sockets.get_mut(&fd).map(|s| s.state = TcpState::SynSent);
    let dst_mac = crate::net::ethernet::arp_lookup(&dst_ip).unwrap_or(MacAddr::BROADCAST);
    let syn = TcpSegment::syn(local_port, dst_port, isn);
    if !send_tcp(&syn, src_ip, dst_ip, dst_mac) {
        crate::serial_println!("[NET][TCP] fd={} SYN falhou (sem rede?)", fd);
        return false;
    }

    let established = wait_for(|| {
        let t = SOCKET_TABLE.lock();
        match t.sockets.get(&fd).map(|s| s.state.clone()) {
            Some(TcpState::Established) => Some(()),
            Some(TcpState::Closed) => Some(()), // RST recebido — sai da espera, connect falha
            _ => None,
        }
    });

    let ok = established.is_some()
        && SOCKET_TABLE.lock().sockets.get(&fd).map(|s| s.state.clone()) == Some(TcpState::Established);

    if ok {
        crate::serial_println!("[NET][SOCK] fd={} conectado a {:?} (TCP handshake real)", fd, addr);
    } else {
        crate::serial_println!("[NET][TCP] fd={} handshake falhou/timeout", fd);
    }
    ok
}

/// Envia dados. UDP: um datagrama real por chamada. TCP: um segmento
/// PSH+ACK real (sem retransmissão — ver nota de âmbito no topo do
/// ficheiro); espera o ACK do peer até NET_TIMEOUT_TICKS.
pub fn socket_send(fd: SocketFd, data: &[u8]) -> usize {
    let Some(src_ip) = crate::net::get_primary_ip() else { return 0; };

    let (kind, state, local, remote, seq, ack) = {
        let t = SOCKET_TABLE.lock();
        let Some(s) = t.sockets.get(&fd) else { return 0; };
        (s.kind.clone(), s.state.clone(), s.local, s.remote, s.seq_num, s.ack_num)
    };
    let (Some(SocketAddr::V4(_, local_port)), Some(SocketAddr::V4(dst_ip, dst_port))) = (local, remote) else { return 0; };
    let dst_mac = crate::net::ethernet::arp_lookup(&dst_ip).unwrap_or(MacAddr::BROADCAST);

    match kind {
        SocketKind::Udp => {
            let udp = UdpPacket::new(local_port, dst_port, data.to_vec());
            let ip = Ipv4Packet::new(src_ip, dst_ip, IP_PROTO_UDP, udp.serialize());
            let frame = EthernetFrame::new(dst_mac, MacAddr(crate::net::virtio_real::mac()), ETH_TYPE_IPV4, ip.serialize());
            if crate::net::virtio_real::transmit(frame.serialize()) { data.len() } else { 0 }
        }
        SocketKind::TcpClient if state == TcpState::Established => {
            let seg = TcpSegment::ack(local_port, dst_port, seq, ack, data.to_vec());
            if !send_tcp(&seg, src_ip, dst_ip, dst_mac) { return 0; }

            let new_seq = seq.wrapping_add(data.len() as u32);
            let acked = wait_for(|| {
                let t = SOCKET_TABLE.lock();
                t.sockets.get(&fd).and_then(|s| if s.seq_num == new_seq { Some(()) } else { None })
            });

            if acked.is_some() {
                data.len()
            } else {
                // Sem confirmação — assume enviado mesmo assim (sem
                // retransmissão nesta versão simplificada) e avança
                // o nosso próprio seq para não bloquear indefinidamente.
                if let Some(s) = SOCKET_TABLE.lock().sockets.get_mut(&fd) { s.seq_num = new_seq; }
                data.len()
            }
        }
        _ => 0,
    }
}

pub fn socket_recv(fd: SocketFd, buf: &mut [u8]) -> usize {
    SOCKET_TABLE.lock().recv(fd, buf)
}

/// Fecha o socket. TCP: envia FIN+ACK real e espera brevemente pela
/// confirmação do peer antes de libertar o socket (sem TIME_WAIT
/// completo — simplificação aceite para esta fase).
pub fn socket_close(fd: SocketFd) {
    let info = {
        let t = SOCKET_TABLE.lock();
        t.sockets.get(&fd).map(|s| (s.kind.clone(), s.state.clone(), s.local, s.remote, s.seq_num, s.ack_num))
    };

    if let Some((SocketKind::TcpClient, TcpState::Established, Some(SocketAddr::V4(src_ip, local_port)), Some(SocketAddr::V4(dst_ip, dst_port)), seq, ack)) = info {
        let dst_mac = crate::net::ethernet::arp_lookup(&dst_ip).unwrap_or(MacAddr::BROADCAST);
        let fin = TcpSegment::fin_ack(local_port, dst_port, seq, ack);
        send_tcp(&fin, src_ip, dst_ip, dst_mac);
        if let Some(s) = SOCKET_TABLE.lock().sockets.get_mut(&fd) { s.state = TcpState::FinWait1; }
        // Espera breve por ACK/FIN do peer — não bloqueia o fecho se não vier.
        let _ = wait_for(|| {
            let t = SOCKET_TABLE.lock();
            match t.sockets.get(&fd).map(|s| s.state.clone()) {
                Some(TcpState::Closed) | Some(TcpState::TimeWait) => Some(()),
                _ => None,
            }
        });
    }

    let mut t = SOCKET_TABLE.lock();
    if let Some(sock) = t.sockets.get_mut(&fd) { sock.state = TcpState::Closed; }
    t.sockets.remove(&fd);
}

/// Transmite um segmento TCP com checksum real
fn send_tcp(seg: &TcpSegment, src_ip: Ipv4Addr, dst_ip: Ipv4Addr, dst_mac: MacAddr) -> bool {
    let ip = Ipv4Packet::new(src_ip, dst_ip, IP_PROTO_TCP, seg.serialize(src_ip, dst_ip));
    let src_mac = MacAddr(crate::net::virtio_real::mac());
    let frame = EthernetFrame::new(dst_mac, src_mac, ETH_TYPE_IPV4, ip.serialize());
    crate::net::virtio_real::transmit(frame.serialize())
}

/// Entrega um segmento TCP recebido ao socket dono (avança o
/// handshake, entrega dados, trata FIN). Chamado pelo dispatcher
/// central — ver net::poll_and_dispatch.
///
/// Estrutura em dois passos para nunca segurar o lock do
/// SOCKET_TABLE enquanto se envia um pacote na rede: primeiro
/// extraem-se os dados necessários e decide-se a resposta (com o
/// lock), depois larga-se o lock e só então se transmite.
pub fn dispatch_tcp(src_ip: Ipv4Addr, seg: &TcpSegment) {
    // (porta_local, porta_dst, seq, ack) do ACK/resposta a enviar, se alguma
    let mut reply: Option<(u16, u16, u32, u32)> = None;

    {
        let mut t = SOCKET_TABLE.lock();
        let remote = SocketAddr::V4(src_ip, seg.src_port);
        let Some(sock) = t.find_owner(seg.dst_port, remote) else { return; };
        if sock.remote.is_none() { sock.remote = Some(remote); }

        if seg.flags & TCP_RST != 0 {
            sock.state = TcpState::Closed;
            return;
        }

        match &sock.state {
            TcpState::SynSent if seg.flags & (TCP_SYN | TCP_ACK) == (TCP_SYN | TCP_ACK) => {
                sock.ack_num = seg.seq_num.wrapping_add(1);
                sock.seq_num = seg.ack_num;
                sock.state = TcpState::Established;
                reply = Some((seg.dst_port, seg.src_port, sock.seq_num, sock.ack_num));
            }
            TcpState::Established => {
                if !seg.payload.is_empty() {
                    sock.rx_buf.extend_from_slice(&seg.payload);
                    sock.ack_num = seg.seq_num.wrapping_add(seg.payload.len() as u32);
                }
                if seg.flags & TCP_FIN != 0 {
                    sock.ack_num = sock.ack_num.wrapping_add(1);
                    sock.state = TcpState::CloseWait;
                    reply = Some((seg.dst_port, seg.src_port, sock.seq_num, sock.ack_num));
                } else if !seg.payload.is_empty() {
                    reply = Some((seg.dst_port, seg.src_port, sock.seq_num, sock.ack_num));
                }
            }
            TcpState::FinWait1 => {
                if seg.flags & TCP_FIN != 0 {
                    sock.state = TcpState::TimeWait;
                } else if seg.flags & TCP_ACK != 0 {
                    sock.state = TcpState::FinWait2;
                }
            }
            TcpState::FinWait2 if seg.flags & TCP_FIN != 0 => {
                sock.ack_num = seg.seq_num.wrapping_add(1);
                sock.state = TcpState::Closed;
                reply = Some((seg.dst_port, seg.src_port, sock.seq_num, sock.ack_num));
            }
            _ => {}
        }
    }

    if let Some((src_port, dst_port, seq, ack)) = reply {
        let Some(my_ip) = crate::net::get_primary_ip() else { return; };
        let dst_mac = arp_lookup(&src_ip).unwrap_or(MacAddr::BROADCAST);
        send_tcp(&TcpSegment::ack(src_port, dst_port, seq, ack, Vec::new()), my_ip, src_ip, dst_mac);
    }
}

/// Entrega um datagrama UDP recebido ao socket dono (se algum estiver
/// vinculado a essa porta local). Chamado pelo dispatcher central.
pub fn dispatch_udp(src_ip: Ipv4Addr, udp: &UdpPacket) {
    let mut t = SOCKET_TABLE.lock();
    let remote = SocketAddr::V4(src_ip, udp.src_port);
    if let Some(sock) = t.find_owner(udp.dst_port, remote) {
        sock.rx_buf.extend_from_slice(&udp.payload);
    }
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
