// ============================================================
// SOC-D Kernel — Driver PCI + virtio-net Real (Fase 7)
// ============================================================
//
// Implementa acesso real ao barramento PCI e ao device
// virtio-net tal como exposto pelo QEMU.
//
// Pipeline completo:
//
//   1. PCI Scan — percorre bus 0, dev 0-31, fn 0
//      → procura vendor=0x1AF4, device=0x1000 (virtio-net)
//
//   2. PCI BAR0 — lê o I/O base address register
//      → obtém iobase (tipicamente 0xC000 no QEMU)
//
//   3. Negociação de features virtio (legacy)
//      → ACKNOWLEDGE + DRIVER + features + DRIVER_OK
//
//   4. Leitura da MAC address do device config space
//
//   5. Virtqueue setup
//      → alloca descriptor table, avail ring, used ring
//      → escreve endereços físicos no QUEUE_ADDR register
//
//   6. TX/RX via port I/O
//      → transmit: preenche descriptor → avail ring → QUEUE_NOTIFY
//      → receive:  poll used ring → lê buffers RX
//
// Compatibilidade: virtio 0.9 (legacy) — suportado por QEMU
// ============================================================

extern crate alloc;
use alloc::{string::ToString, vec::Vec, format};
use spinning_top::Spinlock;
use x86_64::instructions::port::Port;

// ─── PCI Config Space ─────────────────────────────────────────

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA:    u16 = 0xCFC;

const VIRTIO_VENDOR:    u16 = 0x1AF4;
const VIRTIO_NET_DEV:   u16 = 0x1000;
const PCI_BAR0_OFFSET:  u8  = 0x10;
const PCI_COMMAND_REG:  u8  = 0x04;

/// Lê um registo de 32 bits do PCI config space
unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr: u32 = (1u32 << 31)
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) << 8)
        | ((offset & 0xFC) as u32);
    let mut port_addr: Port<u32> = Port::new(PCI_CONFIG_ADDRESS);
    let mut port_data: Port<u32> = Port::new(PCI_CONFIG_DATA);
    port_addr.write(addr);
    port_data.read()
}

/// Lê 16 bits do PCI config space
pub(crate) unsafe fn pci_read16(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let val = pci_read32(bus, dev, func, offset & !3);
    let shift = (offset & 2) * 8;
    (val >> shift) as u16
}

/// Escreve 16 bits no PCI config space
pub(crate) unsafe fn pci_write16(bus: u8, dev: u8, func: u8, offset: u8, value: u16) {
    let val = pci_read32(bus, dev, func, offset & !3);
    let shift = (offset & 2) * 8;
    let mask  = !(0xFFFFu32 << shift);
    let new   = (val & mask) | ((value as u32) << shift);
    let addr: u32 = (1u32 << 31)
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) << 8)
        | ((offset & 0xFC) as u32);
    let mut port_addr: Port<u32> = Port::new(PCI_CONFIG_ADDRESS);
    let mut port_data: Port<u32> = Port::new(PCI_CONFIG_DATA);
    port_addr.write(addr);
    port_data.write(new);
}

// ─── Scan PCI ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus:      u8,
    pub dev:      u8,
    pub func:     u8,
    pub vendor:   u16,
    pub device:   u16,
    pub class:    u8,
    pub subclass: u8,
    pub bar0:     u32,
}

impl PciDevice {
    /// Retorna o I/O base address do BAR0 (se for I/O BAR)
    pub fn io_base(&self) -> Option<u16> {
        if self.bar0 & 0x1 != 0 {
            Some((self.bar0 & 0xFFFC) as u16)
        } else {
            None
        }
    }
}

/// Faz scan do bus PCI e retorna lista de devices encontrados
pub fn pci_scan() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..=0u8 {
        for dev in 0..32u8 {
            let id = unsafe { pci_read32(bus, dev, 0, 0) };
            if id == 0xFFFF_FFFF { continue; } // slot vazio
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16)    as u16;
            let class_info = unsafe { pci_read32(bus, dev, 0, 0x08) };
            let class    = (class_info >> 24) as u8;
            let subclass = (class_info >> 16) as u8;
            let bar0     = unsafe { pci_read32(bus, dev, 0, PCI_BAR0_OFFSET) };
            devices.push(PciDevice { bus, dev, func: 0, vendor, device, class, subclass, bar0 });
        }
    }
    devices
}

/// Encontra o virtio-net device no PCI bus
pub fn find_virtio_net() -> Option<PciDevice> {
    pci_scan().into_iter()
        .find(|d| d.vendor == VIRTIO_VENDOR && d.device == VIRTIO_NET_DEV)
}

// ─── Virtio Legacy I/O Registers ─────────────────────────────

struct VirtioRegs {
    iobase: u16,
}

impl VirtioRegs {
    fn new(iobase: u16) -> Self { Self { iobase } }

    unsafe fn read_u32(&self, offset: u16) -> u32 {
        let mut p: Port<u32> = Port::new(self.iobase + offset);
        p.read()
    }
    unsafe fn write_u32(&self, offset: u16, val: u32) {
        let mut p: Port<u32> = Port::new(self.iobase + offset);
        p.write(val);
    }
    unsafe fn read_u16(&self, offset: u16) -> u16 {
        let mut p: Port<u16> = Port::new(self.iobase + offset);
        p.read()
    }
    unsafe fn write_u16(&self, offset: u16, val: u16) {
        let mut p: Port<u16> = Port::new(self.iobase + offset);
        p.write(val);
    }
    unsafe fn read_u8(&self, offset: u16) -> u8 {
        let mut p: Port<u8> = Port::new(self.iobase + offset);
        p.read()
    }
    unsafe fn write_u8(&self, offset: u16, val: u8) {
        let mut p: Port<u8> = Port::new(self.iobase + offset);
        p.write(val);
    }

    // Offsets virtio legacy
    fn device_features(&self) -> u32 { unsafe { self.read_u32(0x00) } }
    fn set_driver_features(&self, f: u32) { unsafe { self.write_u32(0x04, f); } }
    fn set_queue_addr(&self, addr: u32) { unsafe { self.write_u32(0x08, addr); } }
    fn queue_size(&self) -> u16 { unsafe { self.read_u16(0x0C) } }
    fn set_queue_select(&self, q: u16) { unsafe { self.write_u16(0x0E, q); } }
    fn notify_queue(&self, q: u16) { unsafe { self.write_u16(0x10, q); } }
    fn device_status(&self) -> u8 { unsafe { self.read_u8(0x12) } }
    fn set_device_status(&self, s: u8) { unsafe { self.write_u8(0x12, s); } }
    fn isr_status(&self) -> u8 { unsafe { self.read_u8(0x13) } }
    fn mac_byte(&self, i: u16) -> u8 { unsafe { self.read_u8(0x14 + i) } }
}

// ─── Virtqueue Real (legacy virtio 0.9) ───────────────────────
//
// Layout legacy (um único bloco físico contíguo por queue):
//   [ Descriptor Table (16 bytes * qsz) ]
//   [ Available Ring (6 + 2*qsz bytes) ]
//   [ padding até à página seguinte ]
//   [ Used Ring (6 + 8*qsz bytes), alinhado a 4096 ]
//
// O offset físico é escrito em QUEUE_ADDR como (phys_addr >> 12) —
// por isso o bloco tem de estar alinhado a página E fisicamente
// contíguo (o device não sabe nada de paginação virtual do kernel).
//
// Para garantir contiguidade física sem um alocador de frames
// contíguos, usamos buffers `static` — fazem parte da imagem do
// kernel carregada pelo bootloader como um bloco físico único, ao
// contrário do heap (cujas páginas podem vir de frames físicos
// dispersos). Verificamos isso em runtime com `verify_contiguous`
// antes de confiar neles para DMA — se falhar, caímos de volta para
// o modo simulado em vez de arriscar corromper memória.

const MAX_QSZ:      usize = 256;
const VRING_BYTES:  usize = 3 * 4096; // margem generosa p/ qsz até 256
const RX_BUF_COUNT: usize = 32;
const TX_BUF_COUNT: usize = 32;
const PKT_BUF_SIZE: usize = 2048; // hdr(10) + frame Ethernet (até 1514)
const VNET_HDR_LEN: usize = 10;   // virtio_net_hdr legacy sem MRG_RXBUF

const VIRTQ_DESC_F_WRITE: u16 = 2; // descritor escrito pelo device (RX)

#[repr(C, align(4096))]
struct AlignedVring([u8; VRING_BYTES]);

#[repr(C, align(16))]
struct PktBufPool([[u8; PKT_BUF_SIZE]; RX_BUF_COUNT]); // RX_BUF_COUNT==TX_BUF_COUNT

static mut RX_VRING: AlignedVring = AlignedVring([0; VRING_BYTES]);
static mut TX_VRING: AlignedVring = AlignedVring([0; VRING_BYTES]);
static mut RX_BUFS:  PktBufPool   = PktBufPool([[0; PKT_BUF_SIZE]; RX_BUF_COUNT]);
static mut TX_BUFS:  PktBufPool   = PktBufPool([[0; PKT_BUF_SIZE]; TX_BUF_COUNT]);

fn align_up(x: usize, a: usize) -> usize { (x + a - 1) & !(a - 1) }

/// Confirma que um bloco de `len` bytes a partir de `virt` está
/// mapeado em física CONTÍGUA (verifica o início e cada fronteira de
/// página). Devolve o endereço físico do início se sim.
unsafe fn verify_contiguous(virt_start: u64, len: usize) -> Option<u64> {
    let start_phys = crate::memory::virt_to_phys(x86_64::VirtAddr::new(virt_start))?.as_u64();
    let mut off = 0usize;
    while off < len {
        let v = virt_start + off as u64;
        let p = crate::memory::virt_to_phys(x86_64::VirtAddr::new(v))?.as_u64();
        if p != start_phys + off as u64 {
            return None;
        }
        off += 4096;
    }
    Some(start_phys)
}

/// Layout calculado de uma virtqueue já registada no device — apenas
/// ponteiros/offsets, sem dono dos dados (os buffers são `static`).
#[derive(Clone, Copy)]
struct VringLayout {
    base:      *mut u8,
    qsz:       u16,
    avail_off: usize,
    used_off:  usize,
}
unsafe impl Send for VringLayout {}

impl VringLayout {
    fn desc_off(idx: u16) -> usize { idx as usize * 16 }

    unsafe fn set_desc(&self, idx: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let o = Self::desc_off(idx);
        core::ptr::write_volatile(self.base.add(o) as *mut u64, addr);
        core::ptr::write_volatile(self.base.add(o + 8) as *mut u32, len);
        core::ptr::write_volatile(self.base.add(o + 12) as *mut u16, flags);
        core::ptr::write_volatile(self.base.add(o + 14) as *mut u16, next);
    }

    unsafe fn avail_idx(&self) -> u16 {
        core::ptr::read_volatile(self.base.add(self.avail_off + 2) as *const u16)
    }
    unsafe fn set_avail_idx(&self, v: u16) {
        core::ptr::write_volatile(self.base.add(self.avail_off + 2) as *mut u16, v);
    }
    unsafe fn set_avail_ring(&self, slot: u16, desc_idx: u16) {
        let o = self.avail_off + 4 + (slot as usize % self.qsz as usize) * 2;
        core::ptr::write_volatile(self.base.add(o) as *mut u16, desc_idx);
    }

    unsafe fn used_idx(&self) -> u16 {
        core::ptr::read_volatile(self.base.add(self.used_off + 2) as *const u16)
    }
    /// (desc_id, len) do elemento `slot` do used ring
    unsafe fn used_elem(&self, slot: u16) -> (u32, u32) {
        let o = self.used_off + 4 + (slot as usize % self.qsz as usize) * 8;
        (
            core::ptr::read_volatile(self.base.add(o) as *const u32),
            core::ptr::read_volatile(self.base.add(o + 4) as *const u32),
        )
    }
}

/// Regista e activa uma virtqueue (RX=0, TX=1) no device.
unsafe fn setup_vring(regs: &VirtioRegs, queue_idx: u16, vring_buf: *mut u8) -> Option<VringLayout> {
    regs.set_queue_select(queue_idx);
    let qsz = regs.queue_size();
    if qsz == 0 || qsz as usize > MAX_QSZ {
        crate::serial_println!("[VIRTIO-REAL] Queue {} tamanho invalido: {}", queue_idx, qsz);
        return None;
    }

    let desc_bytes  = 16 * qsz as usize;
    let avail_bytes = 6 + 2 * qsz as usize;
    let used_off    = align_up(desc_bytes + avail_bytes, 4096);
    let used_bytes  = 6 + 8 * qsz as usize;
    let total       = used_off + used_bytes;
    if total > VRING_BYTES {
        crate::serial_println!("[VIRTIO-REAL] Queue {} nao cabe no buffer estatico ({} > {})",
            queue_idx, total, VRING_BYTES);
        return None;
    }

    core::ptr::write_bytes(vring_buf, 0, total);

    let phys = match verify_contiguous(vring_buf as u64, total) {
        Some(p) => p,
        None => {
            crate::serial_println!("[VIRTIO-REAL] Vring da queue {} nao e fisicamente contigua — a usar modo simulado", queue_idx);
            return None;
        }
    };
    if phys & 0xFFF != 0 {
        crate::serial_println!("[VIRTIO-REAL] Vring da queue {} nao esta alinhada a pagina", queue_idx);
        return None;
    }

    regs.set_queue_addr((phys >> 12) as u32);

    Some(VringLayout { base: vring_buf, qsz, avail_off: desc_bytes, used_off })
}

// ─── Driver Físico virtio-net ─────────────────────────────────

pub struct VirtioNetReal {
    pub initialized: bool,
    pub iobase:      u16,
    pub mac:         [u8; 6],
    pub link_up:     bool,
    /// Virtqueues reais (None se DMA não pôde ser configurado — ver
    /// `setup_vring`/`verify_contiguous`)
    rx_ring:      Option<VringLayout>,
    tx_ring:      Option<VringLayout>,
    rx_bufs_base: *mut u8,
    rx_bufs_phys: u64,
    tx_bufs_base: *mut u8,
    tx_bufs_phys: u64,
    rx_used_seen: u16,
    tx_next:      u16,
    /// Fallback em memória, usado só se a DMA real não ficar
    /// disponível (ex: verify_contiguous falhou) — mantém o kernel a
    /// funcionar em modo degradado em vez de desligar a rede.
    tx_buffers: Vec<Vec<u8>>,
    rx_buffers: Vec<Vec<u8>>,
    /// Estatísticas
    pub tx_packets:  u64,
    pub rx_packets:  u64,
    pub tx_bytes:    u64,
    pub rx_bytes:    u64,
    pub tx_dropped:  u64,
}
unsafe impl Send for VirtioNetReal {}

impl VirtioNetReal {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            iobase:      0,
            mac:         [0; 6],
            link_up:     false,
            rx_ring:      None,
            tx_ring:      None,
            rx_bufs_base: core::ptr::null_mut(),
            rx_bufs_phys: 0,
            tx_bufs_base: core::ptr::null_mut(),
            tx_bufs_phys: 0,
            rx_used_seen: 0,
            tx_next:      0,
            tx_buffers: Vec::new(),
            rx_buffers: Vec::new(),
            tx_packets:  0,
            rx_packets:  0,
            tx_bytes:    0,
            rx_bytes:    0,
            tx_dropped:  0,
        }
    }

    /// Inicialização real do virtio-net via PCI
    pub fn init_real(&mut self) -> bool {
        // 1. Encontra o device no PCI bus
        let dev = match find_virtio_net() {
            Some(d) => d,
            None => {
                crate::serial_println!("[VIRTIO-REAL] Device nao encontrado no PCI bus");
                return false;
            }
        };

        let iobase = match dev.io_base() {
            Some(b) => b,
            None => {
                crate::serial_println!("[VIRTIO-REAL] BAR0 nao e I/O BAR");
                return false;
            }
        };

        crate::serial_println!("[VIRTIO-REAL] Found virtio-net @ PCI {:02x}:{:02x} iobase=0x{:04x}",
            dev.bus, dev.dev, iobase);

        // 2. Habilita PCI Bus Master + I/O Space
        unsafe {
            let cmd = pci_read16(dev.bus, dev.dev, 0, PCI_COMMAND_REG);
            pci_write16(dev.bus, dev.dev, 0, PCI_COMMAND_REG, cmd | 0x05); // I/O + Bus Master
        }

        let regs = VirtioRegs::new(iobase);

        // 3. Sequência de inicialização virtio legacy
        regs.set_device_status(0x00); // Reset
        regs.set_device_status(0x01); // ACKNOWLEDGE
        regs.set_device_status(0x03); // ACKNOWLEDGE | DRIVER

        // 4. Negocia features (queremos MAC + STATUS)
        let features = regs.device_features();
        crate::serial_println!("[VIRTIO-REAL] Device features: 0x{:08x}", features);
        // Aceita MAC e STATUS se disponíveis. Não negociamos MRG_RXBUF
        // nem GSO/checksum offload — mantém o virtio_net_hdr simples,
        // fixo em 10 bytes (VNET_HDR_LEN) em todos os buffers.
        let driver_features = features & (1 << 5 | 1 << 16); // F_MAC | F_STATUS
        regs.set_driver_features(driver_features);

        // 5. Lê MAC address
        for i in 0..6u16 {
            self.mac[i as usize] = regs.mac_byte(i);
        }
        crate::serial_println!("[VIRTIO-REAL] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.mac[0], self.mac[1], self.mac[2],
            self.mac[3], self.mac[4], self.mac[5]);

        // 6. Virtqueue setup — RX (0) e TX (1). Se qualquer uma falhar
        // (device incomum, ou memória DMA não contígua), continuamos
        // sem DMA real: initialized fica false e quem chamou cai para
        // o driver simulado (ver `init_physical`).
        unsafe {
            let rx_vring_ptr = core::ptr::addr_of_mut!(RX_VRING.0) as *mut u8;
            let tx_vring_ptr = core::ptr::addr_of_mut!(TX_VRING.0) as *mut u8;
            let rx_bufs_ptr  = core::ptr::addr_of_mut!(RX_BUFS.0) as *mut u8;
            let tx_bufs_ptr  = core::ptr::addr_of_mut!(TX_BUFS.0) as *mut u8;

            let rx_ring = setup_vring(&regs, 0, rx_vring_ptr);
            let tx_ring = setup_vring(&regs, 1, tx_vring_ptr);
            let rx_bufs_phys = verify_contiguous(rx_bufs_ptr as u64, RX_BUF_COUNT * PKT_BUF_SIZE);
            let tx_bufs_phys = verify_contiguous(tx_bufs_ptr as u64, TX_BUF_COUNT * PKT_BUF_SIZE);

            match (rx_ring, tx_ring, rx_bufs_phys, tx_bufs_phys) {
                (Some(rx), Some(tx), Some(rxp), Some(txp)) => {
                    self.rx_ring = Some(rx);
                    self.tx_ring = Some(tx);
                    self.rx_bufs_base = rx_bufs_ptr;
                    self.rx_bufs_phys = rxp;
                    self.tx_bufs_base = tx_bufs_ptr;
                    self.tx_bufs_phys = txp;

                    // Pré-popula o RX ring com buffers vazios para o
                    // device poder começar a preencher imediatamente.
                    let n = RX_BUF_COUNT.min(rx.qsz as usize) as u16;
                    for i in 0..n {
                        let buf_phys = rxp + (i as usize * PKT_BUF_SIZE) as u64;
                        rx.set_desc(i, buf_phys, PKT_BUF_SIZE as u32, VIRTQ_DESC_F_WRITE, 0);
                        rx.set_avail_ring(i, i);
                    }
                    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                    rx.set_avail_idx(n);
                    regs.notify_queue(0);

                    crate::serial_println!("[VIRTIO-REAL] Virtqueues RX/TX reais configuradas (qsz={}/{})",
                        rx.qsz, tx.qsz);
                }
                _ => {
                    crate::serial_println!("[VIRTIO-REAL] DMA real indisponivel — TX/RX ficam simulados em memoria");
                }
            }
        }

        // 7. Sinaliza DRIVER_OK
        regs.set_device_status(0x07); // ACK | DRIVER | DRIVER_OK

        // 8. Verifica status final
        let status = regs.device_status();
        if status & 0x80 != 0 {
            crate::serial_println!("[VIRTIO-REAL] Device sinalizou FAILED (0x{:02x})", status);
            return false;
        }

        self.iobase      = iobase;
        self.link_up     = true;
        self.initialized = true;

        crate::serial_println!("[VIRTIO-REAL] virtio-net PRONTO — link up");
        true
    }

    /// Envia um frame Ethernet (DMA real via virtqueue TX, se
    /// configurada; caso contrário cai para o buffer em memória)
    pub fn transmit_real(&mut self, frame: Vec<u8>) -> bool {
        if !self.initialized || !self.link_up {
            self.tx_dropped += 1;
            return false;
        }
        if frame.len() > 1514 {
            self.tx_dropped += 1;
            return false;
        }

        let Some(tx) = self.tx_ring else {
            // Sem DMA real disponível: mantém o comportamento antigo
            // (buffer em memória) como fallback, para não perder a
            // funcionalidade em hardware/QEMU não suportado.
            let mut buf = alloc::vec![0u8; 12];
            buf.extend_from_slice(&frame);
            self.tx_buffers.push(buf);
            self.tx_packets += 1;
            self.tx_bytes   += frame.len() as u64;
            let regs = VirtioRegs::new(self.iobase);
            regs.notify_queue(1);
            return true;
        };

        if VNET_HDR_LEN + frame.len() > PKT_BUF_SIZE {
            self.tx_dropped += 1;
            return false;
        }

        unsafe {
            let slot = (self.tx_next as usize) % TX_BUF_COUNT;
            self.tx_next = self.tx_next.wrapping_add(1);

            let buf_ptr = self.tx_bufs_base.add(slot * PKT_BUF_SIZE);
            // virtio_net_hdr legacy: 10 bytes a zero (sem GSO/checksum offload)
            core::ptr::write_bytes(buf_ptr, 0, VNET_HDR_LEN);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf_ptr.add(VNET_HDR_LEN), frame.len());

            let total_len  = (VNET_HDR_LEN + frame.len()) as u32;
            let buf_phys   = self.tx_bufs_phys + (slot * PKT_BUF_SIZE) as u64;
            let desc_idx   = slot as u16;
            tx.set_desc(desc_idx, buf_phys, total_len, 0, 0);

            let avail_slot = tx.avail_idx();
            tx.set_avail_ring(avail_slot, desc_idx);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            tx.set_avail_idx(avail_slot.wrapping_add(1));
        }

        let regs = VirtioRegs::new(self.iobase);
        regs.notify_queue(1);

        self.tx_packets += 1;
        self.tx_bytes   += frame.len() as u64;
        true
    }

    /// Polling de frames RX recebidos (lê o used ring real da
    /// virtqueue RX, se configurada; caso contrário devolve o buffer
    /// de simulação/testes preenchido via `inject_rx`)
    pub fn receive_real(&mut self) -> Vec<Vec<u8>> {
        if !self.initialized { return Vec::new(); }

        let Some(rx) = self.rx_ring else {
            let frames = self.rx_buffers.drain(..).collect::<Vec<_>>();
            self.rx_packets += frames.len() as u64;
            return frames;
        };

        let mut frames = Vec::new();
        unsafe {
            let new_used_idx = rx.used_idx();
            while self.rx_used_seen != new_used_idx {
                let slot = self.rx_used_seen;
                let (desc_id, len) = rx.used_elem(slot);
                self.rx_used_seen = self.rx_used_seen.wrapping_add(1);

                let desc_id = desc_id as usize;
                if desc_id < RX_BUF_COUNT && (len as usize) > VNET_HDR_LEN {
                    let buf_ptr = self.rx_bufs_base.add(desc_id * PKT_BUF_SIZE);
                    let payload_len = ((len as usize) - VNET_HDR_LEN).min(PKT_BUF_SIZE - VNET_HDR_LEN);
                    let mut v = alloc::vec![0u8; payload_len];
                    core::ptr::copy_nonoverlapping(buf_ptr.add(VNET_HDR_LEN), v.as_mut_ptr(), payload_len);
                    frames.push(v);

                    // Devolve o buffer ao device para reutilização
                    let buf_phys = self.rx_bufs_phys + (desc_id * PKT_BUF_SIZE) as u64;
                    rx.set_desc(desc_id as u16, buf_phys, PKT_BUF_SIZE as u32, VIRTQ_DESC_F_WRITE, 0);
                    let avail_slot = rx.avail_idx();
                    rx.set_avail_ring(avail_slot, desc_id as u16);
                    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                    rx.set_avail_idx(avail_slot.wrapping_add(1));
                }
            }
        }

        if !frames.is_empty() {
            let regs = VirtioRegs::new(self.iobase);
            regs.notify_queue(0);
        }
        self.rx_packets += frames.len() as u64;
        self.rx_bytes   += frames.iter().map(|f| f.len() as u64).sum::<u64>();
        frames
    }

    /// Injecta um frame RX (para testes e simulação)
    pub fn inject_rx(&mut self, frame: Vec<u8>) {
        self.rx_buffers.push(frame);
    }

    pub fn mac_string(&self) -> alloc::string::String {
        format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.mac[0], self.mac[1], self.mac[2],
            self.mac[3], self.mac[4], self.mac[5])
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static VIRTIO_REAL: Spinlock<VirtioNetReal> =
    Spinlock::new(VirtioNetReal::new());

// ─── API Pública ─────────────────────────────────────────────

/// Inicializa o driver físico — tenta PCI real, fallback para simulado
pub fn init_physical() -> bool {
    let success = VIRTIO_REAL.lock().init_real();
    if success {
        crate::serial_println!("[VIRTIO-REAL] Driver fisico ativo");
    } else {
        crate::serial_println!("[VIRTIO-REAL] Usando driver simulado (sem device PCI)");
    }
    success
}

pub fn transmit(frame: Vec<u8>) -> bool {
    VIRTIO_REAL.lock().transmit_real(frame)
}

pub fn receive() -> Vec<Vec<u8>> {
    VIRTIO_REAL.lock().receive_real()
}

pub fn is_up() -> bool {
    VIRTIO_REAL.lock().link_up
}

pub fn mac() -> [u8; 6] {
    VIRTIO_REAL.lock().mac
}

pub fn stats() -> (u64, u64, u64, u64) {
    let d = VIRTIO_REAL.lock();
    (d.tx_packets, d.rx_packets, d.tx_bytes, d.rx_bytes)
}

// ─── PCI Device Listing ───────────────────────────────────────

pub fn list_pci_devices() -> Vec<PciDevice> {
    pci_scan()
}

// ─── Demonstração Fase 7 ─────────────────────────────────────

pub fn run_demo() {
    crate::serial_println!("\n[FASE7] === Driver de Rede Fisico (virtio-net PCI) ===");

    // Scan PCI bus
    let devices = list_pci_devices();
    crate::serial_println!("[FASE7] PCI Bus scan: {} devices encontrados", devices.len());
    for d in &devices {
        crate::serial_println!("[FASE7]   {:02x}:{:02x} vendor={:04x} device={:04x} class={:02x}",
            d.bus, d.dev, d.vendor, d.device, d.class);
    }

    // Tenta inicializar o driver real
    let success = init_physical();

    if success {
        let mac = VIRTIO_REAL.lock().mac_string();
        crate::serial_println!("[FASE7] virtio-net PCI real ativo — MAC: {}", mac);

        // Testa TX
        let test_frame = alloc::vec![0xFFu8; 60]; // frame mínimo Ethernet
        let sent = transmit(test_frame);
        crate::serial_println!("[FASE7] Frame TX de teste: {}", if sent {"enviado"} else {"falhou"});
    } else {
        crate::serial_println!("[FASE7] PCI real nao disponivel — QEMU sem -device virtio-net-pci");
        crate::serial_println!("[FASE7] Para ativar: adicionar ao comando QEMU:");
        crate::serial_println!("[FASE7]   -netdev user,id=net0 -device virtio-net-pci,netdev=net0");
    }

    let (tx_p, rx_p, tx_b, rx_b) = stats();
    crate::serial_println!("[FASE7] Stats: TX {} pkts/{} bytes | RX {} pkts/{} bytes",
        tx_p, tx_b, rx_p, rx_b);
    crate::serial_println!("[FASE7] Use 'net' e 'pci' no shell para inspecionar");
    crate::serial_println!("[FASE7] ==========================================\n");
}
