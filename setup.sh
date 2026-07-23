#!/usr/bin/env bash
# ============================================================
# SOC-D Kernel — Instalador Automático
# Uso: chmod +x setup.sh && ./setup.sh
# ============================================================

set -e  # Para ao primeiro erro

# ─── Cores ───────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

ok()   { echo -e "${GREEN}[OK]${RESET} $1"; }
info() { echo -e "${CYAN}[>>]${RESET} $1"; }
warn() { echo -e "${YELLOW}[!!]${RESET} $1"; }
fail() { echo -e "${RED}[ERRO]${RESET} $1"; exit 1; }
step() { echo -e "\n${BOLD}${CYAN}══ $1 ══${RESET}"; }

# ─── Banner ──────────────────────────────────────────────────
echo -e "${CYAN}"
cat << 'BANNER'
  ╔═══════════════════════════════════════════════════╗
  ║        SOC-D Kernel — Instalador Automático       ║
  ║   Sistema Operacional Cognitivo Distribuído       ║
  ╚═══════════════════════════════════════════════════╝
BANNER
echo -e "${RESET}"

# ─── Detecta OS ──────────────────────────────────────────────
detect_os() {
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if command -v apt &>/dev/null; then
            echo "ubuntu"
        elif command -v dnf &>/dev/null; then
            echo "fedora"
        elif command -v pacman &>/dev/null; then
            echo "arch"
        else
            echo "linux"
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    else
        echo "unknown"
    fi
}

OS=$(detect_os)
info "Sistema detectado: $OS"
info "Arquitetura: $(uname -m)"
info "Kernel: $(uname -r)"
echo ""

# ─── PASSO 1: Dependências do sistema ────────────────────────
step "PASSO 1: Dependências do sistema"

install_deps() {
    case $OS in
        ubuntu)
            info "Instalando pacotes via apt..."
            sudo apt-get update -qq
            sudo apt-get install -y -qq \
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
                qemu-system-x86 \
                qemu-system-arm \
                gdb \
                2>/dev/null || warn "Alguns pacotes falharam (continuando...)"
            ;;
        fedora)
            info "Instalando via dnf..."
            sudo dnf install -y -q \
                gcc make curl git wget \
                qemu-kvm qemu-system-x86 qemu-system-aarch64 \
                gdb openssl-devel \
                2>/dev/null || true
            ;;
        arch)
            info "Instalando via pacman..."
            sudo pacman -Sy --noconfirm --quiet \
                base-devel curl git wget \
                qemu-system-x86 qemu-system-aarch64 \
                gdb \
                2>/dev/null || true
            ;;
        macos)
            if ! command -v brew &>/dev/null; then
                info "Instalando Homebrew..."
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
            fi
            info "Instalando via brew..."
            brew install curl git wget make qemu gdb 2>/dev/null || true
            ;;
        *)
            warn "OS não reconhecido — instale manualmente: curl git make qemu-system-x86_64"
            ;;
    esac
}

install_deps
ok "Dependências do sistema instaladas"

# ─── PASSO 2: Rust Nightly ───────────────────────────────────
step "PASSO 2: Rust Nightly"

if command -v rustup &>/dev/null; then
    RUST_VER=$(rustc --version 2>/dev/null || echo "unknown")
    ok "rustup já instalado: $RUST_VER"
    info "Atualizando toolchain nightly..."
    rustup update nightly 2>/dev/null || true
else
    info "Instalando rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
        sh -s -- -y --default-toolchain nightly --quiet
    # Carrega variáveis de ambiente
    source "$HOME/.cargo/env" 2>/dev/null || \
        export PATH="$HOME/.cargo/bin:$PATH"
    ok "rustup instalado!"
fi

# Garante que PATH inclui cargo
export PATH="$HOME/.cargo/bin:$PATH"

# Define nightly como padrão
rustup default nightly 2>/dev/null || true
ok "Toolchain nightly ativa: $(rustc --version)"

# ─── PASSO 3: Componentes Rust ───────────────────────────────
step "PASSO 3: Componentes Rust"

COMPONENTS=(
    "rust-src"
    "llvm-tools-preview"
    "rustfmt"
    "clippy"
)

for comp in "${COMPONENTS[@]}"; do
    if rustup component list --installed 2>/dev/null | grep -q "$comp"; then
        ok "Componente já instalado: $comp"
    else
        info "Instalando componente: $comp"
        rustup component add "$comp" 2>/dev/null && ok "$comp instalado" || warn "$comp falhou"
    fi
done

# ─── PASSO 4: Targets de compilação ──────────────────────────
step "PASSO 4: Targets bare-metal"

TARGETS=(
    "x86_64-unknown-none"
    "aarch64-unknown-none"
)

for target in "${TARGETS[@]}"; do
    if rustup target list --installed 2>/dev/null | grep -q "$target"; then
        ok "Target já instalado: $target"
    else
        info "Adicionando target: $target"
        rustup target add "$target" 2>/dev/null && ok "$target adicionado" || warn "$target falhou"
    fi
done

# ─── PASSO 5: bootimage ──────────────────────────────────────
step "PASSO 5: bootimage"

if command -v cargo-bootimage &>/dev/null || \
   "$HOME/.cargo/bin/cargo-bootimage" --version &>/dev/null 2>&1; then
    ok "bootimage já instalado"
else
    info "Instalando bootimage..."
    cargo install bootimage 2>/dev/null && ok "bootimage instalado" || \
        warn "bootimage falhou — tente: cargo install bootimage"
fi

# ─── PASSO 6: Verificar QEMU ─────────────────────────────────
step "PASSO 6: Verificar QEMU"

if command -v qemu-system-x86_64 &>/dev/null; then
    QEMU_VER=$(qemu-system-x86_64 --version | head -1)
    ok "QEMU disponível: $QEMU_VER"
else
    warn "qemu-system-x86_64 não encontrado!"
    warn "Ubuntu: sudo apt install qemu-system-x86"
    warn "macOS:  brew install qemu"
fi

# ─── PASSO 7: Compilar o SOC-D ───────────────────────────────
step "PASSO 7: Compilar o SOC-D Kernel"

# Verifica se está no diretório correto
if [ ! -f "Cargo.toml" ]; then
    fail "Execute este script dentro do diretório socd-kernel/
    (onde está o arquivo Cargo.toml)"
fi

info "Compilando para x86_64 (debug)..."
if cargo build --target x86_64-unknown-none 2>&1 | tail -3; then
    ok "Compilação x86_64 (debug) concluída!"
else
    warn "Compilação com avisos — veja erros acima"
fi

info "Compilando para x86_64 (release)..."
if cargo build --target x86_64-unknown-none --release 2>&1 | tail -3; then
    ok "Compilação x86_64 (release) concluída!"

    KERNEL_DEBUG="target/x86_64-unknown-none/debug/socd-kernel"
    KERNEL_RELEASE="target/x86_64-unknown-none/release/socd-kernel"

    if [ -f "$KERNEL_DEBUG" ]; then
        DEBUG_SIZE=$(ls -lh "$KERNEL_DEBUG" | awk '{print $5}')
        ok "Kernel debug: $KERNEL_DEBUG ($DEBUG_SIZE)"
    fi
    if [ -f "$KERNEL_RELEASE" ]; then
        RELEASE_SIZE=$(ls -lh "$KERNEL_RELEASE" | awk '{print $5}')
        ok "Kernel release: $KERNEL_RELEASE ($RELEASE_SIZE)"
    fi
fi

# Cria imagem bootável
info "Criando imagem bootável..."
if cargo bootimage 2>&1 | tail -3; then
    BOOTIMG="target/x86_64-unknown-none/debug/bootimage-socd-kernel.bin"
    if [ -f "$BOOTIMG" ]; then
        IMG_SIZE=$(ls -lh "$BOOTIMG" | awk '{print $5}')
        ok "Imagem bootável criada: $BOOTIMG ($IMG_SIZE)"
    fi
else
    warn "bootimage falhou — kernel compilado, mas sem imagem bootável"
fi

# ─── PASSO 8: Testar compilação ARM ──────────────────────────
step "PASSO 8: Compilar para AArch64 (ARM)"

info "Compilando para aarch64..."
if cargo build --target aarch64-unknown-none 2>&1 | tail -3; then
    ARM_KERNEL="target/aarch64-unknown-none/debug/socd-kernel"
    if [ -f "$ARM_KERNEL" ]; then
        ARM_SIZE=$(ls -lh "$ARM_KERNEL" | awk '{print $5}')
        ok "Kernel ARM64: $ARM_KERNEL ($ARM_SIZE)"
    fi
else
    warn "Compilação ARM64 falhou (opcional, kernel x86_64 já está pronto)"
fi

# ─── PASSO 9: Resumo Final ───────────────────────────────────
step "PASSO 9: Resumo"

echo ""
echo -e "${BOLD}${GREEN}╔══════════════════════════════════════════════════════╗${RESET}"
echo -e "${BOLD}${GREEN}║           SOC-D — Instalação Concluída!              ║${RESET}"
echo -e "${BOLD}${GREEN}╚══════════════════════════════════════════════════════╝${RESET}"
echo ""
echo -e "${BOLD}Versões instaladas:${RESET}"
echo "  Rust:    $(rustc --version 2>/dev/null)"
echo "  Cargo:   $(cargo --version 2>/dev/null)"
echo "  QEMU:    $(qemu-system-x86_64 --version 2>/dev/null | head -1 || echo 'não encontrado')"
echo ""
echo -e "${BOLD}Próximos passos:${RESET}"
echo ""
echo -e "  ${CYAN}1. Rodar no QEMU (modo recomendado):${RESET}"
echo "     make run"
echo ""
echo -e "  ${CYAN}2. Rodar manualmente:${RESET}"
echo "     cargo bootimage run -- -serial stdio -display gtk -m 256M"
echo ""
echo -e "  ${CYAN}3. Rodar headless (só serial, sem janela):${RESET}"
echo "     cargo bootimage run -- -serial stdio -display none -m 256M"
echo ""
echo -e "  ${CYAN}4. Debug com GDB:${RESET}"
echo "     Terminal 1: make debug"
echo "     Terminal 2: make gdb"
echo ""
echo -e "  ${CYAN}5. Executar testes:${RESET}"
echo "     make test"
echo ""
echo -e "${BOLD}No shell do SOC-D, digite:${RESET}"
echo "  help     → lista todos os 22 comandos"
echo "  quantum  → demo Bell State (emaranhamento quântico)"
echo "  ia       → motor de IA em tempo real"
echo "  p2p      → rede descentralizada"
echo ""
echo -e "${YELLOW}Para sair do QEMU: Ctrl+A, depois X${RESET}"
echo ""
