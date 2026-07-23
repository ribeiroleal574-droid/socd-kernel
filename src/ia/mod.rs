extern crate alloc;
// ============================================================
// SOC-D Kernel — Motor de IA Integrado ao Núcleo
// ============================================================
//
// A IA do SOC-D não é um app separado — vive no kernel.
// Tem acesso direto a métricas do scheduler, memória, P2P
// e toma decisões em tempo real sem latência de IPC.
//
// Componentes:
//   collector  — coleta métricas de uso do sistema
//   model      — modelos de inferência (ONNX simulado)
//   predictor  — prevê próximos apps/recursos a usar
//   optimizer  — otimiza scheduler, memória e P2P
//   suggest    — gera sugestões ao usuário
//
// Pipeline completo:
//   Métricas → Features → Modelo → Predição → Ação
//
// Fase 2 (atual):
//   - Coleta real de métricas do kernel
//   - Modelos simulados (regras + heurísticas)
//   - Base para integrar ONNX Runtime na Fase 3
//
// Fase 3: ONNX Runtime embutido (inference em C++ + bindings Rust)
// ============================================================

pub mod collector;  // Coleta de métricas do sistema
pub mod model;      // Modelos de inferência
pub mod predictor;  // Predição de uso futuro
pub mod optimizer;  // Otimização do sistema
pub mod suggest;    // Sugestões ao usuário
pub mod cognitive;  // Motor cognitivo + automação (Fase 5)

use spinning_top::Spinlock;

/// Estado global do motor de IA
pub static IA_STATE: Spinlock<IaEngineState> =
    Spinlock::new(IaEngineState::new());

pub struct IaEngineState {
    pub initialized: bool,
    pub inferences_total: u64,
    pub optimizations_applied: u64,
    pub suggestions_generated: u64,
    pub last_inference_tick: u64,
    pub model_accuracy: u8, // 0-100%
}

impl IaEngineState {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            inferences_total: 0,
            optimizations_applied: 0,
            suggestions_generated: 0,
            last_inference_tick: 0,
            model_accuracy: 0,
        }
    }
}

/// Inicializa o motor de IA completo
pub fn init() {
    collector::init();
    model::init();
    predictor::init();
    optimizer::init();
    suggest::init();

    let mut state = IA_STATE.lock();
    state.initialized = true;
    state.model_accuracy = 72; // Estimativa inicial

    crate::serial_println!("[IA] Motor de IA inicializado");
    crate::serial_println!("[IA] Modelos: predictor de uso + otimizador de recursos");
}

/// Ciclo principal da IA — chamado periodicamente pelo timer
pub fn tick(current_tick: u64) {
    // Roda a cada 1000 ticks (~1 segundo)
    if current_tick % 1000 != 0 { return; }

    collector::collect(current_tick);
    let prediction = predictor::predict(current_tick);
    optimizer::apply(&prediction, current_tick);
    suggest::evaluate(current_tick);

    let mut state = IA_STATE.lock();
    state.inferences_total += 1;
    state.last_inference_tick = current_tick;
}

/// Estatísticas do motor de IA
pub fn get_stats() -> IaStats {
    let state = IA_STATE.lock();
    let col = collector::get_stats();
    IaStats {
        initialized: state.initialized,
        inferences_total: state.inferences_total,
        optimizations_applied: state.optimizations_applied,
        suggestions_generated: state.suggestions_generated,
        model_accuracy: state.model_accuracy,
        metrics_collected: col.total_samples,
        last_inference_tick: state.last_inference_tick,
    }
}

#[derive(Debug, Clone)]
pub struct IaStats {
    pub initialized: bool,
    pub inferences_total: u64,
    pub optimizations_applied: u64,
    pub suggestions_generated: u64,
    pub model_accuracy: u8,
    pub metrics_collected: u64,
    pub last_inference_tick: u64,
}
