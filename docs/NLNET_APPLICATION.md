# Candidatura NLnet NGI Zero — SOC-D Kernel
## Next Generation Internet — Commons Fund / Core Fund

---

## 1. Nome do Projecto

**SOC-D — Sistema Operacional Cognitivo Distribuído**
*(Distributed Cognitive Operating System)*

---

## 2. Resumo (Abstract — máx. 500 palavras)

SOC-D is a bare-metal operating system kernel written in Rust that integrates
distributed peer-to-peer infrastructure, cognitive AI, and privacy-first
architecture at the kernel level — not as applications running on top of a
conventional OS, but as fundamental components of the system itself.

The project addresses a critical problem in the current technology landscape:
users are forced to rely on centralized cloud providers (Google, Apple, Amazon)
for basic computing functions like file synchronization, device coordination,
and data backup. This creates systemic dependencies that undermine digital
sovereignty and privacy.

SOC-D proposes a different model: each user's devices (phone, laptop, desktop,
AR glasses) form a self-organizing P2P cluster that handles data distribution,
synchronization, and computation without requiring external servers. A
cryptographically-signed DAG (Directed Acyclic Graph) provides tamper-evident
versioning of all user data, synchronized across devices via a gossip protocol.

The cognitive engine learns user patterns and automates routine tasks — not
through cloud-based profiling, but through local inference running directly in
the kernel. Security is handled by a defensive AI subsystem that detects
anomalous process behavior using heuristics and automatically quarantines
suspicious processes.

**Current state:** 90+ Rust source files, ~22,500 lines of no_std kernel code,
compiles and boots in QEMU, includes 48 automated tests, interactive shell with
tab-completion, a real virtio-net PCI driver with genuine DMA (descriptor
rings, not simulated buffers), a real virtio-blk driver providing disk
persistence, real preemptive multitasking (stack-switching scheduler), real
Ed25519/SHA-256/HMAC cryptography, and a real (if simplified) TCP/UDP network
stack — details in Section 6.

---

## 3. Problema que resolve

### Centralização Digital

Os sistemas operativos actuais forçam os utilizadores a depender de
infraestrutura controlada por empresas privadas:

- **Sincronização** — Google Drive, iCloud, OneDrive
- **Identidade** — Apple ID, Google Account, Microsoft Account
- **Computação** — AWS, Azure, GCP
- **IA** — GPT (OpenAI/Microsoft), Gemini (Google), Claude (Anthropic)

Isto cria:
1. **Dependência estrutural** — os dados do utilizador ficam reféns do fornecedor
2. **Vigilância** — os dados passam por servidores de terceiros
3. **Custo** — subscriptions mensais para funcionalidades básicas
4. **Fragmentação** — cada dispositivo usa um ecossistema diferente

### A Alternativa

SOC-D coloca a infraestrutura no próprio sistema operativo:
- P2P entre dispositivos do utilizador — sem servidor central
- DAG criptográfico local — versionamento sem cloud
- IA no kernel — inferência local, sem envio de dados para fora
- Identidade baseada em criptografia de chave pública

---

## 4. Solução Técnica

### Arquitectura

```
Camada de Aplicação    → Shell interativo, WASM apps
Camada Cognitiva       → Motor cognitivo, knowledge graph
Camada P2P             → DAG distribuído, gossip, crypto E2E
Camada de Segurança    → IA defensiva, sandbox, TrustChain
Kernel Core            → Scheduler, memoria, syscall, drivers
```

### Componentes Principais

**DAG Distribuído com Criptografia**
- Cada bloco de dados é assinado com Ed25519 real (via `ed25519-dalek`,
  backend `fiat` puro-Rust — sem SIMD, compatível com o alvo bare-metal)
- Resolução de conflitos CRDT (Last-Write-Wins determinístico)
- Blocos com assinatura inválida rejeitados na camada de rede

**Rede Real**
- Driver virtio-net PCI com DMA genuína: descriptor table, avail ring e
  used ring geridos directamente, sem camada de simulação por baixo
- Transporte P2P sobre UDP real (Ethernet + IPv4 + UDP construídos e
  transmitidos byte a byte)
- Descoberta de nós via mDNS real (RFC 6762 — codificação/descodificação
  DNS própria, sem dependências externas)
- Sockets TCP/UDP genéricos com handshake TCP real (SYN/SYN-ACK/ACK) —
  ver limitações na Secção 6

**Motor Cognitivo**
- Detecção de padrões comportamentais do utilizador
- Knowledge graph de relações entre apps, devices e ficheiros
- Acções automáticas com cooldown e aprovação explícita do utilizador
- Memória episódica local — sem envio de dados para servidores externos

**IA Defensiva**
- 7 heurísticas de detecção de comportamento anómalo
- Score de ameaça 0–100 com resposta escalonada
- Quarentena automática de processos suspeitos
- Políticas de privacidade dinâmicas (Open/Balanced/Private/Lockdown)

**Interface Cross-Device**
- Handoff de sessão entre dispositivos (continuar uma tarefa no telemóvel)
- Clipboard distribuído via DAG
- Interface holográfica AR com spatial anchors persistidos

---

## 5. Alinhamento com NGI

O SOC-D alinha-se directamente com os objectivos NGI:

| Objectivo NGI | Como o SOC-D contribui |
|---------------|------------------------|
| Internet descentralizada | P2P sem servidores centrais, UDP real |
| Privacidade por design | IA e dados locais, DAG criptográfico |
| Soberania digital | Utilizador controla os seus dados |
| Open source | MIT License, código público |
| Inovação técnica | Kernel com IA cognitiva e stack de rede próprios |
| Interoperabilidade | WASM, POSIX syscalls, TCP/UDP, mDNS (RFC 6762) |

---

## 6. Estado Actual e Roadmap

### Completado

Estes componentes correm de facto — construídos byte a byte, testados em
QEMU, sem camada de simulação por baixo:

- Kernel bare-metal x86_64 funcional em QEMU, ~22 500 linhas Rust `no_std`
- **Scheduler preemptivo real**: troca de contexto via stack-switching
  (técnica xv6), com trampolim de arranque e um scheduler
  round-robin corrigido
- **Criptografia real**: SHA-256, HMAC-SHA256, Ed25519 (crates auditadas,
  backend puro-Rust sem SIMD para compatibilidade bare-metal)
- **Driver de rede real**: virtio-net PCI com DMA genuína (virtqueues
  legacy 0.9 completas — descriptor table, avail/used ring)
- **Driver de disco real**: virtio-blk PCI, usado para persistência do
  sistema de ficheiros (snapshot em disco, sobrevive a reinícios)
- **P2P sobre UDP real**: transporte, descoberta mDNS (RFC 6762), e
  sockets TCP/UDP genéricos com handshake real
- **IA com inferência real**: 3 modelos MLP (redes neuronais pequenas
  com multiplicação de matrizes e activações reais — ReLU/sigmoid),
  substituindo heurísticas hardcoded
- 48 testes automatizados, shell interactivo com tab-completion

### Limitações actuais (honestidade técnica)

Para um avaliador que vá ler o código, estas são as limitações conhecidas
e não escondidas:

- **Um único core**: o scheduler e os locks assumem execução single-core;
  SMP exigiria uma revisão significativa da sincronização
- **TCP simplificado**: handshake e transferência de dados funcionam,
  mas sem retransmissão, controlo de congestão, ou reordenação de
  pacotes fora de ordem
- **Nunca testado entre dois dispositivos físicos**: toda a validação de
  rede (P2P, mDNS, sockets) foi feita num único QEMU — falta confirmar
  interoperabilidade real entre máquinas
- **Sem driver WiFi**: só virtio-net (adequado para QEMU/VMs, não para
  hardware físico de consumo)
- **Subsistemas ainda decorativos**: o simulador quântico, o runtime XR,
  e partes da UI gráfica existem mas não têm hardware real por trás —
  não fazem parte do valor central do projecto (soberania de dados e
  rede P2P) e não é onde o financiamento seria direccionado

### Com Financiamento NGI (12 meses)

**Meses 1–4: Validação Física e Robustez de Rede**
- Testar P2P/mDNS/sockets entre 2+ dispositivos físicos reais (não só QEMU)
- Retransmissão e controlo de congestão no TCP
- Driver WiFi básico (802.11, cartão comum)

**Meses 5–8: Multi-Core (SMP)**
- Arranque de APs (Application Processors) via ACPI/MADT
- Scheduler e locks seguros para múltiplos cores
- Per-CPU data structures onde necessário

**Meses 9–12: Utilizabilidade**
- Interface gráfica básica para utilizador não técnico
- Packaging como ISO bootável
- Port para Raspberry Pi (ARM64)
- Documentação completa

---

## 7. Equipa

**Desenvolvedor Principal**
- Linguagem: Rust (sistemas, bare-metal)
- Áreas: kernel development, P2P, criptografia aplicada
- Projecto: SOC-D (autor e arquitecto principal)

**Nota sobre metodologia**: partes significativas da implementação das
fases descritas na Secção 6 ("Completado") foram desenvolvidas com
assistência de um LLM (Claude, Anthropic) em modo de par-programação —
o desenvolvedor definiu a arquitectura, tomou as decisões de scope, e
validou cada fase em QEMU antes de avançar; o LLM ajudou a escrever e
depurar o código Rust/asm de baixo nível. Achamos importante divulgar
isto com transparência.

**Procura de colaboradores** nas áreas de:
- Network engineering (WiFi drivers, testes multi-dispositivo)
- Kernel/SMP engineering (multi-core, sincronização)
- UX/Design (interface para utilizador final)

---

## 8. Orçamento Estimado (12 meses)

| Item | Valor |
|------|-------|
| Desenvolvimento principal (part-time) | 24 000€ |
| Hardware de teste (3+ dispositivos físicos, cartões WiFi) | 1 500€ |
| Infraestrutura CI/CD | 600€ |
| Documentação e tradução | 1 000€ |
| Conferências / disseminação | 900€ |
| **Total** | **28 000€** |

---

## 9. Links

- Repositório: https://github.com/ribeiroleal574-droid/socd-kernel
- Licença: MIT
- Linguagem: Rust (nightly)
- Target: x86_64-unknown-none (bare metal)

---

## 10. Contacto

Email: ribeiroleal574@gmail.com
Contacto: 9817252
GitHub: ribeiroleal574-droid
País: São Tomé e Príncipe
