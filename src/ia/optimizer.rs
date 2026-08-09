extern crate alloc;
// ============================================================
// SOC-D — Otimizador de Sistema Baseado em IA
// ============================================================
// Aplica ações de otimização com base nas predições do modelo.
// ============================================================

use spinning_top::Spinlock;
use super::predictor::Prediction;

#[derive(Debug, Clone)]
pub enum OptimizationAction {
    /// Ajusta quantum do scheduler para processos idle
    AdjustSchedulerQuantum { idle_boost: bool },
    /// Inicia sincronização P2P
    TriggerSync { priority: super::model::SyncPriority },
    /// Isola processo com alto score de anomalia
    IsolateProcess { pid: u64 },
    /// Libera memória de processos suspensos
    TrimMemory,
    /// Nenhuma ação necessária
    NoAction,
}

pub struct Optimizer {
    pub actions_applied: u64,
    pub last_action: OptimizationAction,
}

impl Optimizer {
    const fn new() -> Self {
        Self {
            actions_applied: 0,
            last_action: OptimizationAction::NoAction,
        }
    }

    pub fn apply(&mut self, pred: &Prediction, tick: u64) {
        let action = self.decide(pred);

        match &action {
            OptimizationAction::TriggerSync { priority } => {
                // Fase 3: chamar p2p::sync::trigger()
                // Log silenciado — demasiado verboso no terminal
            }
            OptimizationAction::AdjustSchedulerQuantum { idle_boost } => {
                if *idle_boost {
                    // Log silenciado
                }
            }
            OptimizationAction::IsolateProcess { pid } => {
                crate::serial_println!(
                    "[IA][OPT] tick={} ALERTA: Isolando PID {} (anomalia={:.2})",
                    tick, pid, pred.anomaly_score
                );
                // Fase 3: chamar security::sandbox::isolate(pid)
            }
            OptimizationAction::TrimMemory => {
                crate::serial_println!(
                    "[IA][OPT] tick={} Memoria alta ({:.0}%) — limpando processos suspensos",
                    tick, pred.mem_forecast_1s * 100.0
                );
            }
            OptimizationAction::NoAction => {}
        }

        if !matches!(action, OptimizationAction::NoAction) {
            self.actions_applied += 1;
        }
        self.last_action = action;
    }

    fn decide(&self, pred: &Prediction) -> OptimizationAction {
        // Anomalia crítica → isolar processo
        if pred.anomaly_score > 0.8 {
            return OptimizationAction::IsolateProcess { pid: 0 }; // Fase 3: PID real
        }

        // Memória alta → liberar
        if pred.mem_forecast_1s > 0.85 {
            return OptimizationAction::TrimMemory;
        }

        // Sincronização P2P necessária
        if pred.should_sync {
            return OptimizationAction::TriggerSync {
                priority: pred.sync_priority.clone(),
            };
        }

        // CPU ociosa → boost interatividade
        if pred.cpu_forecast_1s < 0.15 {
            return OptimizationAction::AdjustSchedulerQuantum { idle_boost: true };
        }

        OptimizationAction::NoAction
    }
}

static OPTIMIZER: Spinlock<Optimizer> = Spinlock::new(Optimizer::new());

pub fn init() {
    crate::serial_println!("[IA][OPT] Otimizador de sistema ativo");
}

pub fn apply(pred: &Prediction, tick: u64) {
    OPTIMIZER.lock().apply(pred, tick);
}

pub fn actions_applied() -> u64 {
    OPTIMIZER.lock().actions_applied
}
