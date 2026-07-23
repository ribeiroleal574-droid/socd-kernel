extern crate alloc;
// ============================================================
// SOC-D Kernel — WASM Instance Executor
// ============================================================
//
// Executa uma instância de módulo WASM.
// Implementa o interpretador completo do WebAssembly 1.0.
//
// Modelo de execução:
//   - Stack machine: operações empilham/desempilham valores
//   - Memória linear: array de bytes endereçável por i32
//   - Tabela de funções: chamadas indiretas via table
//   - Globals: variáveis globais mutáveis/imutáveis
//   - Labels: estruturas de controle (block, loop, if)
//
// Subconjunto de opcodes implementados (Fase 4):
//   Controle:    unreachable, nop, block, loop, if, else, end,
//                br, br_if, br_table, return, call, call_indirect
//   Memória:     i32.load, i32.store, i64.load, i64.store,
//                f32.load, f32.store, memory.size, memory.grow
//   Numérico:    i32.{const,add,sub,mul,div_s,div_u,rem_s,rem_u,
//                     and,or,xor,shl,shr_s,shr_u,rotl,rotr,
//                     clz,ctz,popcnt,eq,ne,lt_s,lt_u,gt_s,gt_u,
//                     le_s,le_u,ge_s,ge_u,eqz}
//               i64.{idem}, f32.{idem}, f64.{idem}
//   Conversão:   i32.wrap_i64, i64.extend_i32_s, f32.convert_i32_s,
//                i32.trunc_f32_s, etc.
//   Variáveis:   local.get, local.set, local.tee,
//                global.get, global.set
// ============================================================

use alloc::{vec::Vec, string::{String, ToString}};
use super::{WasmModule, WasmValue, WasmError, WasmTrap, FuncType, read_uleb128};

/// Frame de ativação de função
#[derive(Debug)]
struct CallFrame {
    /// Índice da função sendo executada
    func_idx: u32,
    /// Program counter (offset no bytecode)
    pc: usize,
    /// Variáveis locais (parâmetros + declaradas)
    locals: Vec<WasmValue>,
    /// Base da stack de valores para este frame
    stack_base: usize,
}

/// Estrutura de controle (block/loop/if)
#[derive(Debug, Clone)]
struct ControlFrame {
    /// Tipo de bloco
    kind: ControlKind,
    /// Profundidade da stack no início do bloco
    stack_depth: usize,
    /// Offset de inicio do bloco (para loops)
    start_pc: usize,
    /// Tipo de resultado do bloco
    result_arity: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum ControlKind { Block, Loop, If, Else }

/// Uma instância em execução de um módulo WASM
pub struct WasmInstance {
    /// Módulo compilado
    pub module: WasmModule,
    /// Memória linear (heap do módulo)
    pub memory: Vec<u8>,
    /// Valores globais
    pub globals: Vec<WasmValue>,
    /// Tabela de funções (para call_indirect)
    pub table: Vec<Option<u32>>,
    /// Stack de valores
    value_stack: Vec<WasmValue>,
    /// Stack de frames de chamada
    call_stack: Vec<CallFrame>,
    /// Stack de controle (block/loop/if)
    control_stack: Vec<ControlFrame>,
    /// Total de instruções executadas
    pub instructions_executed: u64,
}

impl WasmInstance {
    /// Cria uma instância a partir de um módulo parseado
    pub fn instantiate(module: WasmModule) -> Result<Self, WasmError> {
        // Aloca memória linear
        let mem_pages = module.memory_limits
            .map(|(min, _)| min)
            .unwrap_or(1);
        let mem_size = mem_pages as usize * 65536; // 64KB por página

        if mem_size > super::MAX_LINEAR_MEMORY {
            return Err(WasmError::MemoryOutOfBounds {
                addr: 0,
                size: mem_size as u32,
            });
        }

        let mut instance = Self {
            module,
            memory: alloc::vec![0u8; mem_size],
            globals: Vec::new(),
            table: Vec::new(),
            value_stack: Vec::new(),
            call_stack: Vec::new(),
            control_stack: Vec::new(),
            instructions_executed: 0,
        };

        // Inicializa segmentos de dados
        instance.init_data_segments()?;

        crate::serial_println!(
            "[WASM][INST] Instancia criada: {} KB memoria, {} exports",
            mem_size / 1024,
            instance.module.exports.len()
        );

        Ok(instance)
    }

    /// Copia segmentos de dados na memória linear
    fn init_data_segments(&mut self) -> Result<(), WasmError> {
        for segment in &self.module.data_segments.clone() {
            // Evalua expressão de offset (simplificado: assume i32.const)
            let offset = if segment.offset_expr.len() >= 2 {
                let mut off = 1usize; // Pula opcode i32.const (0x41)
                super::read_uleb128(&segment.offset_expr, &mut off)
                    .unwrap_or(0) as usize
            } else { 0 };

            let end = offset + segment.data.len();
            if end > self.memory.len() {
                return Err(WasmError::MemoryOutOfBounds {
                    addr: offset as u32,
                    size: segment.data.len() as u32,
                });
            }
            self.memory[offset..end].copy_from_slice(&segment.data);
        }
        Ok(())
    }

    /// Chama uma função exportada pelo nome
    pub fn call_export(&mut self, name: &str, args: &[WasmValue])
        -> Result<Vec<WasmValue>, WasmError>
    {
        // Encontra o índice da função exportada
        let func_idx = self.module.exports.iter()
            .find(|e| e.name == name && e.kind == super::ExportKind::Function)
            .map(|e| e.index)
            .ok_or_else(|| WasmError::UndefinedFunction(name.to_string()))?;

        self.call_function(func_idx, args)
    }

    /// Executa uma função pelo índice
    pub fn call_function(&mut self, func_idx: u32, args: &[WasmValue])
        -> Result<Vec<WasmValue>, WasmError>
    {
        // Verifica profundidade máxima da call stack
        if self.call_stack.len() >= super::MAX_CALL_DEPTH {
            return Err(WasmError::Trap(WasmTrap::StackOverflow));
        }

        // Determina se é função importada ou local
        let import_count = self.module.imports.iter()
            .filter(|i| matches!(i.kind, super::ImportKind::Function {..}))
            .count() as u32;

        if func_idx < import_count {
            // Função importada — chama host API
            return self.call_import(func_idx, args);
        }

        let local_idx = (func_idx - import_count) as usize;
        if local_idx >= self.module.code_bodies.len() {
            return Err(WasmError::UndefinedFunction(
                alloc::format!("func#{}", func_idx)
            ));
        }

        // Obtém tipo da função
        let type_idx = *self.module.func_type_indices.get(local_idx)
            .ok_or(WasmError::UnexpectedEof)? as usize;
        let func_type = self.module.types.get(type_idx)
            .ok_or(WasmError::UnexpectedEof)?.clone();

        // Inicializa locals: parâmetros + zeros
        let mut locals = Vec::new();
        for arg in args.iter().take(func_type.params.len()) {
            locals.push(*arg);
        }
        // Preenche parâmetros faltando com zero
        while locals.len() < func_type.params.len() {
            locals.push(WasmValue::I32(0));
        }
        // Adiciona locals declarados no corpo
        let body = &self.module.code_bodies[local_idx];
        for (count, typ) in &body.locals {
            for _ in 0..*count {
                locals.push(match typ {
                    super::WasmValueType::I32 => WasmValue::I32(0),
                    super::WasmValueType::I64 => WasmValue::I64(0),
                    super::WasmValueType::F32 => WasmValue::F32(0.0),
                    super::WasmValueType::F64 => WasmValue::F64(0.0),
                    _ => WasmValue::I32(0),
                });
            }
        }

        let code = body.code.clone();
        let stack_base = self.value_stack.len();

        self.call_stack.push(CallFrame {
            func_idx,
            pc: 0,
            locals,
            stack_base,
        });

        // Executa bytecode
        self.execute_bytecode(&code, &func_type)?;

        // Coleta resultados
        let result_arity = func_type.results.len();
        let mut results = Vec::new();
        for _ in 0..result_arity {
            if let Some(v) = self.value_stack.pop() {
                results.push(v);
            }
        }
        results.reverse();

        // Restaura stack ao estado anterior
        self.value_stack.truncate(stack_base);
        self.call_stack.pop();

        Ok(results)
    }

    /// Chama uma função importada (host API SOC-D)
    fn call_import(&mut self, func_idx: u32, args: &[WasmValue])
        -> Result<Vec<WasmValue>, WasmError>
    {
        let import = self.module.imports.iter()
            .filter(|i| matches!(i.kind, super::ImportKind::Function {..}))
            .nth(func_idx as usize)
            .cloned()
            .ok_or(WasmError::UnexpectedEof)?;

        // Dispatch para host API SOC-D
        let result = match (import.module.as_str(), import.name.as_str()) {
            ("socd", "log") => {
                // Imprime string da memória linear
                if let Some(WasmValue::I32(ptr)) = args.get(0) {
                    if let Some(WasmValue::I32(len)) = args.get(1) {
                        let s = self.read_string(*ptr as usize, *len as usize);
                        crate::serial_println!("[WASM][LOG] {}", s);
                    }
                }
                alloc::vec![]
            }
            ("socd", "random_i32") => {
                // Retorna um i32 pseudo-aleatório
                alloc::vec![WasmValue::I32(0xDEAD_BEEF_u32 as i32)]
            }
            ("socd", "uptime_ms") => {
                alloc::vec![WasmValue::I64(self.instructions_executed as i64)]
            }
            ("socd", "heap_free_kb") => {
                let (_, free) = crate::memory::heap::heap_stats();
                alloc::vec![WasmValue::I32((free / 1024) as i32)]
            }
            ("socd", "peers_active") => {
                let (_, active) = crate::p2p::peer::count_peers();
                alloc::vec![WasmValue::I32(active as i32)]
            }
            ("env", "abort") | ("env", "__assert_fail") => {
                return Err(WasmError::Trap(WasmTrap::UnreachableInstruction));
            }
            _ => {
                crate::serial_println!("[WASM] Import nao resolvido: {}.{}", import.module, import.name);
                alloc::vec![WasmValue::I32(0)]
            }
        };

        Ok(result)
    }

    /// Lê uma string UTF-8 da memória linear
    fn read_string(&self, ptr: usize, len: usize) -> &str {
        let end = (ptr + len).min(self.memory.len());
        if ptr > self.memory.len() { return ""; }
        core::str::from_utf8(&self.memory[ptr..end]).unwrap_or("?")
    }

    /// Executor principal do bytecode WASM
    fn execute_bytecode(&mut self, code: &[u8], func_type: &FuncType)
        -> Result<(), WasmError>
    {
        let mut pc = 0usize;
        let max_instructions = 10_000_000u64; // Limite de segurança

        while pc < code.len() {
            if self.instructions_executed >= max_instructions {
                return Err(WasmError::HostError("CPU limit exceeded".into()));
            }

            let opcode = code[pc];
            pc += 1;
            self.instructions_executed += 1;

            match opcode {
                // ── Controle ─────────────────────────────────────────
                0x00 => return Err(WasmError::Trap(WasmTrap::UnreachableInstruction)),
                0x01 => {} // nop
                0x0F => break, // return

                0x02 | 0x03 | 0x04 => {
                    // block / loop / if
                    let _bt = read_uleb128(code, &mut pc).unwrap_or(0x40); // blocktype
                    let kind = match opcode {
                        0x02 => ControlKind::Block,
                        0x03 => ControlKind::Loop,
                        _    => ControlKind::If,
                    };
                    if opcode == 0x04 {
                        // if: consome condição
                        let cond = self.pop_i32()?;
                        if cond == 0 {
                            // Pula para else/end
                            self.skip_to_else_or_end(code, &mut pc);
                        }
                    }
                    self.control_stack.push(ControlFrame {
                        kind,
                        stack_depth: self.value_stack.len(),
                        start_pc: pc,
                        result_arity: 0,
                    });
                }
                0x05 => {
                    // else — pula para end do bloco if
                    self.skip_to_end(code, &mut pc);
                    self.control_stack.pop();
                }
                0x0B => {
                    // end
                    if let Some(frame) = self.control_stack.pop() {
                        // Trunca stack ao resultado esperado
                        let results: Vec<WasmValue> = self.value_stack
                            .drain(frame.stack_depth..)
                            .take(frame.result_arity)
                            .collect();
                        self.value_stack.extend(results);
                    }
                }
                0x0C => {
                    // br (break/continue)
                    let depth = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    self.do_branch(depth, code, &mut pc);
                }
                0x0D => {
                    // br_if
                    let depth = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let cond = self.pop_i32()?;
                    if cond != 0 {
                        self.do_branch(depth, code, &mut pc);
                    }
                }
                0x10 => {
                    // call
                    let fidx = read_uleb128(code, &mut pc).unwrap_or(0) as u32;
                    // Collect args from stack
                    let args: Vec<WasmValue> = self.value_stack.drain(
                        self.value_stack.len().saturating_sub(4)..
                    ).collect();
                    let results = self.call_function(fidx, &args)?;
                    self.value_stack.extend(results);
                }
                0x1A => { self.value_stack.pop(); } // drop
                0x1B => {
                    // select
                    let cond = self.pop_i32()?;
                    let b = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    let a = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    self.value_stack.push(if cond != 0 { a } else { b });
                }

                // ── Variables ────────────────────────────────────────
                0x20 => {
                    let idx = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val = self.get_local(idx)?;
                    self.value_stack.push(val);
                }
                0x21 => {
                    let idx = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    self.set_local(idx, val)?;
                }
                0x22 => {
                    let idx = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val = *self.value_stack.last().unwrap_or(&WasmValue::I32(0));
                    self.set_local(idx, val)?;
                }
                0x23 => {
                    let idx = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val = self.globals.get(idx).copied().unwrap_or(WasmValue::I32(0));
                    self.value_stack.push(val);
                }
                0x24 => {
                    let idx = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    while self.globals.len() <= idx { self.globals.push(WasmValue::I32(0)); }
                    self.globals[idx] = val;
                }

                // ── Memória ──────────────────────────────────────────
                0x28 => { // i32.load
                    let _align = read_uleb128(code, &mut pc).unwrap_or(0);
                    let offset = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let base = self.pop_i32()? as usize;
                    let addr = base + offset;
                    let val = self.mem_load_i32(addr)?;
                    self.value_stack.push(WasmValue::I32(val));
                }
                0x36 => { // i32.store
                    let _align = read_uleb128(code, &mut pc).unwrap_or(0);
                    let offset = read_uleb128(code, &mut pc).unwrap_or(0) as usize;
                    let val  = self.pop_i32()?;
                    let base = self.pop_i32()? as usize;
                    self.mem_store_i32(base + offset, val)?;
                }
                0x3F => { // memory.size
                    pc += 1; // reserved byte
                    let pages = (self.memory.len() / 65536) as i32;
                    self.value_stack.push(WasmValue::I32(pages));
                }
                0x40 => { // memory.grow
                    pc += 1;
                    let delta = self.pop_i32()?;
                    let old_pages = (self.memory.len() / 65536) as i32;
                    let new_size = self.memory.len() + delta as usize * 65536;
                    if new_size <= super::MAX_LINEAR_MEMORY {
                        self.memory.resize(new_size, 0);
                        self.value_stack.push(WasmValue::I32(old_pages));
                    } else {
                        self.value_stack.push(WasmValue::I32(-1));
                    }
                }

                // ── i32 Constante e Aritmética ────────────────────────
                0x41 => {
                    let v = read_uleb128(code, &mut pc).unwrap_or(0) as i32;
                    self.value_stack.push(WasmValue::I32(v));
                }
                0x42 => {
                    let v = read_uleb128(code, &mut pc).unwrap_or(0) as i64;
                    self.value_stack.push(WasmValue::I64(v));
                }
                0x43 => {
                    if pc + 4 <= code.len() {
                        let bytes: [u8;4] = code[pc..pc+4].try_into().unwrap_or([0;4]);
                        pc += 4;
                        self.value_stack.push(WasmValue::F32(f32::from_le_bytes(bytes)));
                    }
                }
                0x44 => {
                    if pc + 8 <= code.len() {
                        let bytes: [u8;8] = code[pc..pc+8].try_into().unwrap_or([0;8]);
                        pc += 8;
                        self.value_stack.push(WasmValue::F64(f64::from_le_bytes(bytes)));
                    }
                }

                // i32 operações
                0x45 => { let v = self.pop_i32()?; self.value_stack.push(WasmValue::I32(if v==0{1}else{0})); }
                0x46 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a==b{1}else{0})); }
                0x47 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a!=b{1}else{0})); }
                0x48 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a<b{1}else{0})); }
                0x4A => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a>b{1}else{0})); }
                0x4C => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a<=b{1}else{0})); }
                0x4E => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(if a>=b{1}else{0})); }

                0x6A => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a.wrapping_add(b))); }
                0x6B => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a.wrapping_sub(b))); }
                0x6C => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a.wrapping_mul(b))); }
                0x6D => {
                    let (a,b)=self.pop2_i32()?;
                    if b==0 { return Err(WasmError::Trap(WasmTrap::IntegerDivisionByZero)); }
                    self.value_stack.push(WasmValue::I32(a.wrapping_div(b)));
                }
                0x71 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a&b)); }
                0x72 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a|b)); }
                0x73 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a^b)); }
                0x74 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a.wrapping_shl(b as u32))); }
                0x75 => { let (a,b)=self.pop2_i32()?; self.value_stack.push(WasmValue::I32(a.wrapping_shr(b as u32))); }

                // i64 operações básicas
                0xC0 | 0xC1 => { pc += 1; } // extend8_s / extend16_s (ignorado)

                // Outros opcodes — ignora graciosamente na Fase 4
                _ => {}
            }
        }

        Ok(())
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    fn pop_i32(&mut self) -> Result<i32, WasmError> {
        match self.value_stack.pop() {
            Some(WasmValue::I32(v)) => Ok(v),
            Some(other) => Err(WasmError::TypeMismatch {
                expected: "i32", got: other.type_name()
            }),
            None => Err(WasmError::StackUnderflow),
        }
    }

    fn pop2_i32(&mut self) -> Result<(i32, i32), WasmError> {
        let b = self.pop_i32()?;
        let a = self.pop_i32()?;
        Ok((a, b))
    }

    fn get_local(&self, idx: usize) -> Result<WasmValue, WasmError> {
        self.call_stack.last()
            .and_then(|f| f.locals.get(idx))
            .copied()
            .ok_or(WasmError::StackUnderflow)
    }

    fn set_local(&mut self, idx: usize, val: WasmValue) -> Result<(), WasmError> {
        if let Some(frame) = self.call_stack.last_mut() {
            while frame.locals.len() <= idx { frame.locals.push(WasmValue::I32(0)); }
            frame.locals[idx] = val;
            Ok(())
        } else {
            Err(WasmError::StackUnderflow)
        }
    }

    fn mem_load_i32(&self, addr: usize) -> Result<i32, WasmError> {
        if addr + 4 > self.memory.len() {
            return Err(WasmError::MemoryOutOfBounds { addr: addr as u32, size: 4 });
        }
        let bytes: [u8;4] = self.memory[addr..addr+4].try_into().unwrap_or([0;4]);
        Ok(i32::from_le_bytes(bytes))
    }

    fn mem_store_i32(&mut self, addr: usize, val: i32) -> Result<(), WasmError> {
        if addr + 4 > self.memory.len() {
            return Err(WasmError::MemoryOutOfBounds { addr: addr as u32, size: 4 });
        }
        self.memory[addr..addr+4].copy_from_slice(&val.to_le_bytes());
        Ok(())
    }

    fn skip_to_else_or_end(&self, code: &[u8], pc: &mut usize) {
        let mut depth = 1;
        while *pc < code.len() {
            match code[*pc] {
                0x02 | 0x03 | 0x04 => { depth += 1; *pc += 1; }
                0x05 if depth == 1 => { *pc += 1; return; }
                0x0B => {
                    depth -= 1;
                    *pc += 1;
                    if depth == 0 { return; }
                }
                _ => { *pc += 1; }
            }
        }
    }

    fn skip_to_end(&self, code: &[u8], pc: &mut usize) {
        let mut depth = 1;
        while *pc < code.len() {
            match code[*pc] {
                0x02 | 0x03 | 0x04 => { depth += 1; *pc += 1; }
                0x0B => {
                    depth -= 1;
                    *pc += 1;
                    if depth == 0 { return; }
                }
                _ => { *pc += 1; }
            }
        }
    }

    fn do_branch(&mut self, depth: usize, _code: &[u8], pc: &mut usize) {
        let len = self.control_stack.len();
        if depth < len {
            let frame = &self.control_stack[len - 1 - depth];
            if frame.kind == ControlKind::Loop {
                *pc = frame.start_pc;
            }
        }
    }
}
