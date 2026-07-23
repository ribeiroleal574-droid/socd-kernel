extern crate alloc;
// ============================================================
// SOC-D Kernel — ELF Loader
// Carrega módulos externos em formato ELF em runtime
// ============================================================
//
// O ELF (Executable and Linkable Format) é o formato padrão
// de binários em sistemas Unix/Linux. Cada módulo SOC-D é
// compilado como um arquivo ELF relocável (.o ou .so).
//
// Fluxo de carregamento:
//   1. Ler cabeçalho ELF e validar magic number
//   2. Iterar sobre seções (.text, .data, .rodata, .bss)
//   3. Alocar memória virtual para cada seção
//   4. Copiar conteúdo das seções para a memória alocada
//   5. Processar relocações (ajustar endereços absolutos)
//   6. Resolver símbolos externos (funções do kernel)
//   7. Chamar o ponto de entrada do módulo (init())
//
// Fase 1: Carregamento de módulos ELF relocáveis estáticos
// Fase 2: Suporte a shared objects (.so) e símbolos dinâmicos
// Fase 3: Verificação de assinatura criptográfica do módulo
// ============================================================

use alloc::{
    collections::BTreeMap,
    vec,
    string::{String, ToString},
    vec::Vec,
};
use core::mem;

// ─── Estruturas ELF64 ────────────────────────────────────────────────────────

/// Magic number que identifica um arquivo ELF
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Classe ELF: 64-bit
const ELFCLASS64: u8 = 2;

/// Endianness: little-endian (x86_64)
const ELFDATA2LSB: u8 = 1;

/// Tipo de objeto: relocável (módulo)
const ET_REL: u16 = 1;

/// Arquitetura: x86_64
const EM_X86_64: u16 = 62;

/// Tipos de seção ELF
const SHT_NULL:     u32 = 0;  // Seção nula
const SHT_PROGBITS: u32 = 1;  // Código ou dados
const SHT_SYMTAB:   u32 = 2;  // Tabela de símbolos
const SHT_STRTAB:   u32 = 3;  // Tabela de strings
const SHT_RELA:     u32 = 4;  // Relocações com addend
const SHT_NOBITS:   u32 = 8;  // BSS (zero-initialized)

/// Flags de seção
const SHF_ALLOC:   u64 = 0x2;  // Ocupa memória em runtime
const SHF_EXECINSTR: u64 = 0x4; // Contém código executável
const SHF_WRITE:   u64 = 0x1;  // Escrita permitida

/// Tipos de símbolo
const STT_FUNC:   u8 = 2;  // Função
const STT_OBJECT: u8 = 1;  // Variável/dado

/// Binding de símbolo
const STB_GLOBAL: u8 = 1;  // Visível externamente
const STB_WEAK:   u8 = 2;  // Fraco (pode ser sobrescrito)

/// Tipos de relocação x86_64
const R_X86_64_64:      u32 = 1;   // 64-bit absoluto
const R_X86_64_PC32:    u32 = 2;   // 32-bit relativo ao PC
const R_X86_64_PLT32:   u32 = 4;   // 32-bit PLT relativo ao PC
const R_X86_64_32:      u32 = 10;  // 32-bit zero-extended
const R_X86_64_32S:     u32 = 11;  // 32-bit sign-extended

/// Cabeçalho ELF64
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Header {
    pub e_ident:     [u8; 16],  // Magic + classe + endianness + versão
    pub e_type:      u16,       // Tipo: ET_REL, ET_EXEC, etc.
    pub e_machine:   u16,       // Arquitetura: EM_X86_64
    pub e_version:   u32,       // Versão ELF
    pub e_entry:     u64,       // Ponto de entrada (0 para relocáveis)
    pub e_phoff:     u64,       // Offset da tabela de programas
    pub e_shoff:     u64,       // Offset da tabela de seções
    pub e_flags:     u32,       // Flags específicas da arquitetura
    pub e_ehsize:    u16,       // Tamanho deste cabeçalho
    pub e_phentsize: u16,       // Tamanho de cada entrada de programa
    pub e_phnum:     u16,       // Número de entradas de programa
    pub e_shentsize: u16,       // Tamanho de cada entrada de seção
    pub e_shnum:     u16,       // Número de seções
    pub e_shstrndx:  u16,       // Índice da seção de nomes de seções
}

/// Cabeçalho de Seção ELF64
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64SectionHeader {
    pub sh_name:      u32,  // Offset do nome na strtab
    pub sh_type:      u32,  // Tipo da seção
    pub sh_flags:     u64,  // Flags (ALLOC, EXEC, WRITE)
    pub sh_addr:      u64,  // Endereço virtual (após relocação)
    pub sh_offset:    u64,  // Offset no arquivo ELF
    pub sh_size:      u64,  // Tamanho em bytes
    pub sh_link:      u32,  // Seção associada
    pub sh_info:      u32,  // Informação extra
    pub sh_addralign: u64,  // Alinhamento requerido
    pub sh_entsize:   u64,  // Tamanho de cada entrada (se tabela)
}

/// Entrada na tabela de símbolos ELF64
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Symbol {
    pub st_name:  u32,  // Offset do nome na strtab
    pub st_info:  u8,   // Tipo e binding (STT_* | STB_*)
    pub st_other: u8,   // Visibilidade
    pub st_shndx: u16,  // Índice da seção
    pub st_value: u64,  // Valor/endereço do símbolo
    pub st_size:  u64,  // Tamanho em bytes
}

impl Elf64Symbol {
    pub fn symbol_type(&self) -> u8  { self.st_info & 0xf }
    pub fn binding(&self)     -> u8  { self.st_info >> 4 }
}

/// Entrada de relocação ELF64 com addend explícito
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Rela {
    pub r_offset: u64,  // Offset a ser modificado
    pub r_info:   u64,  // Tipo + índice do símbolo
    pub r_addend: i64,  // Valor addend
}

impl Elf64Rela {
    pub fn symbol_index(&self) -> u32 { (self.r_info >> 32) as u32 }
    pub fn reloc_type(&self)   -> u32 { (self.r_info & 0xffffffff) as u32 }
}

// ─── Erros do ELF Loader ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ElfError {
    TooSmall,
    InvalidMagic,
    NotElf64,
    NotLittleEndian,
    NotRelocatable,
    WrongArchitecture,
    InvalidSectionOffset,
    UndefinedSymbol(String),
    RelocationFailed(String),
    AllocationFailed,
}

impl core::fmt::Display for ElfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ElfError::TooSmall            => write!(f, "Arquivo muito pequeno"),
            ElfError::InvalidMagic        => write!(f, "Magic ELF invalido"),
            ElfError::NotElf64            => write!(f, "Nao e ELF64"),
            ElfError::NotLittleEndian     => write!(f, "Nao e little-endian"),
            ElfError::NotRelocatable      => write!(f, "Nao e modulo relocavel"),
            ElfError::WrongArchitecture   => write!(f, "Arquitetura incorreta (requer x86_64)"),
            ElfError::InvalidSectionOffset=> write!(f, "Offset de secao invalido"),
            ElfError::UndefinedSymbol(s)  => write!(f, "Simbolo indefinido: {}", s),
            ElfError::RelocationFailed(s) => write!(f, "Relocacao falhou: {}", s),
            ElfError::AllocationFailed    => write!(f, "Falha de alocacao de memoria"),
        }
    }
}

// ─── Módulo Carregado ─────────────────────────────────────────────────────────

/// Representa um módulo ELF carregado na memória
pub struct LoadedModule {
    /// Nome do módulo
    pub name: String,
    /// Memória alocada para o módulo (código + dados)
    pub memory: Vec<u8>,
    /// Endereço base na memória virtual
    pub base_address: u64,
    /// Mapa de símbolos exportados: nome → endereço virtual
    pub exports: BTreeMap<String, u64>,
    /// Tamanho total em bytes
    pub size: usize,
}

impl LoadedModule {
    /// Retorna o endereço de uma função exportada pelo módulo
    pub fn get_function(&self, name: &str) -> Option<u64> {
        self.exports.get(name).copied()
    }
}

// ─── Tabela de Símbolos do Kernel ─────────────────────────────────────────────

/// Símbolo exportado pelo kernel para uso por módulos
#[derive(Debug, Clone)]
pub struct KernelSymbol {
    pub name: &'static str,
    pub address: u64,
}

/// Resolve um símbolo externo pelo nome.
/// Fase 2: delega para process::KERNEL_EXPORTS (tabela preenchida em runtime).
pub fn resolve_kernel_symbol(name: &str) -> Option<u64> {
    crate::modules::process::KERNEL_EXPORTS
        .lock()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.address)
}

// ─── Parser ELF ───────────────────────────────────────────────────────────────

/// Carrega e processa um módulo ELF a partir de bytes brutos
pub fn load_module(name: &str, data: &[u8]) -> Result<LoadedModule, ElfError> {
    crate::serial_println!("[ELF] Carregando modulo '{}' ({} bytes)", name, data.len());

    // ── Passo 1: Validar cabeçalho ELF ───────────────────────
    if data.len() < mem::size_of::<Elf64Header>() {
        return Err(ElfError::TooSmall);
    }

    let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

    // Verifica magic number: 0x7f 'E' 'L' 'F'
    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidMagic);
    }

    // Verifica ELF64
    if header.e_ident[4] != ELFCLASS64 {
        return Err(ElfError::NotElf64);
    }

    // Verifica little-endian
    if header.e_ident[5] != ELFDATA2LSB {
        return Err(ElfError::NotLittleEndian);
    }

    // Deve ser objeto relocável (módulo)
    if header.e_type != ET_REL {
        return Err(ElfError::NotRelocatable);
    }

    // Deve ser x86_64
    if header.e_machine != EM_X86_64 {
        return Err(ElfError::WrongArchitecture);
    }

    crate::serial_println!("[ELF] Cabecalho valido: {} secoes", header.e_shnum);

    // ── Passo 2: Parsear tabela de seções ─────────────────────
    let sh_offset = header.e_shoff as usize;
    let sh_size   = header.e_shentsize as usize;
    let sh_count  = header.e_shnum as usize;

    if sh_offset + sh_size * sh_count > data.len() {
        return Err(ElfError::InvalidSectionOffset);
    }

    // Coleta todos os cabeçalhos de seção
    let sections: Vec<Elf64SectionHeader> = (0..sh_count)
        .map(|i| {
            let offset = sh_offset + i * sh_size;
            unsafe { *(data.as_ptr().add(offset) as *const Elf64SectionHeader) }
        })
        .collect();

    // Seção de nomes de seções (.shstrtab)
    let shstrtab = &sections[header.e_shstrndx as usize];
    let shstrtab_data = &data[shstrtab.sh_offset as usize..
                               (shstrtab.sh_offset + shstrtab.sh_size) as usize];

    // Função auxiliar: lê string nula a partir de offset
    fn read_str(strtab: &[u8], offset: usize) -> &str {
        let start = offset;
        let end = strtab[start..].iter().position(|&b| b == 0)
            .map(|p| start + p)
            .unwrap_or(strtab.len());
        core::str::from_utf8(&strtab[start..end]).unwrap_or("<invalido>")
    }

    // ── Passo 3: Calcular tamanho total e alocar memória ─────
    let mut total_size: usize = 0;
    let mut section_offsets: Vec<Option<usize>> = alloc::vec![None; sh_count];

    for (i, sec) in sections.iter().enumerate() {
        if sec.sh_flags & SHF_ALLOC == 0 || sec.sh_type == SHT_NULL {
            continue;
        }

        // Alinha o offset ao alinhamento requerido pela seção
        let align = sec.sh_addralign as usize;
        if align > 1 {
            total_size = (total_size + align - 1) & !(align - 1);
        }

        section_offsets[i] = Some(total_size);
        total_size += sec.sh_size as usize;
    }

    if total_size == 0 {
        return Err(ElfError::AllocationFailed);
    }

    // Aloca memória contígua para todo o módulo
    let mut module_memory: Vec<u8> = alloc::vec![0u8; total_size];
    let base_address = module_memory.as_ptr() as u64;

    crate::serial_println!("[ELF] Alocado {} bytes em 0x{:x}", total_size, base_address);

    // ── Passo 4: Copiar seções para a memória alocada ─────────
    for (i, sec) in sections.iter().enumerate() {
        let offset = match section_offsets[i] {
            Some(o) => o,
            None => continue,
        };

        if sec.sh_type == SHT_NOBITS {
            // .bss: já está zerado pela alocação
            continue;
        }

        if sec.sh_type == SHT_PROGBITS {
            let src_start = sec.sh_offset as usize;
            let src_end   = src_start + sec.sh_size as usize;
            if src_end <= data.len() {
                let dst_end = offset + sec.sh_size as usize;
                module_memory[offset..dst_end].copy_from_slice(&data[src_start..src_end]);

                let sec_name = read_str(shstrtab_data, sec.sh_name as usize);
                crate::serial_println!("[ELF]   Secao {} -> offset 0x{:x} ({} bytes)",
                    sec_name, offset, sec.sh_size);
            }
        }
    }

    // ── Passo 5: Encontrar tabela de símbolos ─────────────────
    let mut symtab_section: Option<&Elf64SectionHeader> = None;
    let mut strtab_section: Option<&Elf64SectionHeader> = None;

    for sec in &sections {
        match sec.sh_type {
            SHT_SYMTAB => symtab_section = Some(sec),
            SHT_STRTAB => {
                // Evita pegar .shstrtab como strtab de símbolos
                let name = read_str(shstrtab_data, sec.sh_name as usize);
                if name == ".strtab" {
                    strtab_section = Some(sec);
                }
            }
            _ => {}
        }
    }

    // ── Passo 6: Construir mapa de símbolos ───────────────────
    let mut symbol_map: BTreeMap<u32, u64> = BTreeMap::new(); // índice → endereço
    let mut exports: BTreeMap<String, u64> = BTreeMap::new();

    if let (Some(symtab), Some(strtab)) = (symtab_section, strtab_section) {
        let sym_data  = &data[symtab.sh_offset as usize..(symtab.sh_offset + symtab.sh_size) as usize];
        let str_data  = &data[strtab.sh_offset as usize..(strtab.sh_offset + strtab.sh_size) as usize];
        let sym_count = symtab.sh_size as usize / mem::size_of::<Elf64Symbol>();

        for i in 0..sym_count {
            let sym = unsafe {
                *(sym_data.as_ptr().add(i * mem::size_of::<Elf64Symbol>()) as *const Elf64Symbol)
            };

            let sym_name = read_str(str_data, sym.st_name as usize);

            // Símbolo definido (não externo)
            if sym.st_shndx != 0 && sym.st_shndx < sh_count as u16 {
                let sec_idx = sym.st_shndx as usize;
                if let Some(sec_offset) = section_offsets[sec_idx] {
                    let addr = base_address + sec_offset as u64 + sym.st_value;
                    symbol_map.insert(i as u32, addr);

                    // Exporta símbolos globais
                    if sym.binding() == STB_GLOBAL && !sym_name.is_empty() {
                        exports.insert(sym_name.to_string(), addr);
                    }
                }
            }
            // Símbolo externo — resolve no kernel
            else if sym.st_shndx == 0 && !sym_name.is_empty() {
                if let Some(addr) = resolve_kernel_symbol(sym_name) {
                    symbol_map.insert(i as u32, addr);
                } else if sym.binding() != STB_WEAK {
                    crate::serial_println!("[ELF] Aviso: simbolo externo nao resolvido: {}", sym_name);
                    // Em Fase 1, continua; Fase 2 retornará erro aqui
                }
            }
        }

        crate::serial_println!("[ELF] {} simbolos processados, {} exportados",
            sym_count, exports.len());
    }

    // ── Passo 7: Aplicar relocações ───────────────────────────
    for (sec_idx, sec) in sections.iter().enumerate() {
        if sec.sh_type != SHT_RELA {
            continue;
        }

        // A seção alvo da relocação é sh_info
        let target_sec_idx = sec.sh_info as usize;
        let target_offset = match section_offsets.get(target_sec_idx).and_then(|o| *o) {
            Some(o) => o,
            None => continue,
        };

        let rela_count = sec.sh_size as usize / mem::size_of::<Elf64Rela>();
        let rela_data = &data[sec.sh_offset as usize..(sec.sh_offset + sec.sh_size) as usize];

        for i in 0..rela_count {
            let rela = unsafe {
                *(rela_data.as_ptr().add(i * mem::size_of::<Elf64Rela>()) as *const Elf64Rela)
            };

            let sym_addr = symbol_map
                .get(&rela.symbol_index())
                .copied()
                .unwrap_or(0);

            let patch_offset = target_offset + rela.r_offset as usize;
            if patch_offset + 8 > module_memory.len() {
                continue;
            }

            let patch_ptr = module_memory.as_mut_ptr().wrapping_add(patch_offset);
            let patch_addr = base_address + patch_offset as u64;

            apply_relocation(
                patch_ptr,
                rela.reloc_type(),
                sym_addr,
                rela.r_addend,
                patch_addr,
            )?;
        }
    }

    crate::serial_println!("[ELF] Modulo '{}' carregado com sucesso!", name);

    Ok(LoadedModule {
        name: name.to_string(),
        memory: module_memory,
        base_address,
        exports,
        size: total_size,
    })
}

/// Aplica uma relocação x86_64 no local correto
fn apply_relocation(
    patch: *mut u8,
    reloc_type: u32,
    sym_addr: u64,
    addend: i64,
    patch_addr: u64,
) -> Result<(), ElfError> {
    unsafe {
        match reloc_type {
            // S + A — 64-bit absoluto
            R_X86_64_64 => {
                let value = sym_addr.wrapping_add(addend as u64);
                (patch as *mut u64).write_unaligned(value);
            }

            // S + A - P — 32-bit relativo ao PC
            R_X86_64_PC32 | R_X86_64_PLT32 => {
                let value = sym_addr.wrapping_add(addend as u64).wrapping_sub(patch_addr);
                let value32 = value as i64 as i32;
                (patch as *mut i32).write_unaligned(value32);
            }

            // S + A — 32-bit zero-extended
            R_X86_64_32 => {
                let value = sym_addr.wrapping_add(addend as u64) as u32;
                (patch as *mut u32).write_unaligned(value);
            }

            // S + A — 32-bit sign-extended
            R_X86_64_32S => {
                let value = sym_addr.wrapping_add(addend as u64) as i32;
                (patch as *mut i32).write_unaligned(value);
            }

            // Tipos não suportados na Fase 1
            other => {
                crate::serial_println!("[ELF] Aviso: tipo de relocacao nao suportado: {}", other);
            }
        }
    }
    Ok(())
}

// ─── Gerenciador de Módulos ELF ───────────────────────────────────────────────

use spinning_top::Spinlock;

/// Gerenciador global de módulos carregados dinamicamente
pub struct ElfModuleManager {
    modules: Vec<LoadedModule>,
}

impl ElfModuleManager {
    const fn new() -> Self {
        Self { modules: Vec::new() }
    }

    /// Carrega um módulo ELF e o adiciona ao gerenciador
    pub fn load(&mut self, name: &str, data: &[u8]) -> Result<(), ElfError> {
        // Evita carregar o mesmo módulo duas vezes
        if self.modules.iter().any(|m| m.name == name) {
            crate::serial_println!("[ELF] Modulo '{}' ja carregado", name);
            return Ok(());
        }

        let module = load_module(name, data)?;
        self.modules.push(module);
        Ok(())
    }

    /// Descarrega um módulo pelo nome
    pub fn unload(&mut self, name: &str) -> bool {
        let before = self.modules.len();
        self.modules.retain(|m| m.name != name);
        let removed = self.modules.len() < before;
        if removed {
            crate::serial_println!("[ELF] Modulo '{}' descarregado", name);
        }
        removed
    }

    /// Busca função exportada em qualquer módulo carregado
    pub fn find_symbol(&self, symbol: &str) -> Option<u64> {
        self.modules.iter().find_map(|m| m.get_function(symbol))
    }

    /// Lista todos os módulos carregados
    pub fn list(&self) -> Vec<(&str, u64, usize)> {
        self.modules
            .iter()
            .map(|m| (m.name.as_str(), m.base_address, m.size))
            .collect()
    }

    /// Total de memória usada por módulos ELF
    pub fn total_memory(&self) -> usize {
        self.modules.iter().map(|m| m.size).sum()
    }
}

pub static ELF_MANAGER: Spinlock<ElfModuleManager> =
    Spinlock::new(ElfModuleManager::new());

/// API pública: carrega um módulo ELF
pub fn load_elf_module(name: &str, data: &[u8]) -> Result<(), ElfError> {
    ELF_MANAGER.lock().load(name, data)
}

/// API pública: descarrega um módulo ELF
pub fn unload_elf_module(name: &str) -> bool {
    ELF_MANAGER.lock().unload(name)
}

/// API pública: busca símbolo em módulos carregados
pub fn find_module_symbol(symbol: &str) -> Option<u64> {
    ELF_MANAGER.lock().find_symbol(symbol)
}
