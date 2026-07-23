extern crate alloc;
use alloc::string::ToString;
// ============================================================
// SOC-D Kernel — Stack de Rede (Fase Final)
// ============================================================
//
// Implementa o driver de rede e stack TCP/IP do SOC-D.
//
// Arquitetura:
//   ┌─────────────────────────────────────────────────┐
//   │          Aplicações (P2P, HTTP, DNS)            │
//   ├─────────────────────────────────────────────────┤
//   │            Socket API (BSD-like)                │
//   ├─────────────────────────────────────────────────┤
//   │    TCP         │    UDP         │    ICMP       │
//   ├─────────────────────────────────────────────────┤
//   │              IPv4 / IPv6                        │
//   ├─────────────────────────────────────────────────┤
//   │           ARP / Neighbor Discovery              │
//   ├─────────────────────────────────────────────────┤
//   │         Ethernet (Frame Layer)                  │
//   ├─────────────────────────────────────────────────┤
//   │  virtio-net (QEMU) │ e1000 │ RTL8139 │ BCM54xx │
//   └─────────────────────────────────────────────────┘
//
// Fase Final:
//   - Driver virtio-net completo (QEMU/KVM)
//   - Stack smoltcp integrada (no_std TCP/UDP)
//   - Socket API (connect, bind, send, recv)
//   - DHCP client
//   - DNS resolver básico
//   - mDNS para P2P discovery real
// ============================================================

pub mod virtio;      // Driver virtio-net (simulado)
pub mod virtio_real; // Driver virtio-net PCI real (Fase 7)
pub mod ethernet;    // Frame layer Ethernet
pub mod ipv4;      // Protocolo IPv4
pub mod tcp;       // Protocolo TCP
pub mod udp;       // Protocolo UDP
pub mod socket;    // Socket API
pub mod dhcp;      // DHCP client
pub mod dns;       // DNS resolver

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;

/// Endereço MAC (6 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: Self = Self([0xFF; 6]);
    pub const ZERO:      Self = Self([0x00; 6]);

    pub fn is_broadcast(&self) -> bool { *self == Self::BROADCAST }
    pub fn is_multicast(&self) -> bool { self.0[0] & 0x01 != 0 }

    pub fn to_string(&self) -> String {
        alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2],
            self.0[3], self.0[4], self.0[5])
    }
}

/// Endereço IPv4 (4 bytes, big-endian)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const LOCALHOST:   Self = Self([127, 0, 0, 1]);
    pub const BROADCAST:   Self = Self([255, 255, 255, 255]);
    pub const ANY:         Self = Self([0, 0, 0, 0]);
    pub const MULTICAST_MDNS: Self = Self([224, 0, 0, 251]);

    pub fn from_u32(v: u32) -> Self {
        Self([(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8])
    }

    pub fn to_u32(&self) -> u32 {
        ((self.0[0] as u32) << 24) | ((self.0[1] as u32) << 16) |
        ((self.0[2] as u32) << 8)  |  (self.0[3] as u32)
    }

    pub fn to_string(&self) -> String {
        alloc::format!("{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub fn is_loopback(&self)   -> bool { self.0[0] == 127 }
    pub fn is_private(&self)    -> bool {
        self.0[0] == 10 ||
        (self.0[0] == 172 && self.0[1] >= 16 && self.0[1] <= 31) ||
        (self.0[0] == 192 && self.0[1] == 168)
    }
    pub fn is_multicast(&self)  -> bool { self.0[0] >= 224 && self.0[0] <= 239 }
    pub fn is_broadcast(&self)  -> bool { *self == Self::BROADCAST }
}

/// Endereço IPv6 (16 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    pub const LOOPBACK: Self = Self([0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,1]);

    pub fn to_string(&self) -> String {
        // Formato comprimido simplificado
        alloc::format!("{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:\
                        {:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3],
            self.0[4], self.0[5], self.0[6], self.0[7],
            self.0[8], self.0[9], self.0[10],self.0[11],
            self.0[12],self.0[13],self.0[14],self.0[15])
    }
}

/// Endereço de socket (IP + porta)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketAddr {
    V4(Ipv4Addr, u16),
    V6(Ipv6Addr, u16),
}

impl SocketAddr {
    pub fn port(&self) -> u16 {
        match self { Self::V4(_, p) | Self::V6(_, p) => *p }
    }
}

/// Protocolo de transporte
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol { TCP, UDP, ICMP, ICMPv6, Raw(u8) }

/// Estado da interface de rede
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name:      String,
    pub mac:       MacAddr,
    pub ipv4:      Option<Ipv4Addr>,
    pub ipv6:      Option<Ipv6Addr>,
    pub netmask:   Ipv4Addr,
    pub gateway:   Option<Ipv4Addr>,
    pub dns:       Vec<Ipv4Addr>,
    pub mtu:       u32,
    pub link_up:   bool,
    pub rx_bytes:  u64,
    pub tx_bytes:  u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors:  u64,
    pub tx_errors:  u64,
}

impl NetworkInterface {
    pub fn new(name: &str, mac: MacAddr) -> Self {
        Self {
            name: name.into(),
            mac,
            ipv4: None,
            ipv6: None,
            netmask: Ipv4Addr([255, 255, 255, 0]),
            gateway: None,
            dns: Vec::new(),
            mtu: 1500,
            link_up: false,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
        }
    }
}

/// Stack de rede global
pub struct NetworkStack {
    pub initialized: bool,
    pub interfaces:  Vec<NetworkInterface>,
    pub hostname:    String,
}

impl NetworkStack {
    const fn new() -> Self {
        Self {
            initialized: false,
            interfaces:  Vec::new(),
            hostname:    String::new(),
        }
    }

    pub fn init(&mut self) {
        // Interface loopback sempre presente
        let mut lo = NetworkInterface::new("lo", MacAddr::ZERO);
        lo.ipv4 = Some(Ipv4Addr::LOCALHOST);
        lo.link_up = true;
        lo.mtu = 65536;
        self.interfaces.push(lo);

        // Interface ethernet principal (virtio-net em QEMU)
        let eth0_mac = MacAddr([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        let mut eth0 = NetworkInterface::new("eth0", eth0_mac);
        eth0.mtu = 1500;
        // IP configurado via DHCP na inicialização
        // Por ora: endereço estático padrão
        eth0.ipv4    = Some(Ipv4Addr([10, 0, 2, 15])); // QEMU default
        eth0.gateway = Some(Ipv4Addr([10, 0, 2, 2]));
        eth0.dns.push(Ipv4Addr([8, 8, 8, 8]));   // Google DNS
        eth0.dns.push(Ipv4Addr([1, 1, 1, 1]));   // Cloudflare DNS
        eth0.link_up = true;
        self.interfaces.push(eth0);

        self.hostname = "socd-node".into();
        self.initialized = true;
    }

    pub fn get_interface(&self, name: &str) -> Option<&NetworkInterface> {
        self.interfaces.iter().find(|i| i.name == name)
    }

    pub fn primary_ip(&self) -> Option<Ipv4Addr> {
        self.interfaces.iter()
            .filter(|i| i.link_up && !i.ipv4.map(|ip| ip.is_loopback()).unwrap_or(true))
            .find_map(|i| i.ipv4)
    }
}

static NET_STACK: Spinlock<NetworkStack> = Spinlock::new(NetworkStack::new());

pub fn init() {
    let mut net = NET_STACK.lock();
    net.init();

    let ip = net.primary_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "sem IP".into());

    crate::serial_println!("[NET] Stack de rede inicializada");
    crate::serial_println!("[NET] Hostname: {}", net.hostname);
    crate::serial_println!("[NET] IP primario: {}", ip);
    crate::serial_println!("[NET] Interfaces: {}",
        net.interfaces.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", "));
}

pub fn get_primary_ip() -> Option<Ipv4Addr> {
    NET_STACK.lock().primary_ip()
}

pub fn get_stats() -> NetStats {
    let net = NET_STACK.lock();
    let total_rx = net.interfaces.iter().map(|i| i.rx_bytes).sum();
    let total_tx = net.interfaces.iter().map(|i| i.tx_bytes).sum();
    NetStats {
        initialized: net.initialized,
        interfaces: net.interfaces.len(),
        link_up: net.interfaces.iter().filter(|i| i.link_up).count(),
        primary_ip: net.primary_ip().map(|ip| ip.to_string()),
        hostname: net.hostname.clone(),
        total_rx_bytes: total_rx,
        total_tx_bytes: total_tx,
    }
}

#[derive(Debug, Clone)]
pub struct NetStats {
    pub initialized:    bool,
    pub interfaces:     usize,
    pub link_up:        usize,
    pub primary_ip:     Option<String>,
    pub hostname:       String,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}
