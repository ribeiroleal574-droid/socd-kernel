# SOC-D — Sistema Operacional Cognitivo Distribuído

> Um novo paradigma de sistema operativo: não apenas executa programas — aprende, adapta e distribui inteligência entre dispositivos.

![Rust](https://img.shields.io/badge/Rust-nightly-orange?logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue)
![Status](https://img.shields.io/badge/status-em%20desenvolvimento-yellow)
![Lines](https://img.shields.io/badge/código-21%20000%2B%20linhas-green)

---

## O Conceito

O SOC-D é um sistema operativo híbrido que une:

- **Kernel modular cognitivo** — aprende padrões de uso e optimiza recursos automaticamente
- **Nuvem pessoal descentralizada** — os teus dispositivos formam um cluster P2P sem depender de Google, Apple ou Amazon
- **Interface imersiva multi-canal** — desktop, mobile, AR/VR holográfico, tudo sincronizado
- **IA integrada ao núcleo** — não como app separada, mas como extensão cognitiva do sistema
- **Segurança autoevolutiva** — detecção de ameaças zero-day por heurística comportamental

```
Tu acordas, colocas os óculos AR →
o sistema já sincronizou a tua agenda,
distribuiu cálculos pesados para o teu PC em casa,
e sugere um briefing visual flutuando à tua frente.
```

---

## Diferencial

| Sistema | Limitação | SOC-D |
|---------|-----------|-------|
| Windows | Pesado, corporativo, fechado | Modular, cognitivo, aberto |
| Linux | Poderoso mas fragmentado | Coeso com IA integrada ao núcleo |
| Android/iOS | Controlado por big tech | Descentralizado, privado, teu |

---

## Estado Actual

| Fase | Descrição | Estado |
|------|-----------|--------|
| Base | Kernel x86_64, GDT, IDT, heap 8MB, scheduler preemptivo | ✅ |
| Fase 1 | P2P gossip/crypto, IA 3 modelos, UI, Edge, WASM, OpenXR, Quantum | ✅ |
| Fase 2 | ELF loader, processos dinâmicos, símbolos kernel | ✅ |
| Fase 3 | DAG distribuído, containers, IA defensiva, cross-device | ✅ |
| Fase 4 | UI mobile adaptativa (6 form factors), interface holográfica AR | ✅ |
| Fase 5 | Motor cognitivo, knowledge graph, automação | ✅ |
| Fase 6 | Monitor recursos, DAG criptográfico, 46 testes, shell avançado | ✅ |
| Fase 7 | Driver virtio-net PCI real, scan barramento PCI | ✅ |
| Fase 8 | Sincronização P2P real entre dispositivos físicos | 🔄 |
| Fase 9 | UI para utilizador final, packaging ISO | 🔄 |

---

## Compilar e Correr

### Pré-requisitos

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup install nightly && rustup default nightly
rustup component add rust-src llvm-tools-preview
cargo install bootimage
sudo apt install qemu-system-x86   # Linux / WSL2
```

### Compilar

```bash
git clone https://github.com/SEU_USER/socd-kernel
cd socd-kernel
cargo bootimage --target x86_64-unknown-none
```

### Correr

```bash
qemu-system-x86_64 \
  -drive format=raw,file=target/x86_64-unknown-none/debug/bootimage-socd-kernel.bin \
  -serial stdio -display none -m 256M \
  -netdev user,id=net0 -device virtio-net-pci,netdev=net0
```

---

## Shell Interativo

```
socd> help          # todos os comandos
socd> test run      # 46 testes automatizados
socd> monitor       # CPU/RAM/rede em tempo real
socd> dag verify    # cadeia de confiança criptográfica
socd> cogn          # motor cognitivo
socd> ar            # cena holográfica AR
socd> pci           # scan PCI bus
socd> threat        # IA defensiva
```

**Atalhos:** `Tab` (auto-complete), `↑↓` (histórico), `←→` (cursor), `Ctrl+R` (pesquisa)

---

## Estrutura

```
src/
├── arch/      kernel/interrupts/GDT/ARM
├── drivers/   VGA/serial/keyboard/shell
├── memory/    heap/paging
├── modules/   scheduler/ELF/containers/monitor/testes
├── security/  sandbox/threat engine/policy
├── net/       virtio-net PCI/TCP-IP/DNS
├── p2p/       gossip/crypto/DAG/assinaturas
├── ia/        modelos ML/motor cognitivo
├── ui/        compositor/mobile/AR holográfico
├── edge/      balancer/edge computing
├── wasm/      runtime WebAssembly
├── xr/        OpenXR AR/VR
├── quantum/   simulador 20 qubits
└── syscall/   32 syscalls POSIX+SOC-D
```

---

## Contribuir

Vê [CONTRIBUTING.md](CONTRIBUTING.md). Áreas prioritárias:
- Transport layer real para DAG (UDP multicast)
- Driver WiFi 802.11
- Modelos ML reais
- UI para utilizador final
- Port ARM64

---

## Licença

MIT — vê [LICENSE](LICENSE)

---

## Financiamento

Candidato a **NLnet NGI Zero** (soberania digital, privacidade, descentralização) e **EU Horizon Europe CL4**.

Se és investigador ou engenheiro interessado em OS research ou sistemas distribuídos — abre uma issue.
