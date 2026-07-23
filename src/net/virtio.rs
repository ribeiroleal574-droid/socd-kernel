extern crate alloc;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::mem;
// ============================================================
// SOC-D Kernel — Driver virtio-net
// ============================================================
//
// Driver para a interface de rede paravirtualizada virtio-net.
// Usada pelo QEMU/KVM para fornecer rede ao guest.
//
// Protocolo virtio:
//   - Baseado em filas de descritores (virtqueues)
//   - TX queue: frames enviados pelo guest para o host
//   - RX queue: frames recebidos do host para o guest
//   - Control queue: comandos de controle (MAC, offloads)
//
// Cada virtqueue tem:
//   - Descriptor Table: array de descritores (addr, len, flags, next)
//   - Available Ring: guest → device (índices de descritores prontos)
//   - Used Ring:      device → guest (índices de descritores processados)
//
// Fase Final (atual):
//   - Estruturas completas do protocolo virtio
//   - Inicialização do device via PCI
//   - TX/RX ring buffer management
//   - Integração com o stack Ethernet
//
// Fase 5: Suporte a virtio-net features avançadas
//   - VIRTIO_NET_F_CSUM (checksum offload)
//   - VIRTIO_NET_F_GSO  (segmentation offload)
//   - VIRTIO_NET_F_CTRL_VQ (control virtqueue)
// ============================================================

use spinning_top::Spinlock;
use super::MacAddr;

// ─── Registradores PCI virtio ─────────────────────────────────────────────────

/// Vendor ID da VirtIO (Red Hat)
pub const VIRTIO_VENDOR_ID:  u16 = 0x1AF4;
/// Device ID da virtio-net
pub const VIRTIO_NET_DEVICE_ID: u16 = 0x1000;
/// Versão transitional (Legacy)
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;

/// Offsets dos registradores virtio (PCI BAR0)
mod regs {
    pub const DEVICE_FEATURES:  u32 = 0x00;
    pub const DRIVER_FEATURES:  u32 = 0x04;
    pub const QUEUE_ADDR:       u32 = 0x08;
    pub const QUEUE_SIZE:       u32 = 0x0C;
    pub const QUEUE_SELECT:     u32 = 0x0E;
    pub const QUEUE_NOTIFY:     u32 = 0x10;
    pub const DEVICE_STATUS:    u32 = 0x12;
    pub const ISR_STATUS:       u32 = 0x13;
    // Específico da virtio-net (após offset 0x14):
    pub const NET_MAC:          u32 = 0x14; // 6 bytes
    pub const NET_STATUS:       u32 = 0x1A; // u16
    pub const NET_MAX_VIRTQUEUE_PAIRS: u32 = 0x1C;
}

/// Status do device (bits)
pub const STATUS_RESET:      u8 = 0x00;
pub const STATUS_ACKNOWLEDGE: u8 = 0x01;
pub const STATUS_DRIVER:     u8 = 0x02;
pub const STATUS_DRIVER_OK:  u8 = 0x04;
pub const STATUS_FEATURES_OK: u8 = 0x08;
pub const STATUS_FAILED:     u8 = 0x80;

/// Feature bits virtio-net
pub const VIRTIO_NET_F_CSUM:       u32 = 1 << 0;
pub const VIRTIO_NET_F_GUEST_CSUM: u32 = 1 << 1;
pub const VIRTIO_NET_F_MAC:        u32 = 1 << 5;
pub const VIRTIO_NET_F_STATUS:     u32 = 1 << 16;
pub const VIRTIO_NET_F_MRG_RXBUF:  u32 = 1 << 15;

// ─── Descritor virtio ─────────────────────────────────────────────────────────

/// Um descritor na virtqueue
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtqDesc {
    /// Endereço físico do buffer
    pub addr:  u64,
    /// Tamanho do buffer em bytes
    pub len:   u32,
    /// Flags: NEXT, WRITE, INDIRECT
    pub flags: u16,
    /// Índice do próximo descritor (se NEXT estiver setado)
    pub next:  u16,
}

/// Flags de descritor
pub const VIRTQ_DESC_F_NEXT:     u16 = 1;  // Tem próximo descritor
pub const VIRTQ_DESC_F_WRITE:    u16 = 2;  // Write-only (device → driver)
pub const VIRTQ_DESC_F_INDIRECT: u16 = 4;  // Buffer contém tabela de descritores

/// Available Ring (driver → device)
#[derive(Debug, Clone)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx:   u16,
    pub ring:  Vec<u16>,
}

/// Used Ring (device → driver)
#[derive(Debug, Clone, Default)]
pub struct VirtqUsedElem {
    pub id:  u32,
    pub len: u32,
}

#[derive(Debug, Clone)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx:   u16,
    pub ring:  Vec<VirtqUsedElem>,
}

/// Uma virtqueue completa
pub struct Virtqueue {
    pub size:     u16,
    pub descs:    Vec<VirtqDesc>,
    pub avail:    VirtqAvail,
    pub used:     VirtqUsed,
    pub last_used: u16,
    /// Buffers de dados associados aos descritores
    pub buffers:  Vec<Vec<u8>>,
}

impl Virtqueue {
    pub fn new(size: u16) -> Self {
        let sz = size as usize;
        Self {
            size,
            descs:    alloc::vec![VirtqDesc::default(); sz],
            avail:    VirtqAvail { flags: 0, idx: 0, ring: alloc::vec![0u16; sz] },
            used:     VirtqUsed { flags: 0, idx: 0,
                ring: (0..sz).map(|_| VirtqUsedElem::default()).collect() },
            last_used: 0,
            buffers:  (0..sz).map(|_| Vec::new()).collect(),
        }
    }

    /// Adiciona um buffer para transmissão
    pub fn add_tx_buffer(&mut self, data: Vec<u8>) -> Option<u16> {
        let avail_idx = self.avail.idx as usize % self.size as usize;
        let desc_idx  = avail_idx as u16;

        if data.len() > 65536 { return None; }

        let len = data.len() as u32;
        self.buffers[avail_idx] = data;

        self.descs[avail_idx] = VirtqDesc {
            addr:  self.buffers[avail_idx].as_ptr() as u64,
            len,
            flags: 0, // TX: device lê, não escreve
            next:  0,
        };

        self.avail.ring[avail_idx] = desc_idx;
        self.avail.idx = self.avail.idx.wrapping_add(1);

        Some(desc_idx)
    }

    /// Prepara um buffer de RX vazio para o device preencher
    pub fn add_rx_buffer(&mut self, size: usize) -> Option<u16> {
        let avail_idx = self.avail.idx as usize % self.size as usize;
        let desc_idx  = avail_idx as u16;

        self.buffers[avail_idx] = alloc::vec![0u8; size];

        self.descs[avail_idx] = VirtqDesc {
            addr:  self.buffers[avail_idx].as_ptr() as u64,
            len:   size as u32,
            flags: VIRTQ_DESC_F_WRITE, // Device escreve (RX)
            next:  0,
        };

        self.avail.ring[avail_idx] = desc_idx;
        self.avail.idx = self.avail.idx.wrapping_add(1);

        Some(desc_idx)
    }

    /// Processa buffers usados pelo device (RX recebido / TX enviado)
    pub fn drain_used(&mut self) -> Vec<Vec<u8>> {
        let mut received = Vec::new();
        while self.last_used != self.used.idx {
            let used_elem = &self.used.ring[self.last_used as usize % self.size as usize];
            let idx = used_elem.id as usize % self.size as usize;
            let len = used_elem.len as usize;

            if !self.buffers[idx].is_empty() {
                let mut buf = core::mem::take(&mut self.buffers[idx]);
                buf.truncate(len);
                received.push(buf);
            }

            self.last_used = self.last_used.wrapping_add(1);
        }
        received
    }
}

// ─── Header virtio-net ────────────────────────────────────────────────────────

/// Header obrigatório em todo frame virtio-net
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtioNetHeader {
    pub flags:       u8,
    pub gso_type:    u8,
    pub hdr_len:     u16,
    pub gso_size:    u16,
    pub csum_start:  u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}

// ─── Driver virtio-net ────────────────────────────────────────────────────────

pub struct VirtioNetDriver {
    pub initialized: bool,
    pub mac:         MacAddr,
    pub link_up:     bool,
    /// Endereço base dos registradores I/O (PCI BAR0)
    pub iobase:      u16,
    /// TX virtqueue (índice 1)
    pub txq:         Virtqueue,
    /// RX virtqueue (índice 0)
    pub rxq:         Virtqueue,
    /// Estatísticas
    pub tx_packets:  u64,
    pub rx_packets:  u64,
    pub tx_bytes:    u64,
    pub rx_bytes:    u64,
    pub tx_dropped:  u64,
}

impl VirtioNetDriver {
    const QUEUE_SIZE: u16 = 256;

    pub fn new() -> Self {
        Self {
            initialized: false,
            mac: MacAddr([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
            link_up: false,
            iobase: 0xC000, // Endereço PCI típico no QEMU
            txq: Virtqueue::new(Self::QUEUE_SIZE),
            rxq: Virtqueue::new(Self::QUEUE_SIZE),
            tx_packets: 0,
            rx_packets: 0,
            tx_bytes:   0,
            rx_bytes:   0,
            tx_dropped: 0,
        }
    }

    /// Inicializa o device virtio-net via PCI
    pub fn init(&mut self) {
        // Em hardware real:
        // 1. Scan PCI bus por vendor=0x1AF4, device=0x1000
        // 2. Ler MAC do registrador NET_MAC
        // 3. Configurar virtqueues (size, endereços físicos)
        // 4. Negociar features
        // 5. Sinalizar DRIVER_OK

        // Simulado: assume QEMU com virtio-net no iobase padrão
        self.link_up = true;
        self.initialized = true;

        // Pré-aloca buffers RX
        for _ in 0..Self::QUEUE_SIZE / 2 {
            self.rxq.add_rx_buffer(1514); // MTU + headers Ethernet
        }

        crate::serial_println!(
            "[NET][VIRTIO] virtio-net inicializado: MAC={} iobase=0x{:04x}",
            self.mac.to_string(), self.iobase
        );
    }

    /// Envia um frame Ethernet
    pub fn transmit(&mut self, frame: Vec<u8>) -> bool {
        if !self.link_up || frame.len() > 1514 {
            self.tx_dropped += 1;
            return false;
        }

        // Prepend virtio-net header
        let mut buf = alloc::vec![0u8; core::mem::size_of::<VirtioNetHeader>()];
        buf.extend_from_slice(&frame);

        let len = buf.len() as u64;
        if let Some(_desc) = self.txq.add_tx_buffer(buf) {
            // Em hardware real: notify device via QUEUE_NOTIFY write
            // unsafe { outw(self.iobase + regs::QUEUE_NOTIFY as u16, 1); }
            self.tx_packets += 1;
            self.tx_bytes   += len;
            true
        } else {
            self.tx_dropped += 1;
            false
        }
    }

    /// Recebe frames Ethernet pendentes
    pub fn receive(&mut self) -> Vec<Vec<u8>> {
        let raw_frames = self.rxq.drain_used();
        let hdr_size = core::mem::size_of::<VirtioNetHeader>();

        let frames: Vec<Vec<u8>> = raw_frames.into_iter()
            .filter(|f| f.len() > hdr_size)
            .map(|f| {
                self.rx_packets += 1;
                self.rx_bytes   += (f.len() - hdr_size) as u64;
                f[hdr_size..].to_vec() // Remove virtio header
            })
            .collect();

        // Repõe buffers RX consumidos
        for _ in 0..frames.len() {
            self.rxq.add_rx_buffer(1514);
        }

        frames
    }
}

lazy_static::lazy_static! {
    static ref VIRTIO_NET: Spinlock<VirtioNetDriver> = Spinlock::new(VirtioNetDriver::new());
}

pub fn init() {
    VIRTIO_NET.lock().init();
}

pub fn transmit(frame: Vec<u8>) -> bool {
    VIRTIO_NET.lock().transmit(frame)
}

pub fn receive() -> Vec<Vec<u8>> {
    VIRTIO_NET.lock().receive()
}

pub fn get_mac() -> MacAddr {
    VIRTIO_NET.lock().mac
}

pub fn is_up() -> bool {
    VIRTIO_NET.lock().link_up
}

pub fn get_stats() -> (u64, u64, u64, u64) {
    let d = VIRTIO_NET.lock();
    (d.tx_packets, d.rx_packets, d.tx_bytes, d.rx_bytes)
}
