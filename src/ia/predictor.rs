extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D — Preditor de Recursos
// ============================================================
// Agrega resultados dos modelos e mantém histórico de previsões
// ============================================================

use spinning_top::Spinlock;

#[derive(Debug, Clone)]
pub struct Prediction {
    pub tick: u64,
    pub cpu_forecast_1s: f32,
    pub cpu_forecast_5s: f32,
    pub mem_forecast_1s: f32,
    pub mem_forecast_5s: f32,
    pub anomaly_score: f32,
    pub should_sync: bool,
    pub sync_priority: super::model::SyncPriority,
    pub confidence: f32,
}

pub struct Predictor {
    pub history: Vec<Prediction>,
    pub total_predictions: u64,
}

impl Predictor {
    const fn new() -> Self {
        Self { history: Vec::new(), total_predictions: 0 }
    }

    pub fn run(&mut self, tick: u64) -> Prediction {
        let features = super::collector::get_recent_features(10);
        let results = super::model::run_inference(&features);

        let mut pred = Prediction {
            tick,
            cpu_forecast_1s: 0.1,
            cpu_forecast_5s: 0.1,
            mem_forecast_1s: 0.5,
            mem_forecast_5s: 0.5,
            anomaly_score: 0.0,
            should_sync: false,
            sync_priority: super::model::SyncPriority::Background,
            confidence: 0.0,
        };

        for result in &results {
            match &result.output {
                super::model::ModelOutput::ResourceForecast {
                    cpu_next_1s, cpu_next_5s, mem_next_1s, mem_next_5s
                } => {
                    pred.cpu_forecast_1s = *cpu_next_1s;
                    pred.cpu_forecast_5s = *cpu_next_5s;
                    pred.mem_forecast_1s = *mem_next_1s;
                    pred.mem_forecast_5s = *mem_next_5s;
                    pred.confidence = result.confidence;
                }
                super::model::ModelOutput::AnomalyScore { score, .. } => {
                    pred.anomaly_score = *score;
                }
                super::model::ModelOutput::SyncDecision { should_sync, priority, .. } => {
                    pred.should_sync = *should_sync;
                    pred.sync_priority = priority.clone();
                }
                _ => {}
            }
        }

        self.total_predictions += 1;
        self.history.push(pred.clone());
        if self.history.len() > 100 {
            self.history.drain(0..10);
        }

        pred
    }
}

static PREDICTOR: Spinlock<Predictor> = Spinlock::new(Predictor::new());

pub fn init() {
    crate::serial_println!("[IA][PRED] Preditor de recursos ativo");
}

pub fn predict(tick: u64) -> Prediction {
    PREDICTOR.lock().run(tick)
}
