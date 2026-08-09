// ============================================================
// SOC-D Kernel — Criptografia Real (SHA-256 + Ed25519 + HMAC)
// ============================================================
extern crate alloc;
use alloc::{string::String, format, vec::Vec};
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub type Hash256 = [u8; 32];
pub type Sig64   = [u8; 64];
pub type PubKey32  = [u8; 32];
pub type PrivKey32 = [u8; 32];

// ─── SHA-256 ─────────────────────────────────────────────────

pub fn sha256(data: &[u8]) -> Hash256 {
    let mut h = Sha256::new();
    h.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

pub fn sha256_multi(fields: &[&[u8]]) -> Hash256 {
    let mut h = Sha256::new();
    for f in fields { h.update(f); }
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

pub fn hash_to_hex(h: &Hash256) -> String {
    let mut s = String::new();
    for &b in h {
        let hi = b >> 4; let lo = b & 0xf;
        s.push(if hi < 10 { (b'0'+hi) as char } else { (b'a'+hi-10) as char });
        s.push(if lo < 10 { (b'0'+lo) as char } else { (b'a'+lo-10) as char });
    }
    s
}

pub fn hash_short(h: &Hash256) -> String {
    format!("{}..", &hash_to_hex(h)[..16])
}

// ─── HMAC-SHA256 ─────────────────────────────────────────────

type HmacSha256 = Hmac<Sha256>;

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Hash256 {
    let mut mac = HmacSha256::new_from_slice(key)
        .expect("HMAC accepts any key size");
    mac.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

pub fn hmac_verify(key: &[u8], data: &[u8], expected: &Hash256) -> bool {
    let computed = hmac_sha256(key, data);
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(expected.iter()) { diff |= a ^ b; }
    diff == 0
}

// ─── Entropia ────────────────────────────────────────────────

/// Verifica, via CPUID, se o CPU suporta a instrução RDRAND
/// (CPUID.01H:ECX.RDRAND[bit 30]). Tem de ser chamado ANTES de
/// executar `rdrand` — em CPUs sem suporte (ex: o modelo QEMU por
/// omissão, sem `-cpu host`/`-cpu` com +rdrand) a instrução gera uma
/// excepção de opcode inválido (#UD), que sem um handler dedicado
/// resulta em double fault e paragem do kernel.
fn cpu_has_rdrand() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let result = core::arch::x86_64::__cpuid(1);
        (result.ecx & (1 << 30)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn rdrand_bytes(buf: &mut [u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        if !cpu_has_rdrand() { return false; }
        for chunk in buf.chunks_mut(8) {
            let mut val: u64 = 0;
            let mut ok: u8 = 0;
            unsafe {
                for _ in 0..10 {
                    core::arch::asm!(
                        "rdrand {0:r}", "setc {1}",
                        out(reg) val, out(reg_byte) ok,
                        options(nomem, nostack)
                    );
                    if ok != 0 { break; }
                }
            }
            if ok == 0 { return false; }
            let bytes = val.to_le_bytes();
            for (i, b) in chunk.iter_mut().enumerate() { *b = bytes[i]; }
        }
        true
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn fallback_entropy(buf: &mut [u8]) {
    let tsc: u64 = unsafe {
        #[cfg(target_arch = "x86_64")]
        { core::arch::x86_64::_rdtsc() }
        #[cfg(not(target_arch = "x86_64"))]
        { 0u64 }
    };
    let tick = crate::modules::scheduler::get_stats().current_tick;
    let mut seed = sha256_multi(&[
        &tsc.to_le_bytes(), &tick.to_le_bytes(), b"socd-entropy-v1"
    ]);
    let mut pos = 0usize;
    let mut ctr = 0u64;
    while pos < buf.len() {
        let block = sha256_multi(&[&seed, &ctr.to_le_bytes()]);
        let n = (buf.len() - pos).min(32);
        buf[pos..pos+n].copy_from_slice(&block[..n]);
        pos += n; ctr += 1;
        seed = sha256_multi(&[&seed, &block]);
    }
}

pub fn random_bytes(buf: &mut [u8]) {
    if !rdrand_bytes(buf) { fallback_entropy(buf); }
}

pub fn random_32() -> [u8; 32] {
    let mut buf = [0u8; 32];
    random_bytes(&mut buf);
    buf
}

// ─── Ed25519 Real ─────────────────────────────────────────────

pub struct KeyPair {
    pub signing_key:   [u8; 32],
    pub verifying_key: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Self { Self::from_seed(random_32()) }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();
        Self { signing_key: seed, verifying_key: vk.to_bytes() }
    }

    pub fn sign(&self, data: &[u8]) -> Sig64 {
        SigningKey::from_bytes(&self.signing_key).sign(data).to_bytes()
    }

    pub fn verify(&self, data: &[u8], sig: &Sig64) -> bool {
        verify_signature(&self.verifying_key, data, sig)
    }
}

pub fn verify_signature(pubkey: &PubKey32, data: &[u8], sig_bytes: &Sig64) -> bool {
    let vk = match VerifyingKey::from_bytes(pubkey) {
        Ok(k) => k, Err(_) => return false,
    };
    let sig = Signature::from_bytes(sig_bytes);
    vk.verify(data, &sig).is_ok()
}

pub fn sign(privkey: &PrivKey32, data: &[u8]) -> Sig64 {
    SigningKey::from_bytes(privkey).sign(data).to_bytes()
}

// ─── DAG helpers ─────────────────────────────────────────────

pub fn dag_block_hash(
    parents: &[[u8; 32]], author: &[u8; 32],
    timestamp: u64, kind_byte: u8,
    path: &str, payload: &[u8], seq: u64,
) -> Hash256 {
    let mut h = Sha256::new();
    for p in parents { h.update(p); }
    h.update(author);
    h.update(&timestamp.to_le_bytes());
    h.update(&[kind_byte]);
    h.update(path.as_bytes());
    h.update(payload);
    h.update(&seq.to_le_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

pub fn dag_block_sign(privkey: &PrivKey32, hash: &Hash256) -> Sig64 {
    sign(privkey, hash)
}

pub fn dag_block_verify(pubkey: &PubKey32, hash: &Hash256, sig: &Sig64) -> bool {
    verify_signature(pubkey, hash, sig)
}

pub fn pubkey_to_node_id(pubkey: &PubKey32) -> [u8; 32] {
    sha256_multi(&[pubkey, b"socd-node-id-v1"])
}

pub fn session_key(a: &[u8; 32], b: &[u8; 32]) -> Hash256 {
    if a < b { sha256_multi(&[a, b, b"socd-session-v1"]) }
    else      { sha256_multi(&[b, a, b"socd-session-v1"]) }
}

// ─── Self-test e Init ─────────────────────────────────────────

pub fn self_test() -> bool {
    // SHA-256("abc") conhecido (vetor de teste oficial FIPS 180-4).
    // NOTA (bug corrigido): a constante aqui estava corrompida a
    // partir do byte 13 — os primeiros 12 bytes coincidiam com o
    // vetor real, o resto não. O self-test falhava sempre, mesmo com
    // o `sha2` a calcular o hash correctamente.
    let expected: Hash256 = [
        0xba,0x78,0x16,0xbf,0x8f,0x01,0xcf,0xea,
        0x41,0x41,0x40,0xde,0x5d,0xae,0x22,0x23,
        0xb0,0x03,0x61,0xa3,0x96,0x17,0x7a,0x9c,
        0xb4,0x10,0xff,0x61,0xf2,0x00,0x15,0xad,
    ];
    if sha256(b"abc") != expected { return false; }
    // Ed25519 round-trip
    let kp = KeyPair::generate();
    let sig = kp.sign(b"socd-test");
    if !kp.verify(b"socd-test", &sig) { return false; }
    // HMAC round-trip
    let mac = hmac_sha256(b"key", b"msg");
    if !hmac_verify(b"key", b"msg", &mac) { return false; }
    true
}

pub fn init() {
    crate::serial_println!("[CRYPTO] SHA-256 + HMAC-SHA256 + Ed25519 (crates auditadas)");
    if self_test() {
        crate::serial_println!("[CRYPTO] Self-test: PASSOU");
    } else {
        crate::serial_println!("[CRYPTO] Self-test: FALHOU");
    }
    let mut test = [0u8; 8];
    if rdrand_bytes(&mut test) {
        crate::serial_println!("[CRYPTO] Entropia: RDRAND (hardware RNG)");
    } else {
        crate::serial_println!("[CRYPTO] Entropia: TSC fallback");
    }
}
