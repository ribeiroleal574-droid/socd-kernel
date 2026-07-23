# SOC-D Kernel — Guia de Instalação Completo
# Passo a Passo para Ubuntu 24.04 / Debian / macOS / Windows (WSL2)

# ══════════════════════════════════════════════════════════════
# ÍNDICE
# ══════════════════════════════════════════════════════════════
# PARTE 1 — Preparar o ambiente
# PARTE 2 — Instalar Rust nightly
# PARTE 3 — Instalar QEMU (emulador)
# PARTE 4 — Baixar e compilar o SOC-D
# PARTE 5 — Rodar no QEMU
# PARTE 6 — Debug com GDB
# PARTE 7 — Rodar no hardware ARM (Raspberry Pi 4)
# PARTE 8 — Solução de problemas
# ══════════════════════════════════════════════════════════════


# ══════════════════════════════════════════════════════════════
# PARTE 1 — Preparar o ambiente
# ══════════════════════════════════════════════════════════════

# ----- Ubuntu 24.04 / Debian -----
sudo apt update && sudo apt upgrade -y
sudo apt install -y \
    build-essential \
    curl \
    git \
    wget \
    unzip \
    pkg-config \
    libssl-dev \
    gcc \
    make \
    python3 \
    python3-pip

# ----- macOS (Homebrew) -----
# /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
# brew install curl git wget make python3

# ----- Windows (WSL2) -----
# Abra o PowerShell como admin e execute:
#   wsl --install
# Depois use Ubuntu dentro do WSL e siga o guia Ubuntu acima.


# ══════════════════════════════════════════════════════════════
# PARTE 2 — Instalar Rust Nightly
# ══════════════════════════════════════════════════════════════

# Passo 2.1: Instalar rustup (gerenciador do Rust)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Quando perguntar, escolha: 1 (Proceed with standard installation)
# Ao terminar, recarregue o shell:
source ~/.bashrc
# ou source ~/.zshrc se usar zsh

# Verificar instalação:
rustc --version
cargo --version

# Passo 2.2: Instalar toolchain nightly
rustup toolchain install nightly
rustup default nightly

# Verificar:
rustup toolchain list

# Passo 2.3: Instalar componentes necessários
rustup component add rust-src           # Código fonte da stdlib (para compilar sem OS)
rustup component add llvm-tools-preview # Ferramentas LLVM (objcopy, strip, etc.)
rustup component add rustfmt            # Formatador de código
rustup component add clippy             # Analisador estático

# Verificar componentes:
rustup component list --installed

# Passo 2.4: Adicionar targets de compilação
rustup target add x86_64-unknown-none   # Target bare metal x86_64
rustup target add aarch64-unknown-none  # Target bare metal ARM64

# Verificar targets:
rustup target list --installed

# Passo 2.5: Instalar bootimage (cria imagem bootável)
cargo install bootimage

# Verificar:
cargo bootimage --version


# ══════════════════════════════════════════════════════════════
# PARTE 3 — Instalar QEMU
# ══════════════════════════════════════════════════════════════

# ----- Ubuntu / Debian -----
sudo apt install -y qemu-system-x86 qemu-system-arm qemu-utils

# Verificar:
qemu-system-x86_64 --version
qemu-system-aarch64 --version

# ----- macOS -----
# brew install qemu

# ----- Fedora / RHEL -----
# sudo dnf install -y qemu-kvm qemu-system-x86 qemu-system-aarch64

# ----- Arch Linux -----
# sudo pacman -S qemu-system-x86 qemu-system-aarch64


# ══════════════════════════════════════════════════════════════
# PARTE 4 — Baixar e Compilar o SOC-D
# ══════════════════════════════════════════════════════════════

# Passo 4.1: Extrair o projeto
# (assumindo que socd-kernel-completo.zip está no seu diretório atual)
unzip socd-kernel-completo.zip
cd socd-kernel

# Verificar estrutura:
ls -la

# Passo 4.2: Verificar o rust-toolchain.toml
cat rust-toolchain.toml
# Deve mostrar: channel = "nightly"

# Passo 4.3: Verificar o .cargo/config.toml
cat .cargo/config.toml
# Deve mostrar: target = "x86_64-unknown-none"

# Passo 4.4: Primeira compilação (debug)
cargo build --target x86_64-unknown-none

# SAÍDA ESPERADA (primeira vez — baixa dependências):
# Updating crates.io index
# Downloading crates ...
# Compiling volatile v0.5.x
# Compiling spinning_top v0.3.x
# ...
# Compiling socd-kernel v0.1.0
# Finished dev [unoptimized + debuginfo]

# Se der erro, veja a PARTE 8 (Solução de Problemas)

# Passo 4.5: Compilação otimizada (release)
cargo build --target x86_64-unknown-none --release

# Verificar binário gerado:
ls -lh target/x86_64-unknown-none/debug/socd-kernel
ls -lh target/x86_64-unknown-none/release/socd-kernel

# Verificar seções do binário:
size target/x86_64-unknown-none/release/socd-kernel

# Passo 4.6: Criar imagem bootável (.img)
cargo bootimage

# A imagem é gerada em:
# target/x86_64-unknown-none/debug/bootimage-socd-kernel.bin


# ══════════════════════════════════════════════════════════════
# PARTE 5 — Rodar no QEMU
# ══════════════════════════════════════════════════════════════

# Passo 5.1: Rodar com Make (modo recomendado)
make run

# Passo 5.2: Rodar manualmente com bootimage
cargo bootimage run -- \
    -serial stdio \
    -display gtk \
    -m 256M \
    -cpu qemu64

# Passo 5.3: Rodar headless (sem janela gráfica — só serial)
cargo bootimage run -- \
    -serial stdio \
    -display none \
    -m 256M

# Passo 5.4: Rodar com mais memória
cargo bootimage run -- \
    -serial stdio \
    -display gtk \
    -m 512M \
    -smp 2

# SAÍDA ESPERADA NO TERMINAL:
# ╔══════════════════════════════════════════════════╗
# ║         SOC-D — Kernel v0.1.0                   ║
# ║  Sistema Operacional Cognitivo Distribuido       ║
# ║  Fase 1: Kernel Base + Seguranca + Modulos       ║
# ╚══════════════════════════════════════════════════╝
#
# [OK] GDT inicializada
# [OK] IDT configurada
# [OK] Interrupções habilitadas
# [OK] Memoria e heap inicializados (1024 KB heap)
# [OK] Registro de modulos ativo
# [OK] Modulos essenciais carregados
# [OK] Sandbox de seguranca ativo
# [OK] TmpFS inicializado
# [OK] Scheduler preemptivo ativo
# [OK] Rede P2P inicializada
# [OK] Motor de IA ativo
# [OK] Interface grafica ativa
# [OK] Edge computing ativo
# [OK] WASM runtime ativo
# [OK] OpenXR AR/VR ativo
# [OK] Motor quantico ativo
# [OK] Stack de rede ativa
# [OK] Syscall interface ativa
#
# [SOC-D] Sistema pronto. Aguardando entrada...
# Digite 'help' para ver os comandos disponiveis.
# >

# Passo 5.5: Usando o shell de debug
# Após iniciar, você verá o prompt "> "
# Digite comandos:

# Listar comandos disponíveis:
# > help

# Ver estado geral:
# > status

# Ver uso de memória:
# > mem

# Listar processos:
# > ps

# Ver rede P2P:
# > p2p
# > peers

# Motor de IA:
# > ia
# > suggest

# Edge computing:
# > edge

# WASM Runtime:
# > wasm

# OpenXR:
# > xr

# Computação quântica (roda Bell State automaticamente!):
# > quantum

# Stack de rede:
# > net

# Interface gráfica:
# > ui

# Suporte ARM:
# > arm

# Listar sistema de arquivos:
# > ls /
# > ls /etc
# > cat /etc/version

# Syscalls:
# > syscall

# Reiniciar:
# > reboot

# Para sair do QEMU: Ctrl+A, depois X


# ══════════════════════════════════════════════════════════════
# PARTE 6 — Debug com GDB
# ══════════════════════════════════════════════════════════════

# Passo 6.1: Instalar GDB com suporte x86_64
sudo apt install -y gdb

# Passo 6.2: Terminal 1 — Iniciar QEMU em modo debug
make debug
# (aguarda conexão do GDB na porta 1234)

# Passo 6.3: Terminal 2 — Conectar GDB
gdb target/x86_64-unknown-none/debug/socd-kernel

# Dentro do GDB:
(gdb) target remote :1234
(gdb) break kernel_main
(gdb) continue
# O kernel para no breakpoint kernel_main
(gdb) info registers
(gdb) backtrace
(gdb) next
(gdb) continue

# Atalhos úteis no GDB:
# b <função>    = breakpoint
# c             = continue
# n             = next (passo)
# s             = step (entra na função)
# p <variável>  = imprimir variável
# x/16x <addr>  = examinar memória
# disas <func>  = disassembly
# quit          = sair

# Passo 6.4: Usar make gdb (atalho)
# Terminal 1:
make debug
# Terminal 2:
make gdb


# ══════════════════════════════════════════════════════════════
# PARTE 7 — Raspberry Pi 4 (AArch64)
# ══════════════════════════════════════════════════════════════

# Passo 7.1: Instalar toolchain ARM no PC
sudo apt install -y gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu

# Passo 7.2: Verificar instalação
aarch64-linux-gnu-gcc --version

# Passo 7.3: Compilar para AArch64
cargo build --target aarch64-unknown-none

# Passo 7.4: Testar no QEMU (máquina virt ARM)
qemu-system-aarch64 \
    -machine virt \
    -cpu cortex-a57 \
    -m 256M \
    -serial stdio \
    -display none \
    -kernel target/aarch64-unknown-none/debug/socd-kernel

# Passo 7.5: Gravar no SD Card para RPi4 (avançado)
# AVISO: Substitua /dev/sdX pelo seu SD card (use lsblk para verificar)

# Formatar SD:
# sudo fdisk /dev/sdX
# (criar partição FAT32 de boot)
# sudo mkfs.fat -F32 /dev/sdX1

# Copiar kernel:
# sudo mount /dev/sdX1 /mnt
# sudo cp target/aarch64-unknown-none/release/socd-kernel /mnt/kernel8.img
# sudo umount /mnt


# ══════════════════════════════════════════════════════════════
# PARTE 8 — Solução de Problemas
# ══════════════════════════════════════════════════════════════

# ── Problema: "error: linker `rust-lld` not found" ───────────
rustup component add llvm-tools-preview
# ou
sudo apt install -y lld

# ── Problema: "error: can't find crate for `std`" ─────────────
rustup component add rust-src
# Confirme que .cargo/config.toml tem:
# [unstable]
# build-std = ["core", "compiler_builtins", "alloc"]

# ── Problema: "cargo bootimage: command not found" ────────────
cargo install bootimage
# Adicione ao PATH se necessário:
export PATH="$HOME/.cargo/bin:$PATH"
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc

# ── Problema: "qemu-system-x86_64: not found" ─────────────────
sudo apt install -y qemu-system-x86
# Verificar:
which qemu-system-x86_64

# ── Problema: QEMU não abre janela gráfica ────────────────────
# Use modo headless (só serial):
cargo bootimage run -- -serial stdio -display none -m 256M

# ── Problema: "Permission denied" ao acessar /dev/kvm ─────────
sudo usermod -aG kvm $USER
# (faça logout e login novamente)
# Ou desabilite KVM:
cargo bootimage run -- -serial stdio -display gtk -m 256M -accel tcg

# ── Problema: Compilação muito lenta ──────────────────────────
# Use mais threads:
cargo build --target x86_64-unknown-none -j$(nproc)

# ── Problema: Erro de linking "multiple definition" ───────────
# Limpe e recompile:
cargo clean
cargo build --target x86_64-unknown-none

# ── Problema: "RUSTFLAGS" conflito ────────────────────────────
unset RUSTFLAGS
cargo build --target x86_64-unknown-none

# ── Verificar tudo está correto ───────────────────────────────
rustc --version        # deve mostrar nightly
cargo --version
rustup target list --installed | grep "none"
rustup component list --installed | grep -E "rust-src|llvm-tools"
which qemu-system-x86_64
cargo bootimage --version


# ══════════════════════════════════════════════════════════════
# REFERÊNCIA RÁPIDA — Comandos do Shell SOC-D
# ══════════════════════════════════════════════════════════════

# help          → lista todos os comandos
# version       → versão do kernel
# status        → estado dos módulos
# mem           → uso de memória
# ps            → lista processos
# sched         → estatísticas do scheduler
# sandbox       → segurança e violações
# ls [path]     → listar diretório (padrão: /)
# cat <path>    → ver conteúdo de arquivo
# modules       → módulos ELF carregados
# p2p           → rede P2P (status, bytes, sessões)
# peers         → lista de peers com trust score
# ia            → motor de IA (inferences, modelos)
# suggest       → sugestões automáticas da IA
# edge          → edge computing (nós, tarefas)
# wasm          → WASM runtime (módulos, instâncias)
# xr            → OpenXR AR/VR (pose do HMD, frames)
# quantum       → motor quântico (Bell State demo)
# net           → stack de rede (IP, MAC, virtio)
# syscall       → interface de syscall (teste)
# ui            → interface gráfica (compositor)
# arm           → info CPU ARM (MIDR, builds)
# clear         → limpa a tela
# reboot        → reinicia o sistema

# Para sair do QEMU: Ctrl+A, depois X
