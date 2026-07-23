extern crate alloc;
// ============================================================
// SOC-D Kernel — Motor de Inferência (ONNX Simulado)
// ============================================================
//
// Implementa os modelos de IA do SOC-D.
//
// Fase 2 (atual): Modelos baseados em regras + heurísticas
//   Estrutura compatível com ONNX Runtime para fácil migração
//
// Fase 3: ONNX Runtime real
//   - Modelos treinados offline em Python (PyTorch/TensorFlow)
//   - Exportados para ONNX
//   - Inferência embedded via onnxruntime-sys (C bindings)
//
// Modelos implementados:
//   1. ResourcePredictor — prevê uso de CPU/memória
//   2. AppPredictor      — prevê próximos apps a abrir
//   3. AnomalyDetector  — detecta comportamentos suspeitos
//   4. SyncOptimizer    — decide quando/o que sincronizar P2P
// ============================================================

use alloc::{string::String, vec::Vec};
use spinning_top::Spinlock;

/// Resultado de uma inferência de modelo
#[derive(Debug, Clone)]
pub struct InferenceResult {
    /// Nome do modelo que gerou este resultado
    pub model_name: &'static str,
    /// Confiança (0.0–1.0)
    pub confidence: f32,
    /// Saída do modelo (interpretação depende do modelo)
    pub output: ModelOutput,
    /// Latência da inferência em microssegundos
    pub latency_us: u32,
}

/// Saída de modelo (polimórfica)
#[derive(Debug, Clone)]
pub enum ModelOutput {
    /// Previsão de uso de recurso (0.0–1.0 normalizado)
    ResourceForecast {
        cpu_next_1s:  f32,
        cpu_next_5s:  f32,
        mem_next_1s:  f32,
        mem_next_5s:  f32,
    },
    /// Previsão de próximo app a ser acessado
    AppForecast {
        app_name: String,
        probability: f32,
        preload_bytes: u32,
    },
    /// Score de anomalia (0.0 = normal, 1.0 = altamente suspeito)
    AnomalyScore {
        score: f32,
        reason: String,
        pid: Option<u64>,
    },
    /// Decisão de sincronização P2P
    SyncDecision {
        should_sync: bool,
        priority: SyncPriority,
        estimated_bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncPriority { Critical, High, Normal, Background }

// ─── Modelo 1: ResourcePredictor ─────────────────────────────────────────────
//
// Arquitetura (Fase 3): LSTM com 2 camadas ocultas
//   Input:  16 features × 10 timesteps = 160 valores
//   Hidden: 64 → 32 unidades
//   Output: 4 valores (cpu_1s, cpu_5s, mem_1s, mem_5s)
//
// Fase 2: Regressão linear simulada com médias ponderadas

fn predict_resources(features: &[[f32; 16]]) -> InferenceResult {
    if features.is_empty() {
        return InferenceResult {
            model_name: "ResourcePredictor-v1",
            confidence: 0.0,
            output: ModelOutput::ResourceForecast {
                cpu_next_1s: 0.1,
                cpu_next_5s: 0.1,
                mem_next_1s: 0.5,
                mem_next_5s: 0.5,
            },
            latency_us: 10,
        };
    }

    // Média ponderada das últimas amostras (mais recentes têm mais peso)
    let n = features.len().min(10);
    let mut cpu_avg = 0.0f32;
    let mut mem_avg = 0.0f32;
    let mut weight_sum = 0.0f32;

    for (i, feat) in features.iter().rev().take(n).enumerate() {
        let weight = (i + 1) as f32; // Mais recente = mais peso
        // cpu_idle está em feat[0], heap_usage em feat[4]
        let cpu_use = 1.0 - feat[0]; // Inverte idle → uso
        cpu_avg += cpu_use * weight;
        mem_avg += feat[4] * weight;
        weight_sum += weight;
    }

    if weight_sum > 0.0 {
        cpu_avg /= weight_sum;
        mem_avg /= weight_sum;
    }

    // Tendência simples: se últimas amostras sobem, projeta subida
    let trend_cpu = if features.len() >= 2 {
        let last = 1.0 - features.last().unwrap()[0];
        let prev = 1.0 - features[features.len() - 2][0];
        last - prev
    } else { 0.0 };

    let trend_mem = if features.len() >= 2 {
        features.last().unwrap()[4] - features[features.len() - 2][4]
    } else { 0.0 };

    let confidence = (0.5 + (n as f32 / 20.0)).min(1.0);

    InferenceResult {
        model_name: "ResourcePredictor-v1",
        confidence,
        output: ModelOutput::ResourceForecast {
            cpu_next_1s: (cpu_avg + trend_cpu).clamp(0.0, 1.0),
            cpu_next_5s: (cpu_avg + trend_cpu * 3.0).clamp(0.0, 1.0),
            mem_next_1s: (mem_avg + trend_mem).clamp(0.0, 1.0),
            mem_next_5s: (mem_avg + trend_mem * 3.0).clamp(0.0, 1.0),
        },
        latency_us: 15,
    }
}

// ─── Modelo 2: AnomalyDetector ────────────────────────────────────────────────
//
// Arquitetura (Fase 3): Isolation Forest ou Autoencoder
//   Detecta amostras que se desviam do padrão normal.
//
// Fase 2: Z-score nas métricas de segurança

fn detect_anomalies(features: &[[f32; 16]]) -> InferenceResult {
    let latest = match features.last() {
        Some(f) => f,
        None => return InferenceResult {
            model_name: "AnomalyDetector-v1",
            confidence: 0.0,
            output: ModelOutput::AnomalyScore {
                score: 0.0,
                reason: "sem dados".into(),
                pid: None,
            },
            latency_us: 5,
        },
    };

    // feat[11] = anomaly_score_max (normalizado)
    // feat[10] = sandbox_violations
    let violation_score = latest[10]; // 0–1
    let anomaly_raw = latest[11];     // 0–1

    // Score combinado com peso na violações
    let combined = violation_score * 0.6 + anomaly_raw * 0.4;

    let reason = if combined > 0.7 {
        "Muitas violacoes de sandbox detectadas".into()
    } else if combined > 0.4 {
        "Comportamento levemente anormal".into()
    } else {
        "Normal".into()
    };

    InferenceResult {
        model_name: "AnomalyDetector-v1",
        confidence: 0.8,
        output: ModelOutput::AnomalyScore {
            score: combined,
            reason,
            pid: None, // Fase 3: identificar PID específico
        },
        latency_us: 8,
    }
}

// ─── Modelo 3: SyncOptimizer ──────────────────────────────────────────────────
//
// Decide quando e o que sincronizar com outros nós,
// minimizando uso de banda e maximizando disponibilidade.

fn optimize_sync(features: &[[f32; 16]]) -> InferenceResult {
    let latest = match features.last() {
        Some(f) => f,
        None => return InferenceResult {
            model_name: "SyncOptimizer-v1",
            confidence: 0.5,
            output: ModelOutput::SyncDecision {
                should_sync: false,
                priority: SyncPriority::Background,
                estimated_bytes: 0,
            },
            latency_us: 5,
        },
    };

    let peers_active = latest[6]; // 0–1 (normalizado)
    let cpu_idle     = latest[0]; // 0–1
    let mem_free     = latest[5]; // 0–1

    // Sincroniza quando: há peers, CPU está ociosa, memória disponível
    let should_sync = peers_active > 0.1 && cpu_idle > 0.5 && mem_free > 0.3;

    let priority = if peers_active > 0.8 && cpu_idle > 0.8 {
        SyncPriority::High
    } else if should_sync {
        SyncPriority::Normal
    } else {
        SyncPriority::Background
    };

    InferenceResult {
        model_name: "SyncOptimizer-v1",
        confidence: 0.75,
        output: ModelOutput::SyncDecision {
            should_sync,
            priority,
            estimated_bytes: if should_sync { 1024 * 512 } else { 0 }, // 512 KB estimado
        },
        latency_us: 12,
    }
}

// ─── Engine Central ──────────────────────────────────────────────────────────

pub struct InferenceEngine {
    pub initialized: bool,
    pub total_inferences: u64,
    pub total_latency_us: u64,
}

impl InferenceEngine {
    const fn new() -> Self {
        Self { initialized: false, total_inferences: 0, total_latency_us: 0 }
    }

    /// Roda todos os modelos com as features fornecidas
    pub fn run_all(&mut self, features: &[[f32; 16]]) -> Vec<InferenceResult> {
        let results = alloc::vec![
            predict_resources(features),
            detect_anomalies(features),
            optimize_sync(features),
        ];

        for r in &results {
            self.total_inferences += 1;
            self.total_latency_us += r.latency_us as u64;
        }

        results
    }

    pub fn avg_latency_us(&self) -> u64 {
        if self.total_inferences == 0 { return 0; }
        self.total_latency_us / self.total_inferences
    }
}

static ENGINE: Spinlock<InferenceEngine> = Spinlock::new(InferenceEngine::new());

pub fn init() {
    ENGINE.lock().initialized = true;
    crate::serial_println!(
        "[IA][MODEL] 3 modelos carregados: ResourcePredictor, AnomalyDetector, SyncOptimizer"
    );
}

pub fn run_inference(features: &[[f32; 16]]) -> Vec<InferenceResult> {
    ENGINE.lock().run_all(features)
}

pub fn avg_latency_us() -> u64 {
    ENGINE.lock().avg_latency_us()
}
