# ============================================================
# SOC-D Kernel — Makefile
# ============================================================
# Comandos principais:
#   make setup   — Instala dependências
#   make build   — Compila o kernel
#   make run     — Roda no QEMU
#   make debug   — Roda com GDB
#   make test    — Executa testes
#   make clean   — Limpa artefatos
# ============================================================

KERNEL_NAME = socd-kernel
CARGO       = cargo
QEMU        = qemu-system-x86_64
GDB         = gdb

# Flags do QEMU para execução normal
QEMU_FLAGS  = -serial stdio \
              -display gtk \
              -m 256M \
              -cpu qemu64 \
              -device isa-debug-exit,iobase=0xf4,iosize=0x04

# Flags adicionais para debug com GDB
QEMU_DEBUG_FLAGS = $(QEMU_FLAGS) \
                   -s \
                   -S \
                   -display none

.PHONY: all setup build run debug test clean check fmt lint

all: build

## Instala todas as dependências necessárias
setup:
	@echo "[SOC-D] Configurando ambiente..."
	rustup toolchain install nightly
	rustup component add rust-src llvm-tools-preview --toolchain nightly
	rustup target add x86_64-unknown-none --toolchain nightly
	cargo install bootimage
	@echo "[SOC-D] Ambiente pronto!"
	@echo ""
	@echo "Dependencias do sistema necessarias:"
	@echo "  Ubuntu/Debian: sudo apt install qemu-system-x86 gdb"
	@echo "  Arch:          sudo pacman -S qemu gdb"
	@echo "  macOS:         brew install qemu"

## Compila o kernel
build:
	@echo "[SOC-D] Compilando kernel..."
	$(CARGO) build --target x86_64-unknown-none
	@echo "[SOC-D] Kernel compilado em target/x86_64-unknown-none/debug/$(KERNEL_NAME)"

## Compila em modo release (otimizado)
build-release:
	@echo "[SOC-D] Compilando kernel (release)..."
	$(CARGO) build --target x86_64-unknown-none --release

## Cria imagem bootável e roda no QEMU
run: build
	@echo "[SOC-D] Iniciando no QEMU..."
	$(CARGO) bootimage run -- $(QEMU_FLAGS)

## Roda com debugger GDB conectado
debug: build
	@echo "[SOC-D] Iniciando em modo debug (aguardando GDB na porta 1234)..."
	@echo "Em outro terminal: gdb target/x86_64-unknown-none/debug/socd-kernel"
	@echo "No GDB: target remote :1234"
	$(CARGO) bootimage run -- $(QEMU_DEBUG_FLAGS)

## Conecta GDB ao QEMU rodando
gdb:
	$(GDB) \
		-ex "target remote :1234" \
		-ex "symbol-file target/x86_64-unknown-none/debug/$(KERNEL_NAME)" \
		-ex "break kernel_main"

## Executa testes unitários
test:
	@echo "[SOC-D] Executando testes..."
	$(CARGO) test --target x86_64-unknown-none

## Verifica se compila sem erros (mais rápido que build completo)
check:
	$(CARGO) check --target x86_64-unknown-none

## Formata o código
fmt:
	$(CARGO) fmt

## Análise estática (lints)
lint:
	$(CARGO) clippy --target x86_64-unknown-none

## Limpa artefatos de build
clean:
	$(CARGO) clean
	@echo "[SOC-D] Limpo!"

## Exibe tamanho do kernel
size: build
	@ls -lh target/x86_64-unknown-none/debug/$(KERNEL_NAME) | awk '{print "[SOC-D] Tamanho do kernel: " $$5}'
	@size target/x86_64-unknown-none/debug/$(KERNEL_NAME) 2>/dev/null || true

## Exibe símbolos exportados pelo kernel
symbols: build
	@nm target/x86_64-unknown-none/debug/$(KERNEL_NAME) | sort | head -40

help:
	@echo "SOC-D Kernel — Comandos Make"
	@echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
	@echo "  make setup         Instala dependências"
	@echo "  make build         Compila o kernel"
	@echo "  make build-release Compila otimizado"
	@echo "  make run           Roda no QEMU"
	@echo "  make debug         Debug com GDB"
	@echo "  make test          Executa testes"
	@echo "  make check         Verifica erros"
	@echo "  make fmt           Formata código"
	@echo "  make lint          Análise estática"
	@echo "  make clean         Limpa artefatos"
	@echo "  make size          Tamanho do kernel"
