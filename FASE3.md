# SOC-D Kernel — Fase 3: Interface Gráfica + ARM

> **Status:** Em desenvolvimento ativo  
> **Base:** Fase 1 (Kernel) + Fase 2 (P2P + IA) + Fase 3 (UI + ARM)

---

## O que foi adicionado na Fase 3

### 🖥️ Stack Gráfico (`src/ui/`)

| Arquivo | Descrição |
|---|---|
| `mod.rs` | Raiz do módulo, Color ARGB, Rect, Point, paleta SOC-D |
| `render.rs` | Framebuffer 1024×768 32bpp, fonte bitmap 8×8, primitivas 2D |
| `compositor.rs` | Compositor Wayland-inspired, superfícies, z-ordering, alpha blending |
| `shell.rs` | Desktop shell: wallpaper gradiente, taskbar, monitor do sistema |
| `widgets.rs` | Engine de widgets: Label, Button, ProgressBar, TextInput, Panel |
| `input.rs` | Input unificado: teclado, mouse, touch, gestos AR |

**Primitivas de renderização:**
- `fill_rect` — retângulo sólido
- `draw_rect_border` — borda de retângulo
- `draw_line` — linha (Bresenham)
- `draw_circle` / `fill_circle` — círculo (Midpoint)
- `draw_text` / `draw_text_scaled` — texto bitmap escalonável
- `gradient_rect_h` — gradiente horizontal

**Compositor:**
- 6 layers: Background → Desktop → Windows → Floating → Overlay → Cursor
- Alpha blending por superfície (0–255)
- Hit testing (surface_at)
- Decorações automáticas de janela (título, botões, bordas)

### 🦾 Suporte ARM (`src/arch/arm/`)

| Arquivo | Descrição |
|---|---|
| `mod.rs` | Info da CPU (MIDR_EL1), targets RPi4/QEMU, build configs |
| `exception.rs` | Tabela de vetores de exceção, ExceptionClass, handlers |
| `gic.rs` | GIC-400/GIC-v2 completo: GICD + GICC, IRQ enable/disable, EOI, SGI |
| `mmu.rs` | VMSA AArch64: tabelas de 4 níveis, MAIR, TCR, enable_mmu, TLB flush |

---

## Comandos novos no shell

```
ui      — estado do compositor e framebuffer
arm     — info da CPU ARM e targets de build
```

---

## Compilar para ARM (Raspberry Pi 4)

```bash
# Instala toolchain ARM
rustup target add aarch64-unknown-none
sudo apt install gcc-aarch64-linux-gnu qemu-system-aarch64

# Compila para AArch64
cargo build --target aarch64-unknown-none

# Roda no QEMU virt
qemu-system-aarch64 \
  -machine virt \
  -cpu cortex-a57 \
  -m 256M \
  -serial stdio \
  -display none \
  -kernel target/aarch64-unknown-none/debug/socd-kernel
```

---

## Roadmap restante

### ✅ Fase 1 — Kernel Base
- [x] Microkernel, GDT/IDT, memória, módulos, segurança, drivers

### ✅ Fase 2 — P2P + IA
- [x] P2P: node, peers, discovery mDNS, crypto E2E, gossip, routing
- [x] IA: collector, 3 modelos, predictor, optimizer, suggest

### ✅ Fase 3 — UI + ARM
- [x] Framebuffer render backend
- [x] Compositor Wayland-inspired
- [x] Desktop shell com monitor do sistema
- [x] Engine de widgets
- [x] Input unificado
- [x] ARM AArch64: exception vectors, GIC, MMU
- [ ] Integração virtio-net (driver de rede real)
- [ ] Vulkan backend (GPU acceleration)

### 🔲 Fase 4 — Edge + Quantum
- [ ] Edge computing entre dispositivos
- [ ] WASM runtime na userspace
- [ ] API quântica (IBM/Azure Quantum)
- [ ] OpenXR para AR/VR
