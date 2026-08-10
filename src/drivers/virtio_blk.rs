// ============================================================
// SOC-D Kernel — Driver virtio-blk Real (Fase 8 — TmpFS em disco)
// ============================================================
//
// Driver de bloco virtio-blk (legacy 0.9), síncrono e minimalista:
// só é usado para gravar/carregar o snapshot inteiro do TmpFS, não
// para uso geral de disco — por isso um único pedido de cada vez
// (sem fila de pedidos concorrentes) é suficiente e muito mais
// simples do que o driver de rede (net::virtio_real).
//
// Reaproveita o scan PCI de net::virtio_real::pci_scan(). A
// configuração da virtqueue (descriptor table / avail / used ring)
// segue exactamente a mesma técnica documentada em
// net::virtio_real (buffers `static` para garantir contiguidade
// física, ver `verify_contiguous`).
//
// Formato de um pedido virtio-blk (legacy), 3 descritores em cadeia:
//   desc0: header  { type:u32, reserved:u32, sector:u64 } — 16 bytes, driver→device
//   desc1: data    (múltiplo de 512 bytes)                — device→driver (IN) ou driver→device (OUT)
//   desc2: status  (1 byte, devolvido pelo device)         — device→driver
//
// Para usar no QEMU, é preciso um disco associado:
//   -drive file=socd-disk.img,format=raw,if=none,id=disk0
//   -device virtio-blk-pci,drive=disk0
// ============================================================

extern crate alloc;
use x86_64::instructions::port::Port;
use spinning_top::Spinlock;
use crate::net::virtio_real::{pci_scan, PciDevice};

pub const SECTOR_SIZE: usize = 512;
const MAX_CHUNK: usize = 4096; // 8 sectores por pedido (1 página)

const VIRTIO_VENDOR:   u16 = 0x1AF4;
const VIRTIO_BLK_DEV:  u16 = 0x1001;
const PCI_COMMAND_REG: u8  = 0x04;

const VIRTIO_BLK_T_IN:  u32 = 0; // leitura (device → driver)
const VIRTIO_BLK_T_OUT: u32 = 1; // escrita (driver → device)

const VIRTQ_DESC_F_NEXT:  u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const MAX_QSZ:     usize = 256;
const VRING_BYTES: usize = 3 * 4096;

fn align_up(x: usize, a: usize) -> usize { (x + a - 1) & !(a - 1) }

// ─── Registos I/O legacy virtio ───────────────────────────────

struct BlkRegs { iobase: u16 }

impl BlkRegs {
    fn new(iobase: u16) -> Self { Self { iobase } }
    unsafe fn read_u32(&self, off: u16) -> u32 { let mut p: Port<u32> = Port::new(self.iobase + off); p.read() }
    unsafe fn write_u32(&self, off: u16, v: u32) { let mut p: Port<u32> = Port::new(self.iobase + off); p.write(v); }
    unsafe fn read_u16(&self, off: u16) -> u16 { let mut p: Port<u16> = Port::new(self.iobase + off); p.read() }
    unsafe fn write_u16(&self, off: u16, v: u16) { let mut p: Port<u16> = Port::new(self.iobase + off); p.write(v); }
    unsafe fn read_u8(&self, off: u16) -> u8 { let mut p: Port<u8> = Port::new(self.iobase + off); p.read() }
    unsafe fn write_u8(&self, off: u16, v: u8) { let mut p: Port<u8> = Port::new(self.iobase + off); p.write(v); }

    fn device_features(&self) -> u32 { unsafe { self.read_u32(0x00) } }
    fn set_driver_features(&self, f: u32) { unsafe { self.write_u32(0x04, f); } }
    fn set_queue_addr(&self, addr: u32) { unsafe { self.write_u32(0x08, addr); } }
    fn queue_size(&self) -> u16 { unsafe { self.read_u16(0x0C) } }
    fn set_queue_select(&self, q: u16) { unsafe { self.write_u16(0x0E, q); } }
    fn notify_queue(&self, q: u16) { unsafe { self.write_u16(0x10, q); } }
    fn device_status(&self) -> u8 { unsafe { self.read_u8(0x12) } }
    fn set_device_status(&self, s: u8) { unsafe { self.write_u8(0x12, s); } }
    /// capacity (nº de sectores de 512 bytes) — config space, offset 0x14, 64 bits
    fn capacity_sectors(&self) -> u64 {
        unsafe {
            let lo = self.read_u32(0x14) as u64;
            let hi = self.read_u32(0x18) as u64;
            lo | (hi << 32)
        }
    }
}

// ─── Virtqueue (idêntica em técnica a net::virtio_real) ───────

#[repr(C, align(4096))]
struct AlignedVring([u8; VRING_BYTES]);
static mut BLK_VRING: AlignedVring = AlignedVring([0; VRING_BYTES]);

#[repr(C, align(16))]
struct HdrBuf([u8; 16]);
static mut HDR_BUF: HdrBuf = HdrBuf([0; 16]);

#[repr(C, align(4096))]
struct DataBuf([u8; MAX_CHUNK]);
static mut DATA_BUF: DataBuf = DataBuf([0; MAX_CHUNK]);

#[repr(C, align(8))]
struct StatusBuf([u8; 8]); // só o primeiro byte é usado; resto é folga
static mut STATUS_BUF: StatusBuf = StatusBuf([0; 8]);

unsafe fn verify_contiguous(virt_start: u64, len: usize) -> Option<u64> {
    let start_phys = crate::memory::virt_to_phys(x86_64::VirtAddr::new(virt_start))?.as_u64();
    let mut off = 0usize;
    while off < len {
        let v = virt_start + off as u64;
        let p = crate::memory::virt_to_phys(x86_64::VirtAddr::new(v))?.as_u64();
        if p != start_phys + off as u64 { return None; }
        off += 4096;
    }
    Some(start_phys)
}

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
}

unsafe fn setup_vring(regs: &BlkRegs, queue_idx: u16, vring_buf: *mut u8) -> Option<VringLayout> {
    regs.set_queue_select(queue_idx);
    let qsz = regs.queue_size();
    if qsz == 0 || qsz as usize > MAX_QSZ { return None; }

    let desc_bytes  = 16 * qsz as usize;
    let avail_bytes = 6 + 2 * qsz as usize;
    let used_off    = align_up(desc_bytes + avail_bytes, 4096);
    let used_bytes  = 6 + 8 * qsz as usize;
    let total       = used_off + used_bytes;
    if total > VRING_BYTES { return None; }

    core::ptr::write_bytes(vring_buf, 0, total);

    let phys = verify_contiguous(vring_buf as u64, total)?;
    if phys & 0xFFF != 0 { return None; }

    regs.set_queue_addr((phys >> 12) as u32);
    Some(VringLayout { base: vring_buf, qsz, avail_off: desc_bytes, used_off })
}

// ─── Driver ─────────────────────────────────────────────────────

struct VirtioBlkReal {
    initialized: bool,
    iobase:      u16,
    capacity:    u64, // sectores de 512 bytes
    ring:        Option<VringLayout>,
    hdr_phys:    u64,
    data_phys:   u64,
    status_phys: u64,
}
unsafe impl Send for VirtioBlkReal {}

impl VirtioBlkReal {
    const fn new() -> Self {
        Self {
            initialized: false, iobase: 0, capacity: 0,
            ring: None, hdr_phys: 0, data_phys: 0, status_phys: 0,
        }
    }

    fn init_real(&mut self) -> bool {
        let dev = match pci_scan().into_iter().find(|d: &PciDevice| d.vendor == VIRTIO_VENDOR && d.device == VIRTIO_BLK_DEV) {
            Some(d) => d,
            None => {
                crate::serial_println!("[VIRTIO-BLK] Device nao encontrado no PCI bus");
                return false;
            }
        };
        let iobase = match dev.io_base() {
            Some(b) => b,
            None => { crate::serial_println!("[VIRTIO-BLK] BAR0 nao e I/O BAR"); return false; }
        };
        crate::serial_println!("[VIRTIO-BLK] Found virtio-blk @ PCI {:02x}:{:02x} iobase=0x{:04x}",
            dev.bus, dev.dev, iobase);

        unsafe {
            let cmd = crate::net::virtio_real::pci_read16(dev.bus, dev.dev, 0, PCI_COMMAND_REG);
            crate::net::virtio_real::pci_write16(dev.bus, dev.dev, 0, PCI_COMMAND_REG, cmd | 0x05);
        }

        let regs = BlkRegs::new(iobase);
        regs.set_device_status(0x00);
        regs.set_device_status(0x01);
        regs.set_device_status(0x03);

        let features = regs.device_features();
        regs.set_driver_features(0); // sem features opcionais — mantém tudo simples

        let capacity = regs.capacity_sectors();
        crate::serial_println!("[VIRTIO-BLK] Features: 0x{:08x} | Capacidade: {} sectores ({} MB)",
            features, capacity, capacity * SECTOR_SIZE as u64 / 1024 / 1024);

        let ring = unsafe {
            let vring_ptr = core::ptr::addr_of_mut!(BLK_VRING.0) as *mut u8;
            setup_vring(&regs, 0, vring_ptr)
        };
        let (hdr_phys, data_phys, status_phys) = unsafe {
            let h = verify_contiguous(core::ptr::addr_of_mut!(HDR_BUF.0) as u64, 16);
            let d = verify_contiguous(core::ptr::addr_of_mut!(DATA_BUF.0) as u64, MAX_CHUNK);
            let s = verify_contiguous(core::ptr::addr_of_mut!(STATUS_BUF.0) as u64, 8);
            (h, d, s)
        };

        let (ring, hdr_phys, data_phys, status_phys) = match (ring, hdr_phys, data_phys, status_phys) {
            (Some(r), Some(h), Some(d), Some(s)) => (r, h, d, s),
            _ => {
                crate::serial_println!("[VIRTIO-BLK] DMA real indisponivel — TmpFS em disco desativado");
                return false;
            }
        };

        regs.set_device_status(0x07);
        if regs.device_status() & 0x80 != 0 {
            crate::serial_println!("[VIRTIO-BLK] Device sinalizou FAILED");
            return false;
        }

        self.iobase = iobase;
        self.capacity = capacity;
        self.ring = Some(ring);
        self.hdr_phys = hdr_phys;
        self.data_phys = data_phys;
        self.status_phys = status_phys;
        self.initialized = true;

        crate::serial_println!("[VIRTIO-BLK] virtio-blk PRONTO");
        true
    }

    /// Um pedido síncrono de leitura/escrita, bloqueante (poll até o
    /// device terminar). `len` tem de ser múltiplo de 512 e <= MAX_CHUNK.
    fn do_request(&self, write: bool, sector: u64, buf_in_out: &mut [u8]) -> bool {
        if !self.initialized { return false; }
        let Some(ring) = self.ring else { return false; };
        let len = buf_in_out.len();
        if len == 0 || len % SECTOR_SIZE != 0 || len > MAX_CHUNK { return false; }

        unsafe {
            let hdr_ptr = core::ptr::addr_of_mut!(HDR_BUF.0) as *mut u8;
            let data_ptr = core::ptr::addr_of_mut!(DATA_BUF.0) as *mut u8;
            let status_ptr = core::ptr::addr_of_mut!(STATUS_BUF.0) as *mut u8;

            // Cabeçalho: type(4) + reserved(4) + sector(8)
            let req_type: u32 = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
            core::ptr::write_volatile(hdr_ptr as *mut u32, req_type);
            core::ptr::write_volatile(hdr_ptr.add(4) as *mut u32, 0);
            core::ptr::write_volatile(hdr_ptr.add(8) as *mut u64, sector);

            if write {
                core::ptr::copy_nonoverlapping(buf_in_out.as_ptr(), data_ptr, len);
            }
            core::ptr::write_volatile(status_ptr, 0xFF); // valor sentinela (device escreve 0 se OK)

            let data_flags = if write { VIRTQ_DESC_F_NEXT } else { VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE };
            ring.set_desc(0, self.hdr_phys, 16, VIRTQ_DESC_F_NEXT, 1);
            ring.set_desc(1, self.data_phys, len as u32, data_flags, 2);
            ring.set_desc(2, self.status_phys, 1, VIRTQ_DESC_F_WRITE, 0);

            let avail_slot = ring.avail_idx();
            ring.set_avail_ring(avail_slot, 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            ring.set_avail_idx(avail_slot.wrapping_add(1));

            let regs = BlkRegs::new(self.iobase);
            regs.notify_queue(0);

            // Poll síncrono — pedidos de bloco no QEMU completam em
            // microssegundos; um limite generoso de iterações evita
            // travar para sempre se não houver disco associado.
            let target_used = avail_slot.wrapping_add(1);
            let mut spins = 0u64;
            while ring.used_idx() != target_used {
                core::hint::spin_loop();
                spins += 1;
                if spins > 200_000_000 {
                    crate::serial_println!("[VIRTIO-BLK] Timeout a espera do device (sem -drive/virtio-blk-pci?)");
                    return false;
                }
            }

            let status = core::ptr::read_volatile(status_ptr);
            if status != 0 {
                crate::serial_println!("[VIRTIO-BLK] Pedido falhou, status={}", status);
                return false;
            }

            if !write {
                core::ptr::copy_nonoverlapping(data_ptr, buf_in_out.as_mut_ptr(), len);
            }
        }
        true
    }
}

static VIRTIO_BLK: Spinlock<VirtioBlkReal> = Spinlock::new(VirtioBlkReal::new());

pub fn init_physical() -> bool {
    let ok = VIRTIO_BLK.lock().init_real();
    if !ok {
        crate::serial_println!("[VIRTIO-BLK] Disco nao disponivel — TmpFS fica so em RAM (sem persistencia)");
    }
    ok
}

pub fn is_up() -> bool { VIRTIO_BLK.lock().initialized }
pub fn capacity_sectors() -> u64 { VIRTIO_BLK.lock().capacity }

/// Lê `buf.len()` bytes a partir de `byte_offset` (tem de ser múltiplo
/// de 512). Faz várias operações de disco internamente se necessário.
pub fn read_at(byte_offset: u64, buf: &mut [u8]) -> bool {
    if byte_offset % SECTOR_SIZE as u64 != 0 { return false; }
    let drv = VIRTIO_BLK.lock();
    let mut off = 0usize;
    while off < buf.len() {
        let chunk = (buf.len() - off).min(MAX_CHUNK);
        let padded = align_up(chunk, SECTOR_SIZE);
        let sector = (byte_offset + off as u64) / SECTOR_SIZE as u64;
        let mut tmp = [0u8; MAX_CHUNK];
        if !drv.do_request(false, sector, &mut tmp[..padded]) { return false; }
        buf[off..off + chunk].copy_from_slice(&tmp[..chunk]);
        off += chunk;
    }
    true
}

/// Escreve `data` a partir de `byte_offset` (múltiplo de 512).
pub fn write_at(byte_offset: u64, data: &[u8]) -> bool {
    if byte_offset % SECTOR_SIZE as u64 != 0 { return false; }
    let drv = VIRTIO_BLK.lock();
    let mut off = 0usize;
    while off < data.len() {
        let chunk = (data.len() - off).min(MAX_CHUNK);
        let padded = align_up(chunk, SECTOR_SIZE);
        let sector = (byte_offset + off as u64) / SECTOR_SIZE as u64;
        let mut tmp = [0u8; MAX_CHUNK];
        tmp[..chunk].copy_from_slice(&data[off..off + chunk]);
        if !drv.do_request(true, sector, &mut tmp[..padded]) { return false; }
        off += chunk;
    }
    true
}
