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

Current state: 84 Rust source files, ~21,000 lines of no_std kernel code,
compiles and boots in QEMU, includes 46 automated tests, interactive shell with
tab-completion, real virtio-net PCI driver, and full subsystem implementations
for P2P, IA, UI, AR, Edge Computing, WASM, Quantum simulation, and cross-device
synchronization.

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
- Cada bloco de dados é assinado com HMAC da chave Ed25519 do nó
- Resolução de conflitos CRDT (Last-Write-Wins determinístico)
- Blocos com assinatura inválida rejeitados na camada de rede
- Sincronização via protocolo Gossip entre dispositivos do utilizador

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
| Internet descentralizada | P2P sem servidores centrais |
| Privacidade por design | IA e dados locais, DAG criptográfico |
| Soberania digital | Utilizador controla os seus dados |
| Open source | MIT License, código público |
| Inovação técnica | Primeiro OS com IA cognitiva no núcleo |
| Interoperabilidade | WASM, POSIX syscalls, múltiplos runtimes |

---

## 6. Estado Actual e Roadmap

### Completado (7 fases)
- Kernel bare-metal x86_64 funcional em QEMU
- 84 ficheiros Rust, ~21 000 linhas no_std
- P2P gossip + crypto + DAG + assinaturas
- Motor cognitivo com padrões e automação
- IA defensiva com quarentena automática
- Interface AR holográfica (OpenXR)
- Driver virtio-net PCI real
- 46 testes automatizados

### Com Financiamento NGI (12 meses)

**Meses 1–4: Rede Real**
- Transport UDP real para DAG (multicast local)
- Sincronização entre 2 dispositivos físicos na mesma rede
- Driver WiFi básico

**Meses 5–8: IA Real**
- Substituir modelos ML simulados por inferência real (TinyML/ONNX)
- Motor cognitivo com aprendizagem efectiva de padrões
- Modelos privados — treino local, sem cloud

**Meses 9–12: Utilizabilidade**
- Interface gráfica básica para utilizador não técnico
- Packaging como ISO bootável
- Documentação completa
- Port para Raspberry Pi (ARM64)

---

## 7. Equipa

**Desenvolvedor Principal**
- Linguagem: Rust (sistemas, bare-metal)
- Áreas: kernel development, P2P, criptografia aplicada
- Projecto: SOC-D (autor e arquitecto principal)

**Procura de colaboradores** nas áreas de:
- Network engineering (WiFi drivers, UDP real)
- ML/AI engineering (modelos de inferência local)
- UX/Design (interface para utilizador final)

---

## 8. Orçamento Estimado (12 meses)

| Item | Valor |
|------|-------|
| Desenvolvimento principal (part-time) | 24 000€ |
| Hardware de teste (3 dispositivos) | 1 500€ |
| Infraestrutura CI/CD | 600€ |
| Documentação e tradução | 1 000€ |
| Conferências / disseminação | 900€ |
| **Total** | **28 000€** |

---

## 9. Links

- Repositório: https://github.com/SEU_USER/socd-kernel
- Licença: MIT
- Linguagem: Rust (nightly)
- Target: x86_64-unknown-none (bare metal)

---

## 10. Contacto

[Preencher com dados reais antes de submeter]

Email: [email]
GitHub: [user]
País: Portugal
