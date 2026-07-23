# Contribuir para o SOC-D

Obrigado pelo interesse! Este documento explica como contribuir.

---

## Ambiente de Desenvolvimento

```bash
rustup install nightly && rustup default nightly
rustup component add rust-src llvm-tools-preview rustfmt clippy
cargo install bootimage
```

### Verificar antes de submeter

```bash
cargo fmt --all                              # formata o código
cargo clippy --target x86_64-unknown-none   # avisos
cargo bootimage --target x86_64-unknown-none # compila
```

---

## Estrutura de um Contributo

1. **Fork** do repositório
2. **Branch** com nome descritivo: `feat/dag-udp-transport`, `fix/cognitive-overflow`
3. **Commits** pequenos e focados
4. **Pull Request** com descrição do que foi feito e porquê

### Convenções de commit

```
feat: adiciona transport UDP real para DAG
fix: corrige overflow em cognitive tick
docs: actualiza README com Fase 8
test: adiciona T77 para DAG multicast
refactor: simplifica VirtioNetReal::init
```

---

## Áreas Prioritárias

### Alto Impacto

| Área | Ficheiro | Dificuldade |
|------|----------|-------------|
| Transport UDP real para DAG | `src/p2p/dag.rs` | Alta |
| Driver WiFi 802.11 | `src/net/` (novo) | Muito alta |
| Modelos ML reais (treino offline) | `src/ia/model.rs` | Alta |
| Sincronização cross-device real | `src/modules/xdev.rs` | Alta |

### Médio Impacto

| Área | Ficheiro | Dificuldade |
|------|----------|-------------|
| Port ARM64 completo | `src/arch/arm/` | Média |
| Shell com pipes (`cmd1 \| cmd2`) | `src/drivers/serial_shell.rs` | Média |
| TmpFS persistente em disco | `src/modules/tmpfs.rs` | Média |
| Mais testes automatizados | `src/modules/tests.rs` | Baixa |

### Bom para começar

- Corrigir warnings do compilador
- Melhorar mensagens de erro no shell
- Adicionar novos comandos ao shell
- Documentar módulos existentes

---

## Regras

### O que aceitar

- Código que compila com `cargo bootimage --target x86_64-unknown-none`
- Código `no_std` puro (sem dependências std)
- Sem `unsafe` desnecessário — justificar o unsafe existente
- Testes para funcionalidades novas (adicionar a `src/modules/tests.rs`)

### O que não aceitar

- Código que quebra a compilação
- Remover subsistemas existentes sem discussão prévia
- Dependências externas pesadas sem justificação
- Código que causa kernel panic em cenários normais

---

## Ambiente no_std

O SOC-D corre em bare metal — sem biblioteca standard. Regras:

```rust
// CORRECTO: usar alloc
extern crate alloc;
use alloc::{string::String, vec::Vec};

// ERRADO: usar std
use std::collections::HashMap; // nao existe em no_std

// CORRECTO: format! qualificado
let s = alloc::format!("valor: {}", x);

// CORRECTO: sqrt em no_std
let r = libm::sqrtf(x * x + y * y);

// ERRADO: método f32 directo
let r = (x * x + y * y).sqrt(); // nao existe em no_std
```

---

## Testes

Adicionar testes em `src/modules/tests.rs`:

```rust
fn test_meu_modulo() -> TestOutcome {
    // Setup
    let result = meu_modulo::alguma_funcao();

    // Assert
    assert_test!(result.is_ok(), "funcao falhou");
    assert_test!(result.unwrap() == valor_esperado, "valor errado");

    TestOutcome::Pass
}

// Registar no runner:
suite.run("T80 meu_modulo_funcao", test_meu_modulo);
```

---

## Dúvidas

Abre uma **Issue** no GitHub com a label adequada:
- `question` — dúvida técnica
- `bug` — algo não funciona
- `enhancement` — sugestão de melhoria
- `help wanted` — precisas de ajuda para implementar

---

## Código de Conduta

- Respeita todos os contribuidores
- Discussões técnicas são bem-vindas, ataques pessoais não
- Foco no projecto: OS research, privacidade, descentralização
