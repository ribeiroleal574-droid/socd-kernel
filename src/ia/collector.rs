extern crate alloc;
use alloc::vec::Vec;
// ============================================================
// SOC-D Kernel — Coletor de Métricas para IA
// ============================================================
//
// Coleta métricas do kernel para alimentar os modelos de IA.
// Todas as métricas são coletadas sem syscalls — acesso direto
// às estruturas de dados do kernel.
//
// Métricas coletadas:
//   CPU: uso por processo, tempo idle, trocas de contexto
//   Memória: heap usado/livre, page faults, alocações
//   P2P: peers ativos, bytes transferidos, latência
//   FS: arquivos acessados, padrões de I/O
//   Segurança: violações, scores de anomalia
//
// Armazenamento: ring buffer circular (últimas N amostras)
// Feature engineering: normalização + janela temporal
// ============================================================

use spinning_top::Spinlock;

/// Uma amostra de métricas do sistema em um instante de tempo
#[derive(Debug, Clone, Default)]
pub struct MetricSample {
    pub tick: u64,

    // CPU
    pub cpu_idle_pct: u8,          // % tempo idle
    pub context_switches: u64,     // Trocas de contexto desde boot
    pub processes_running: u8,     // Processos ativos agora
    pub processes_total: u16,      // Total de processos

    // Memória
    pub heap_used_kb: u32,
    pub heap_free_kb: u32,
    pub heap_usage_pct: u8,

    // P2P
    pub p2p_peers_active: u8,
    pub p2p_bytes_sent_kb: u32,
    pub p2p_bytes_recv_kb: u32,
    pub p2p_latency_avg_us: u32,

    // Segurança
    pub sandbox_violations: u32,
    pub anomaly_score_max: u8,

    // IA (para meta-aprendizado)
    pub ia_last_prediction_confidence: u8,
    pub ia_last_optimization_gain: u8,
}

impl MetricSample {
    /// Coleta métricas reais das estruturas do kernel
    pub fn collect(tick: u64) -> Self {
        // Memória
        let (heap_used, heap_free) = crate::memory::heap::heap_stats();
        let heap_total = crate::memory::heap::HEAP_SIZE;
        let heap_usage_pct = (heap_used * 100 / heap_total.max(1)) as u8;

        // Scheduler
        let sched_stats = crate::modules::scheduler::get_stats();

        // P2P
        let (_p2p_known, p2p_active) = crate::p2p::peer::count_peers();
        let (_tx_pkts, _rx_pkts, tx_bytes, rx_bytes) = crate::p2p::transport::get_stats();

        // Segurança
        let sec_stats = crate::security::sandbox::get_stats();

        Self {
            tick,
            cpu_idle_pct: if sched_stats.running == 0 { 95 } else { 60 },
            context_switches: sched_stats.context_switches,
            processes_running: sched_stats.running as u8,
            processes_total: sched_stats.total_processes as u16,
            heap_used_kb: (heap_used / 1024) as u32,
            heap_free_kb: (heap_free / 1024) as u32,
            heap_usage_pct,
            p2p_peers_active: p2p_active as u8,
            p2p_bytes_sent_kb: (tx_bytes / 1024) as u32,
            p2p_bytes_recv_kb: (rx_bytes / 1024) as u32,
            p2p_latency_avg_us: 500, // Fase 3: medir real
            sandbox_violations: sec_stats.total_violations as u32,
            anomaly_score_max: 0,
            ia_last_prediction_confidence: 0,
            ia_last_optimization_gain: 0,
        }
    }

    /// Converte para vetor de features normalizado [0.0–1.0]
    /// Usado como input dos modelos de ML
    pub fn to_features(&self) -> [f32; 16] {
        [
            self.cpu_idle_pct as f32 / 100.0,
            (self.context_switches % 1000) as f32 / 1000.0,
            self.processes_running as f32 / 32.0,
            self.processes_total as f32 / 256.0,
            self.heap_usage_pct as f32 / 100.0,
            self.heap_free_kb as f32 / 1024.0,
            self.p2p_peers_active as f32 / 16.0,
            self.p2p_bytes_sent_kb as f32 / 1024.0,
            self.p2p_bytes_recv_kb as f32 / 1024.0,
            self.p2p_latency_avg_us as f32 / 100_000.0,
            self.sandbox_violations as f32 / 100.0,
            self.anomaly_score_max as f32 / 100.0,
            self.ia_last_prediction_confidence as f32 / 100.0,
            self.ia_last_optimization_gain as f32 / 100.0,
            (self.tick % 86_400_000) as f32 / 86_400_000.0, // Hora do dia normalizada
            0.0, // Reservado
        ]
    }
}

/// Estatísticas do coletor
#[derive(Debug, Clone)]
pub struct CollectorStats {
    pub total_samples: u64,
    pub buffer_size: usize,
    pub oldest_tick: u64,
    pub newest_tick: u64,
}

/// Ring buffer de amostras (janela temporal para os modelos)
pub struct MetricsCollector {
    /// Buffer circular (últimas 512 amostras)
    buffer: Vec<MetricSample>,
    capacity: usize,
    head: usize,  // Posição de escrita
    count: usize, // Amostras válidas
    total_collected: u64,
}

impl MetricsCollector {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            head: 0,
            count: 0,
            total_collected: 0,
        }
    }

    /// Adiciona uma nova amostra ao buffer circular
    pub fn push(&mut self, sample: MetricSample) {
        if self.buffer.len() < self.capacity {
            self.buffer.push(sample);
        } else {
            self.buffer[self.head] = sample;
        }
        self.head = (self.head + 1) % self.capacity;
        self.count = self.count.min(self.capacity);
        self.count = if self.buffer.len() < self.capacity {
            self.buffer.len()
        } else {
            self.capacity
        };
        self.total_collected += 1;
    }

    /// Retorna as últimas N amostras como features para o modelo
    pub fn recent_features(&self, n: usize) -> Vec<[f32; 16]> {
        let n = n.min(self.count);
        let start = if self.count < self.capacity {
            0
        } else {
            self.head
        };

        (0..n)
            .map(|i| {
                let idx = (start + self.count - n + i) % self.capacity.max(1);
                if idx < self.buffer.len() {
                    self.buffer[idx].to_features()
                } else {
                    [0.0f32; 16]
                }
            })
            .collect()
    }

    /// Última amostra coletada
    pub fn latest(&self) -> Option<&MetricSample> {
        if self.buffer.is_empty() { return None; }
        let idx = if self.head == 0 { self.buffer.len() - 1 } else { self.head - 1 };
        self.buffer.get(idx)
    }

    pub fn stats(&self) -> CollectorStats {
        CollectorStats {
            total_samples: self.total_collected,
            buffer_size: self.count,
            oldest_tick: self.buffer.first().map(|s| s.tick).unwrap_or(0),
            newest_tick: self.buffer.last().map(|s| s.tick).unwrap_or(0),
        }
    }
}

static COLLECTOR: Spinlock<MetricsCollector> =
    Spinlock::new(MetricsCollector {
        buffer: Vec::new(),
        capacity: 512,
        head: 0,
        count: 0,
        total_collected: 0,
    });

pub fn init() {
    crate::serial_println!("[IA][COLLECTOR] Coletor de metricas ativo (buffer=512 amostras)");
}

/// Coleta uma nova amostra de métricas
pub fn collect(tick: u64) {
    let sample = MetricSample::collect(tick);
    COLLECTOR.lock().push(sample);
}

/// Retorna features recentes para inferência
pub fn get_recent_features(n: usize) -> Vec<[f32; 16]> {
    COLLECTOR.lock().recent_features(n)
}

/// Retorna a amostra mais recente
pub fn get_latest() -> Option<MetricSample> {
    COLLECTOR.lock().latest().cloned()
}

pub fn get_stats() -> CollectorStats {
    COLLECTOR.lock().stats()
}
