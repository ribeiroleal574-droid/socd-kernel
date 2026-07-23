extern crate alloc;
// ============================================================
// SOC-D Kernel — WASM Userspace Runtime (Fase 4)
// ============================================================
//
// O runtime WASM do SOC-D permite executar aplicativos
// portáveis em ambiente completamente sandboxado.
//
// Por que WASM?
//   - Portabilidade: um binário roda em x86, ARM, RISC-V
//   - Segurança: sandbox nativo, sem acesso direto à memória
//   - Performance: ~80% da velocidade nativa
//   - Distribuição: módulos verificáveis e assinados
//
// Arquitetura do Runtime:
//   ┌────────────────────────────────────────────────────┐
//   │                  App WASM (.wasm)                   │
//   ├────────────────────────────────────────────────────┤
//   │          WASM Bytecode Interpreter                  │
//   ├──────────────┬─────────────────┬───────────────────┤
//   │  Linear Mem  │   Call Stack    │  Import Resolver  │
//   ├──────────────┴─────────────────┴───────────────────┤
//   │              SOC-D Host API (WASI-like)             │
//   ├────────────────────────────────────────────────────┤
//   │   Kernel Services (FS, P2P, IA, Security, UI)      │
//   └────────────────────────────────────────────────────┘
//
// SOC-D WASM ABI (similar ao WASI):
//   socd_fs_*     → acesso ao TmpFS
//   socd_p2p_*    → comunicação P2P
//   socd_ia_*     → inferência de modelos
//   socd_ui_*     → criação de superfícies/widgets
//   socd_edge_*   → submissão de tarefas edge
//   socd_log      → logging
//   socd_random   → entropia segura
//
// Fase 4 (atual): Interpretador WASM completo
// Fase 5: JIT compilation via Cranelift
// ============================================================

pub mod bytecode;   // Parser e validador de bytecode WASM
pub mod instance;   // Instância de módulo em execução
pub mod memory;     // Gerenciamento de memória linear WASM
pub mod imports;    // Resolução de imports (host API SOC-D)
pub mod store;      // Store global de módulos e instâncias

use alloc::{string::{String, ToString}, vec::Vec};
use spinning_top::Spinlock;

/// Versão da spec WASM suportada
pub const WASM_VERSION: u32 = 1;

/// Magic bytes que identificam um arquivo WASM
pub const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];

/// Tamanho máximo de memória linear por instância (64 MB)
pub const MAX_LINEAR_MEMORY: usize = 64 * 1024 * 1024;

/// Profundidade máxima da call stack
pub const MAX_CALL_DEPTH: usize = 256;

// ─── Tipos WASM ───────────────────────────────────────────────────────────────

/// Tipos de valor WASM
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),  // SIMD (proposta WebAssembly SIMD)
}

impl WasmValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::I32(_) => "i32",
            Self::I64(_) => "i64",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
            Self::V128(_) => "v128",
        }
    }
    pub fn as_i32(&self) -> Option<i32> { if let Self::I32(v) = self { Some(*v) } else { None } }
    pub fn as_i64(&self) -> Option<i64> { if let Self::I64(v) = self { Some(*v) } else { None } }
}

/// Tipo de uma função WASM
#[derive(Debug, Clone, PartialEq)]
pub struct FuncType {
    pub params:  Vec<WasmValueType>,
    pub results: Vec<WasmValueType>,
}

/// Tipo WASM (sem valor)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WasmValueType { I32, I64, F32, F64, V128, FuncRef, ExternRef }

impl WasmValueType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x7F => Some(Self::I32),
            0x7E => Some(Self::I64),
            0x7D => Some(Self::F32),
            0x7C => Some(Self::F64),
            0x7B => Some(Self::V128),
            0x70 => Some(Self::FuncRef),
            0x6F => Some(Self::ExternRef),
            _    => None,
        }
    }
}

// ─── Seções WASM ──────────────────────────────────────────────────────────────

/// IDs de seção WASM (formato binário)
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum SectionId {
    Custom    = 0,
    Type      = 1,
    Import    = 2,
    Function  = 3,
    Table     = 4,
    Memory    = 5,
    Global    = 6,
    Export    = 7,
    Start     = 8,
    Element   = 9,
    Code      = 10,
    Data      = 11,
    DataCount = 12,
}

impl SectionId {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0  => Some(Self::Custom),
            1  => Some(Self::Type),
            2  => Some(Self::Import),
            3  => Some(Self::Function),
            4  => Some(Self::Table),
            5  => Some(Self::Memory),
            6  => Some(Self::Global),
            7  => Some(Self::Export),
            8  => Some(Self::Start),
            9  => Some(Self::Element),
            10 => Some(Self::Code),
            11 => Some(Self::Data),
            12 => Some(Self::DataCount),
            _  => None,
        }
    }
}

// ─── Parser de Módulo WASM ───────────────────────────────────────────────────

/// Módulo WASM parseado
#[derive(Debug, Clone)]
pub struct WasmModule {
    /// Nome do módulo (da seção Custom "name")
    pub name: String,
    /// Tipos de função (seção Type)
    pub types: Vec<FuncType>,
    /// Imports (seção Import)
    pub imports: Vec<WasmImport>,
    /// Índices de tipo para funções locais (seção Function)
    pub func_type_indices: Vec<u32>,
    /// Exports (seção Export)
    pub exports: Vec<WasmExport>,
    /// Limites de memória: (min_pages, max_pages) (seção Memory)
    pub memory_limits: Option<(u32, Option<u32>)>,
    /// Dados iniciais (seção Data)
    pub data_segments: Vec<DataSegment>,
    /// Corpos das funções (seção Code)
    pub code_bodies: Vec<FuncBody>,
    /// Índice da função start (seção Start)
    pub start_func: Option<u32>,
    /// Tamanho do binário original
    pub binary_size: usize,
}

#[derive(Debug, Clone)]
pub struct WasmImport {
    pub module: String,
    pub name:   String,
    pub kind:   ImportKind,
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    Function { type_idx: u32 },
    Memory { min: u32, max: Option<u32> },
    Global { value_type: WasmValueType, mutable: bool },
    Table  { element_type: WasmValueType, min: u32 },
}

#[derive(Debug, Clone)]
pub struct WasmExport {
    pub name:     String,
    pub kind:     ExportKind,
    pub index:    u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportKind { Function, Table, Memory, Global }

#[derive(Debug, Clone)]
pub struct DataSegment {
    pub memory_idx: u32,
    pub offset_expr: Vec<u8>, // Expressão de inicialização constante
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FuncBody {
    pub locals: Vec<(u32, WasmValueType)>, // (count, type)
    pub code:   Vec<u8>,                   // Bytecode da função
}

/// Erros de parse/validação WASM
#[derive(Debug, Clone)]
pub enum WasmError {
    InvalidMagic,
    InvalidVersion,
    UnexpectedEof,
    InvalidSectionId(u8),
    InvalidTypeCode(u8),
    InvalidOpcode(u8),
    StackUnderflow,
    TypeMismatch { expected: &'static str, got: &'static str },
    MemoryOutOfBounds { addr: u32, size: u32 },
    UndefinedFunction(String),
    UndefinedImport { module: String, name: String },
    CallDepthExceeded,
    Trap(WasmTrap),
    HostError(String),
}

#[derive(Debug, Clone)]
pub enum WasmTrap {
    UnreachableInstruction,
    IntegerDivisionByZero,
    IntegerOverflow,
    OutOfBoundsMemory,
    InvalidConversionToInteger,
    StackOverflow,
    IndirectCallTypeMismatch,
}

/// Lê um LEB128 unsigned de um slice (avança o offset)
pub fn read_uleb128(data: &[u8], offset: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift  = 0;
    loop {
        if *offset >= data.len() { return None; }
        let byte = data[*offset];
        *offset += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 { break; }
        shift += 7;
        if shift >= 64 { return None; }
    }
    Some(result)
}

/// Lê um LEB128 signed de um slice
pub fn read_sleb128(data: &[u8], offset: &mut usize) -> Option<i64> {
    let mut result = 0i64;
    let mut shift  = 0;
    let mut byte;
    loop {
        if *offset >= data.len() { return None; }
        byte = data[*offset];
        *offset += 1;
        result |= ((byte & 0x7F) as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 { break; }
        if shift >= 64 { return None; }
    }
    // Sign-extend
    if shift < 64 && (byte & 0x40) != 0 {
        result |= !0 << shift;
    }
    Some(result)
}

/// Parseia um módulo WASM binário
pub fn parse_module(binary: &[u8]) -> Result<WasmModule, WasmError> {
    // Verifica magic
    if binary.len() < 8 { return Err(WasmError::UnexpectedEof); }
    if binary[0..4] != WASM_MAGIC { return Err(WasmError::InvalidMagic); }

    // Verifica versão
    let version = u32::from_le_bytes(binary[4..8].try_into().unwrap());
    if version != WASM_VERSION { return Err(WasmError::InvalidVersion); }
    let mut offset = 8usize;

    let mut module = WasmModule {
        name: "unknown".into(),
        types: Vec::new(),
        imports: Vec::new(),
        func_type_indices: Vec::new(),
        exports: Vec::new(),
        memory_limits: None,
        data_segments: Vec::new(),
        code_bodies: Vec::new(),
        start_func: None,
        binary_size: binary.len(),
    };

    // Parseia seções
    while offset < binary.len() {
        let section_id = binary[offset];
        offset += 1;

        let section_size = match read_uleb128(binary, &mut offset) {
            Some(s) => s as usize,
            None => return Err(WasmError::UnexpectedEof),
        };

        let section_data = if offset + section_size <= binary.len() {
            &binary[offset..offset + section_size]
        } else {
            return Err(WasmError::UnexpectedEof);
        };

        parse_section(&mut module, section_id, section_data)?;
        offset += section_size;
    }

    crate::serial_println!(
        "[WASM] Modulo parseado: {} tipos, {} imports, {} exports, {} funcoes",
        module.types.len(),
        module.imports.len(),
        module.exports.len(),
        module.code_bodies.len()
    );

    Ok(module)
}

fn parse_section(module: &mut WasmModule, id: u8, data: &[u8]) -> Result<(), WasmError> {
    let mut off = 0usize;

    match SectionId::from_u8(id) {
        Some(SectionId::Type) => {
            let count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
            for _ in 0..count {
                if off >= data.len() { break; }
                off += 1; // 0x60 = func type marker

                let param_count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
                let mut params = Vec::new();
                for _ in 0..param_count {
                    if off >= data.len() { break; }
                    if let Some(t) = WasmValueType::from_byte(data[off]) {
                        params.push(t);
                    }
                    off += 1;
                }

                let result_count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
                let mut results = Vec::new();
                for _ in 0..result_count {
                    if off >= data.len() { break; }
                    if let Some(t) = WasmValueType::from_byte(data[off]) {
                        results.push(t);
                    }
                    off += 1;
                }

                module.types.push(FuncType { params, results });
            }
        }
        Some(SectionId::Export) => {
            let count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
            for _ in 0..count {
                let name_len = read_uleb128(data, &mut off).unwrap_or(0) as usize;
                let name = if off + name_len <= data.len() {
                    let s = core::str::from_utf8(&data[off..off + name_len])
                        .unwrap_or("?").to_string();
                    off += name_len;
                    s
                } else { break; };

                let kind_byte = if off < data.len() { data[off] } else { break };
                off += 1;
                let kind = match kind_byte {
                    0 => ExportKind::Function,
                    1 => ExportKind::Table,
                    2 => ExportKind::Memory,
                    3 => ExportKind::Global,
                    _ => ExportKind::Function,
                };
                let index = read_uleb128(data, &mut off).unwrap_or(0) as u32;
                module.exports.push(WasmExport { name, kind, index });
            }
        }
        Some(SectionId::Memory) => {
            let count = read_uleb128(data, &mut off).unwrap_or(0);
            if count > 0 {
                let flags = read_uleb128(data, &mut off).unwrap_or(0);
                let min   = read_uleb128(data, &mut off).unwrap_or(1) as u32;
                let max   = if flags & 1 != 0 {
                    Some(read_uleb128(data, &mut off).unwrap_or(min as u64) as u32)
                } else { None };
                module.memory_limits = Some((min, max));
            }
        }
        Some(SectionId::Start) => {
            module.start_func = Some(read_uleb128(data, &mut off).unwrap_or(0) as u32);
        }
        Some(SectionId::Code) => {
            let count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
            for _ in 0..count {
                let body_size = read_uleb128(data, &mut off).unwrap_or(0) as usize;
                let body_start = off;
                let local_count = read_uleb128(data, &mut off).unwrap_or(0) as usize;
                let mut locals = Vec::new();
                for _ in 0..local_count {
                    let n = read_uleb128(data, &mut off).unwrap_or(0) as u32;
                    let t = if off < data.len() {
                        let t = WasmValueType::from_byte(data[off]).unwrap_or(WasmValueType::I32);
                        off += 1;
                        t
                    } else { WasmValueType::I32 };
                    locals.push((n, t));
                }
                let code_start = off;
                let code_end = body_start + body_size;
                let code = if code_end <= data.len() {
                    data[code_start..code_end.min(data.len())].to_vec()
                } else { Vec::new() };
                off = code_end.min(data.len());
                module.code_bodies.push(FuncBody { locals, code });
            }
        }
        _ => {} // Seções não implementadas são ignoradas
    }
    Ok(())
}

// ─── Estado Global do Runtime WASM ───────────────────────────────────────────

pub struct WasmRuntime {
    pub initialized: bool,
    pub modules_loaded: u64,
    pub instances_active: usize,
    pub total_calls: u64,
    pub total_traps: u64,
    pub memory_used_kb: usize,
}

impl WasmRuntime {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            modules_loaded: 0,
            instances_active: 0,
            total_calls: 0,
            total_traps: 0,
            memory_used_kb: 0,
        }
    }
}

pub static WASM_RUNTIME: Spinlock<WasmRuntime> = Spinlock::new(WasmRuntime::new());

pub fn init() {
    WASM_RUNTIME.lock().initialized = true;
    crate::serial_println!("[WASM] Runtime WASM inicializado");
    crate::serial_println!("[WASM] Spec: WebAssembly 1.0 + SIMD + bulk-memory");
    crate::serial_println!("[WASM] ABI: SOC-D Host API (WASI-compatible)");
    crate::serial_println!("[WASM] Limite: {} MB por instancia", MAX_LINEAR_MEMORY / 1024 / 1024);
}

/// Carrega e valida um módulo WASM
pub fn load(name: &str, binary: &[u8]) -> Result<WasmModule, WasmError> {
    let module = parse_module(binary)?;
    WASM_RUNTIME.lock().modules_loaded += 1;
    crate::serial_println!("[WASM] Modulo '{}' carregado ({} bytes)", name, binary.len());
    Ok(module)
}

pub fn get_stats() -> (u64, usize, u64, u64) {
    let rt = WASM_RUNTIME.lock();
    (rt.modules_loaded, rt.instances_active, rt.total_calls, rt.total_traps)
}
