extern crate alloc;
use alloc::string::ToString;
// ============================================================
// SOC-D Kernel — Descoberta de Nós (mDNS Simulado)
// ============================================================
//
// O módulo de descoberta localiza outros nós SOC-D na rede.
//
// Fase 2 (atual): Simulação em memória da lógica mDNS
//   - Estrutura de pacotes mDNS correta (RFC 6762)
//   - Lógica de descoberta e anúncio implementada
//   - Sem sockets reais (kernel bare metal)
//
// Fase 3: Integração real
//   - UDP multicast 224.0.0.251:5353
//   - Service type: _socd._tcp.local
//   - Integração com driver de rede (virtio-net, e1000)
//
// Fluxo de descoberta:
//   1. Ao iniciar: envia mDNS Query para _socd._tcp.local
//   2. Nós SOC-D respondem com seu NodeAnnouncement
//   3. Cada resposta é adicionada à PeerTable
//   4. A cada 30s: re-envia Query para detectar novos nós
//   5. Cada nó anuncia sua presença periodicamente
// ============================================================

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;

/// Tipo de serviço mDNS do SOC-D
pub const SOCD_SERVICE_TYPE: &str = "_socd._tcp.local";

/// Porta padrão do protocolo SOC-D
pub const SOCD_PORT: u16 = 7700;

/// Intervalo de re-descoberta (em ticks do timer ~1ms = 30s)
pub const REDISCOVERY_INTERVAL_TICKS: u64 = 30_000;

// ─── Estrutura de Pacotes mDNS ────────────────────────────────────────────────

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

/// Registro DNS genérico
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub rtype: DnsRecordType,
    pub ttl: u32,
    pub data: Vec<u8>,
}

/// Pacote mDNS completo
#[derive(Debug, Clone)]
pub struct MdnsPacket {
    pub header: MdnsHeader,
    pub questions: Vec<MdnsQuestion>,
    pub answers: Vec<DnsRecord>,
    pub additional: Vec<DnsRecord>,
}

/// Questão mDNS
#[derive(Debug, Clone)]
pub struct MdnsQuestion {
    pub name: String,
    pub qtype: DnsRecordType,
    pub unicast_response: bool,
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
        _ipv4: &str,
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

    /// Processa um tick do sistema — verifica se deve re-descobrir
    pub fn tick(&mut self, current_tick: u64) {
        if !self.running { return; }

        let elapsed = current_tick.saturating_sub(self.last_query_tick);
        if elapsed >= REDISCOVERY_INTERVAL_TICKS {
            self.send_discovery_query(current_tick);
        }
    }

    /// Envia query de descoberta (simulado na Fase 2)
    fn send_discovery_query(&mut self, tick: u64) {
        let _pkt = MdnsPacket::discovery_query();

        // Fase 3: enviar UDP multicast para 224.0.0.251:5353
        // Por agora, simula respostas imediatas dos peers conhecidos
        self.queries_sent += 1;
        self.last_query_tick = tick;

        self.event_log.push(DiscoveryEvent {
            tick,
            kind: DiscoveryEventKind::QuerySent,
            peer_name: "broadcast".into(),
        });

        // Simula respostas dos peers inicializados em peer::init()
        self.simulate_responses(tick);

        crate::serial_println!("[P2P][DISC] Query #{} enviada", self.queries_sent);
    }

    /// Simula respostas de peers (Fase 2 — sem rede real)
    fn simulate_responses(&mut self, tick: u64) {
        let simulated = [
            "socd-phone",
            "socd-tablet",
            "socd-server",
        ];

        for name in &simulated {
            self.responses_received += 1;
            self.event_log.push(DiscoveryEvent {
                tick,
                kind: DiscoveryEventKind::ResponseReceived,
                peer_name: (*name).to_string(),
            });
        }

        // Mantém o log pequeno (últimos 50 eventos)
        if self.event_log.len() > 50 {
            let drain_count = self.event_log.len() - 50;
            self.event_log.drain(0..drain_count);
        }
    }
}

static DISCOVERY: Spinlock<DiscoveryEngine> = Spinlock::new(DiscoveryEngine::new());

pub fn init() {
    let mut disc = DISCOVERY.lock();
    disc.running = true;
    disc.send_discovery_query(0);
    crate::serial_println!("[P2P][DISC] Motor de descoberta ativo");
}

pub fn tick(current_tick: u64) {
    DISCOVERY.lock().tick(current_tick);
}

pub fn get_stats() -> (u64, u64) {
    let d = DISCOVERY.lock();
    (d.queries_sent, d.responses_received)
}
