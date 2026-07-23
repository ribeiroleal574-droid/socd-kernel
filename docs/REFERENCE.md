# SOC-D Kernel — Documentação Técnica Completa

**Sistema Operacional Cognitivo Distribuído**  
Versão: 0.1.0-alpha | Arquitetura: x86_64, AArch64

---

## Índice

1. [Visão Geral](#visao-geral)
2. [Arquitetura](#arquitetura)
3. [Módulos do Kernel](#modulos)
4. [API de Syscalls](#syscalls)
5. [API P2P](#p2p-api)
6. [API de IA](#ia-api)
7. [API Edge Computing](#edge-api)
8. [API WASM](#wasm-api)
9. [API OpenXR](#xr-api)
10. [API Quântica](#quantum-api)
11. [Build & Deploy](#build)
12. [Desenvolvimento de Módulos](#modulo-dev)

---

## 1. Visão Geral

O SOC-D é um sistema operacional experimental que integra quatro pilares em um único kernel:

| Pilar | Descrição | Status |
|---|---|---|
| **Kernel Cognitivo** | Microkernel modular com IA no núcleo | ✅ Fase 1-2 |
| **Nuvem P2P Pessoal** | Cluster distribuído entre seus dispositivos | ✅ Fase 2 |
| **Interface Imersiva** | Desktop + Mobile + AR/VR sincronizados | ✅ Fase 3 |
| **Computação Avançada** | Edge + WASM + Quântico | ✅ Fase 4 |

### Diferencial Central

```
Sistema Tradicional:    App → OS → Hardware
SOC-D:                  App → OS ←→ IA ←→ P2P ←→ Edge ←→ Quantum
```

---

## 2. Arquitetura

### Stack Completa

```
┌──────────────────────────────────────────────────────────────┐
│                     Apps (WASM / Native)                      │
├──────────────────────────────────────────────────────────────┤
│              Syscall Interface (115 syscalls)                  │
├────────────┬───────────┬──────────┬──────────┬───────────────┤
│  UI Stack  │  P2P Net  │ IA Engine│  Edge    │   Quantum     │
│ (Wayland)  │ (libp2p)  │ (ONNX)  │Computing │  (Qiskit)     │
├────────────┴───────────┴──────────┴──────────┴───────────────┤
│          WASM Runtime  │  Net Stack  │  TmpFS                 │
├──────────────────────────────────────────────────────────────┤
│     Scheduler  │  Memory  │  Security  │  Modules            │
├──────────────────────────────────────────────────────────────┤
│           Kernel Core (GDT, IDT, Interrupts)                  │
├──────────────────────────────────────────────────────────────┤
│    x86_64 (QEMU/Bare Metal) │ AArch64 (RPi4 / QEMU virt)    │
└──────────────────────────────────────────────────────────────┘
```

### Estrutura de Arquivos

```
socd-kernel/
├── src/
│   ├── main.rs              ← Entry point, sequência de boot
│   ├── arch/
│   │   ├── mod.rs           ← Abstração de arquitetura
│   │   ├── gdt.rs           ← Global Descriptor Table (x86_64)
│   │   ├── interrupts.rs    ← IDT + handlers IRQ
│   │   ├── port.rs          ← I/O port access
│   │   └── arm/
│   │       ├── mod.rs       ← CPU info, build targets
│   │       ├── exception.rs ← Tabela de vetores ARM64
│   │       ├── gic.rs       ← Generic Interrupt Controller
│   │       └── mmu.rs       ← VMSA, tabelas de página
│   ├── memory/
│   │   ├── mod.rs
│   │   ├── heap.rs          ← Heap 1MB (linked_list_allocator)
│   │   ├── paging.rs        ← Page tables, virtual memory
│   │   └── frame_allocator.rs ← Alocação de frames físicos
│   ├── modules/
│   │   ├── mod.rs           ← KernelModule trait, built-ins
│   │   ├── registry.rs      ← Registro global de módulos
│   │   ├── scheduler.rs     ← Scheduler preemptivo (5 prioridades)
│   │   ├── elf_loader.rs    ← ELF loader para módulos externos
│   │   └── tmpfs.rs         ← Sistema de arquivos em RAM
│   ├── security/
│   │   ├── mod.rs           ← TrustLevel, SecurityEvent
│   │   ├── sandbox.rs       ← Isolamento de processos
│   │   └── policy.rs        ← Políticas de privacidade
│   ├── drivers/
│   │   ├── mod.rs
│   │   ├── vga.rs           ← VGA texto 80×25 + print! macro
│   │   ├── serial.rs        ← UART serial + serial_println!
│   │   └── keyboard.rs      ← PS/2 + shell de debug
│   ├── p2p/
│   │   ├── mod.rs           ← Estado global P2P
│   │   ├── node.rs          ← Identidade Ed25519
│   │   ├── peer.rs          ← Tabela de peers, trust score
│   │   ├── discovery.rs     ← mDNS (RFC 6762)
│   │   ├── crypto.rs        ← X25519 + AES-256-GCM
│   │   ├── gossip.rs        ← Gossip Protocol (fanout=3, TTL=7)
│   │   ├── routing.rs       ← Kademlia-like XOR routing
│   │   └── transport.rs     ← Transport layer
│   ├── ia/
│   │   ├── mod.rs           ← Motor de IA global
│   │   ├── collector.rs     ← Ring buffer 512 amostras
│   │   ├── model.rs         ← 3 modelos ONNX-compatíveis
│   │   ├── predictor.rs     ← Agregador de predições
│   │   ├── optimizer.rs     ← Otimizações automáticas
│   │   └── suggest.rs       ← Sugestões ao usuário
│   ├── ui/
│   │   ├── mod.rs           ← Color, Rect, paleta SOC-D
│   │   ├── render.rs        ← Framebuffer 1024×768, fonte 8×8
│   │   ├── compositor.rs    ← Superfícies, z-ordering, alpha
│   │   ├── shell.rs         ← Desktop shell, taskbar, monitor
│   │   ├── widgets.rs       ← Label, Button, Progress, Input
│   │   └── input.rs         ← Input unificado
│   ├── edge/
│   │   ├── mod.rs           ← Edge computing global
│   │   ├── node.rs          ← Perfil de capacidade dos nós
│   │   ├── task.rs          ← Tarefas e fila de execução
│   │   ├── balancer.rs      ← Balanceamento + coletor
│   │   ├── collector.rs
│   │   └── protocol.rs      ← Protocolo de mensagens
│   ├── wasm/
│   │   ├── mod.rs           ← Runtime WASM + parser
│   │   └── instance.rs      ← Executor de bytecode completo
│   ├── xr/
│   │   └── mod.rs           ← OpenXR runtime, poses, frames
│   ├── quantum/
│   │   └── mod.rs           ← Circuitos, simulador, jobs
│   ├── net/
│   │   ├── mod.rs           ← Stack de rede, interfaces
│   │   ├── virtio.rs        ← Driver virtio-net completo
│   │   └── ethernet.rs      ← ETH, IP, TCP, UDP, Socket, DNS
│   └── syscall/
│       └── mod.rs           ← 115 syscalls POSIX + SOC-D
├── tests/
│   └── kernel_tests.rs      ← 12 testes de integração (QEMU)
├── .github/workflows/
│   └── ci.yml               ← CI/CD completo (build/test/release)
├── Cargo.toml
├── Makefile
├── README.md
├── FASE3.md
└── rust-toolchain.toml
```

---

## 3. Módulos do Kernel

### 3.1 Trait KernelModule

Todo módulo do SOC-D implementa esta interface:

```rust
pub trait KernelModule: Send + Sync {
    fn name(&self)        -> &'static str;
    fn version(&self)     -> &'static str;
    fn description(&self) -> &'static str;
    fn dependencies(&self) -> &'static [&'static str] { &[] }
    fn priority(&self)    -> ModulePriority { ModulePriority::Normal }
    fn init(&self)        -> Result<(), &'static str>;
    fn cleanup(&self);
}
```

**Exemplo de módulo externo:**

```rust
struct MyModule;
impl KernelModule for MyModule {
    fn name(&self)    -> &'static str { "my.module" }
    fn version(&self) -> &'static str { "1.0.0" }
    fn description(&self) -> &'static str { "Meu primeiro módulo SOC-D" }
    fn priority(&self)    -> ModulePriority { ModulePriority::Normal }
    fn init(&self) -> Result<(), &'static str> {
        // Inicialização aqui
        Ok(())
    }
    fn cleanup(&self) {}
}
```

### 3.2 Scheduler

O scheduler usa 5 níveis de prioridade com Round-Robin:

| Nível | Quantum | Uso |
|---|---|---|
| Critical | 2ms | IRQ handlers, drivers |
| High | 5ms | Serviços do sistema |
| Normal | 10ms | Apps do usuário |
| Low | 20ms | Background |
| Idle | ∞ | Processo idle (hlt loop) |

### 3.3 Sistema de Segurança

```
Nível          Permissões
─────────────────────────────────────────────
TrustLevel::Kernel    → Acesso total
TrustLevel::System    → Todos os recursos
TrustLevel::User      → FS, rede, IPC
TrustLevel::Untrusted → Mínimo absoluto
```

---

## 4. API de Syscalls

### Syscalls de Arquivo

```c
int  open(const char *path, int flags, int mode);
int  close(int fd);
int  read(int fd, void *buf, size_t len);
int  write(int fd, const void *buf, size_t len);
int  stat(const char *path, struct stat *buf);
int  unlink(const char *path);
int  mkdir(const char *path, int mode);
int  readdir(int fd, struct dirent *buf, size_t len);
```

### Syscalls de Processo

```c
void exit(int code);
int  getpid(void);
int  sleep(uint64_t ms);
void yield(void);
```

### Syscalls SOC-D Específicas

```c
// P2P
int  p2p_send(uint8_t node_id[32], void *data, size_t len);
int  p2p_recv(void *buf, size_t len);

// IA
int  ia_infer(int model_id, void *input, size_t in_len,
              void *output, size_t out_len);

// Edge Computing
uint64_t edge_submit(int task_kind, void *payload, size_t len);
int      edge_result(uint64_t job_id, void *buf, size_t len);

// WASM
int      wasm_load(const char *name, void *binary, size_t len);
int      wasm_call(int module_id, const char *func,
                   void *args, size_t args_len);

// OpenXR
int  xr_begin_frame(struct XrFrameState *frame);
int  xr_end_frame(struct XrFrameState *frame);

// Quântico
uint64_t quantum_submit(void *circuit, size_t len, uint32_t shots);
int      quantum_result(uint64_t job_id, void *buf, size_t len);

// UI
uint64_t ui_create_surface(const char *title,
                            int x, int y, uint32_t w, uint32_t h);
int      ui_destroy_surface(uint64_t surface_id);

// Segurança
int  security_check(const char *permission);
int  get_stats(int kind, void *buf, size_t len);
```

### Números de Syscall

```
Nr   Nome                  Categoria
─────────────────────────────────────
0    sys_open              FS
1    sys_close             FS
2    sys_read              FS
3    sys_write             FS
4    sys_stat              FS
10   sys_exit              Processo
13   sys_getpid            Processo
14   sys_sleep             Processo
15   sys_yield             Processo
30   sys_socket            Rede
32   sys_connect           Rede
35   sys_send              Rede
36   sys_recv              Rede
100  sys_p2p_send          SOC-D P2P
101  sys_p2p_recv          SOC-D P2P
102  sys_ia_infer          SOC-D IA
103  sys_edge_submit       SOC-D Edge
104  sys_edge_result       SOC-D Edge
105  sys_wasm_load         SOC-D WASM
106  sys_wasm_call         SOC-D WASM
107  sys_xr_begin_frame    SOC-D XR
108  sys_xr_end_frame      SOC-D XR
109  sys_quantum_submit    SOC-D Quantum
110  sys_quantum_result    SOC-D Quantum
111  sys_ui_create_surface SOC-D UI
113  sys_ui_destroy_surface SOC-D UI
114  sys_security_check    SOC-D Sec
115  sys_get_stats         SOC-D Diag
```

---

## 5. API P2P

### Enviar dados para um peer

```rust
// Encontra o melhor peer disponível
let peers = p2p::peer::get_active_peers();
if let Some(peer) = peers.first() {
    // Criptografa e envia
    let data = b"Hello from SOC-D!";
    p2p::transport::send(peer.node_id, data.to_vec());
}
```

### Verificar peers online

```rust
let (known, active) = p2p::peer::count_peers();
println!("{} peers conhecidos, {} ativos", known, active);
```

### Escutar eventos Gossip

```rust
// O Gossip propaga automaticamente via timer IRQ
// Implementar handler customizado via KernelModule
```

---

## 6. API de IA

### Coletar métricas e inferir

```rust
// Coletar amostra manual
ia::collector::collect(current_tick);

// Obter features recentes
let features = ia::collector::get_recent_features(10);

// Rodar todos os modelos
let results = ia::model::run_inference(&features);

for result in &results {
    match &result.output {
        ModelOutput::ResourceForecast { cpu_next_1s, .. } => {
            println!("CPU em 1s: {:.0}%", cpu_next_1s * 100.0);
        }
        ModelOutput::AnomalyScore { score, reason, .. } => {
            if *score > 0.7 {
                println!("ALERTA: {} (score={:.2})", reason, score);
            }
        }
        _ => {}
    }
}
```

### Sugestões geradas automaticamente

```rust
// O ciclo ia::tick() gera sugestões a cada ~60s
let suggestions = ia::suggest::get_suggestions();
for s in &suggestions {
    println!("[{}] {} — {}", s.id, s.title, s.description);
}
```

---

## 7. API Edge Computing

### Submeter uma tarefa

```rust
use edge::task::TaskKind;

let payload = b"dados para processar".to_vec();
let job_id = edge::submit_task(payload, TaskKind::DataProcessing);
println!("Job #{} submetido", job_id);

// Processar fila (chamado automaticamente pelo timer)
edge::tick(current_tick);

// Verificar resultado
if let Some(result) = edge::collector::get_result(job_id) {
    println!("Resultado: {} bytes", result.data.len());
}
```

### Ver capacidade dos nós

```rust
let nodes = edge::node::get_all();
for node in &nodes {
    let score = node.score_for_task(&TaskKind::MLInference);
    println!("{}: score={:.2} load={}%",
        node.name, score, node.profile.current_load_pct);
}
```

---

## 8. API WASM

### Carregar e executar módulo WASM

```rust
// Bytecode WASM — em produção, lido do TmpFS
let wasm_bytes: &[u8] = include_bytes!("my_app.wasm");

// Parseia o módulo
let module = wasm::load("my-app", wasm_bytes)?;

// Cria instância
use wasm::instance::WasmInstance;
let mut instance = WasmInstance::instantiate(module)?;

// Chama função exportada
let result = instance.call_export("add", &[
    WasmValue::I32(10),
    WasmValue::I32(32),
])?;
// result[0] == WasmValue::I32(42)
```

### Host API disponível para módulos WASM

```wat
;; Importações disponíveis no módulo WASM
(import "socd" "log"         (func $log (param i32 i32)))
(import "socd" "random_i32"  (func $random (result i32)))
(import "socd" "uptime_ms"   (func $uptime (result i64)))
(import "socd" "heap_free_kb" (func $heap_free (result i32)))
(import "socd" "peers_active" (func $peers (result i32)))
```

---

## 9. API OpenXR

### Frame loop básico

```rust
use xr::{begin_frame, end_frame};

loop {
    let frame = begin_frame(current_tick);

    if frame.should_render {
        // Renderiza para olho esquerdo
        render_eye(&frame.views[0]);
        // Renderiza para olho direito
        render_eye(&frame.views[1]);
    }

    // Input dos controllers
    let trigger = frame.right_controller.trigger;
    let grip    = frame.left_controller.grip;

    end_frame(&frame);
}
```

### Posição do HMD

```rust
let frame = xr::begin_frame(tick);
let pos = frame.hmd_pose.position;
let rot = frame.hmd_pose.orientation;
println!("HMD: ({:.2}, {:.2}, {:.2}) yaw={:.1}°",
    pos.x, pos.y, pos.z,
    rot.w.acos() * 2.0 * 57.296);
```

---

## 10. API Quântica

### Criar e executar circuito

```rust
use quantum::{QuantumCircuit, submit_circuit, QUANTUM};

// Circuito Bell State (emaranhamento quântico)
let mut circuit = QuantumCircuit::new("Bell", 2);
circuit
    .h(0)        // Hadamard no qubit 0 → superposição
    .cnot(0, 1)  // CNOT: emaranha qubits 0 e 1
    .measure_all(); // Mede ambos

let job_id = submit_circuit(circuit, 1024);

// Processa (simulação local)
QUANTUM.lock().process_jobs(0);

// Obtém resultado
let q = QUANTUM.lock();
if let Some(counts) = q.get_result(job_id) {
    for (state, count) in counts {
        println!("|{}⟩ = {} ({:.1}%)",
            state, count,
            *count as f32 / 1024.0 * 100.0);
    }
    // Saída esperada: ~50% |00⟩, ~50% |11⟩
}
```

### Gates disponíveis

```rust
// Single-qubit
circuit.add(QuantumGate::H  { qubit: 0 });
circuit.add(QuantumGate::X  { qubit: 1 });
circuit.add(QuantumGate::Rz { qubit: 0, theta: PI / 4.0 });

// Two-qubit
circuit.add(QuantumGate::CNOT { control: 0, target: 1 });
circuit.add(QuantumGate::CZ   { control: 0, target: 1 });
circuit.add(QuantumGate::SWAP { qubit_a: 0, qubit_b: 1 });

// Three-qubit
circuit.add(QuantumGate::CCX { c1: 0, c2: 1, target: 2 }); // Toffoli
```

---

## 11. Build & Deploy

### Pré-requisitos

```bash
# Rust nightly + componentes
rustup toolchain install nightly
rustup component add rust-src llvm-tools-preview rustfmt clippy
rustup target add x86_64-unknown-none aarch64-unknown-none

# QEMU para emulação
sudo apt install qemu-system-x86 qemu-system-aarch64  # Ubuntu
brew install qemu                                       # macOS

# Bootimage para criar imagem bootável
cargo install bootimage
```

### Comandos Make

```bash
make setup        # Instala todas as dependências
make build        # Compila (x86_64, debug)
make build-release # Compila otimizado
make run          # Compila + roda no QEMU
make debug        # Roda com GDB (porta 1234)
make gdb          # Conecta GDB ao QEMU
make test         # Executa testes no QEMU
make check        # Verifica sem compilar completo
make fmt          # Formata código
make lint         # Clippy
make clean        # Limpa artefatos
make size         # Exibe tamanho do kernel
```

### Build para Raspberry Pi 4

```bash
# Instala toolchain ARM
rustup target add aarch64-unknown-none
sudo apt install gcc-aarch64-linux-gnu

# Compila para AArch64
cargo build --target aarch64-unknown-none

# Testa no QEMU (máquina virt)
qemu-system-aarch64 \
  -machine virt -cpu cortex-a57 -m 256M \
  -serial stdio -display none \
  -kernel target/aarch64-unknown-none/debug/socd-kernel
```

### Shell de Debug (ao iniciar no QEMU)

```
> help          Lista todos os comandos
> status        Estado dos módulos
> mem           Uso de memória
> sandbox       Segurança
> ps            Processos
> sched         Scheduler
> ls [path]     Listar TmpFS
> cat <path>    Ver arquivo
> modules       Módulos ELF
> p2p           Rede P2P
> peers         Lista de peers
> ia            Motor de IA
> suggest       Sugestões da IA
> edge          Edge computing
> wasm          Runtime WASM
> xr            OpenXR AR/VR
> quantum       Motor quântico
> ui            Interface gráfica
> arm           Info CPU ARM
> net           Stack de rede
> syscall       Syscalls
> version       Versão do kernel
> clear         Limpa tela
> reboot        Reinicia
```

---

## 12. Desenvolvimento de Módulos

### Estrutura de um Módulo ELF

```rust
// my_module/src/lib.rs
#![no_std]
extern crate alloc;

use alloc::string::String;

// Ponto de entrada do módulo (exportado)
#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    // Usa host API via imports
    unsafe {
        socd_log(b"Modulo iniciado!\0".as_ptr(), 16);
    }
    0 // Sucesso
}

#[no_mangle]
pub extern "C" fn module_cleanup() {
    unsafe { socd_log(b"Modulo encerrado.\0".as_ptr(), 18); }
}

// Imports da host API SOC-D
extern "C" {
    fn socd_log(ptr: *const u8, len: usize);
    fn socd_random_i32() -> i32;
    fn socd_peers_active() -> i32;
}
```

### Compilar módulo para WASM

```bash
# Cargo.toml do módulo
[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "z"
lto = true

# Compilar
cargo build --target wasm32-unknown-unknown --release

# Binário em:
# target/wasm32-unknown-unknown/release/my_module.wasm
```

### Carregar módulo no SOC-D

```rust
// No kernel ou via syscall
let wasm = include_bytes!("my_module.wasm");
let module = wasm::load("my_module", wasm)?;
let mut instance = wasm::instance::WasmInstance::instantiate(module)?;
instance.call_export("module_init", &[])?;
```

---

## Licença

Apache 2.0 — veja LICENSE

## Contribuindo

1. Fork o repositório
2. Crie uma branch: `git checkout -b feature/meu-modulo`
3. Implemente com testes: `make test`
4. Formate: `make fmt && make lint`
5. Pull request com descrição detalhada

---

*"Um sistema que não apenas executa — ele colabora, aprende e distribui inteligência."*
