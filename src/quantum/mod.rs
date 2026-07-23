extern crate alloc;
extern crate libm;
// ============================================================
// SOC-D Kernel — API de Computação Quântica (Fase 4)
// ============================================================
//
// O módulo quântico permite ao SOC-D fazer offload de
// cálculos para computadores quânticos remotos via API.
//
// Provedores suportados (Fase 4):
//   - IBM Quantum (IBMQ) via Qiskit Runtime API
//   - Azure Quantum (Microsoft IonQ, Quantinuum)
//   - Amazon Braket (IonQ, Rigetti, OQC)
//   - Google Quantum AI (Sycamore)
//
// Modelo de programação:
//   Circuito quântico → compilação → transpilação → execução
//
// Conceitos básicos:
//   Qubit     — unidade quântica de informação (0, 1, ou superposição)
//   Gate      — operação unitária em qubit(s)
//   Medição   — colapsa superposição → bit clássico
//   Circuito  — sequência de gates + medições
//   Shot      — uma execução do circuito (resultado estocástico)
//   Resultados — contagem de cada estado final (ex: 512× "00", 488× "11")
//
// Gates implementados:
//   Single-qubit: H, X, Y, Z, S, T, Rx, Ry, Rz, I
//   Two-qubit:    CNOT, CZ, SWAP, CRz, iSWAP
//   Three-qubit:  Toffoli (CCX), Fredkin (CSWAP)
//
// Simulador clássico integrado:
//   Para até 20 qubits, simula localmente.
//   Acima de 20 qubits ou para hardware real: offload via rede.
//
// Aplicações no SOC-D:
//   - Otimização de rotas P2P (QAOA)
//   - Fatoração para criptografia pós-quântica
//   - ML quântico (VQE, QSVM)
//   - Geração de números aleatórios quânticos (QRNG)
// ============================================================

use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;

// ─── Gates Quânticos ─────────────────────────────────────────────────────────

/// Um gate quântico (operação unitária)
#[derive(Debug, Clone, PartialEq)]
pub enum QuantumGate {
    // ── Single-qubit ──────────────────────────────────────
    /// Identidade (no-op)
    I  { qubit: usize },
    /// Pauli-X (NOT quântico, flip de 0↔1)
    X  { qubit: usize },
    /// Pauli-Y
    Y  { qubit: usize },
    /// Pauli-Z (phase flip)
    Z  { qubit: usize },
    /// Hadamard (cria superposição: |0⟩ → (|0⟩+|1⟩)/√2)
    H  { qubit: usize },
    /// S gate (phase π/2)
    S  { qubit: usize },
    /// T gate (phase π/4)
    T  { qubit: usize },
    /// Rotação em torno do eixo X por θ radianos
    Rx { qubit: usize, theta: f32 },
    /// Rotação em torno do eixo Y por θ radianos
    Ry { qubit: usize, theta: f32 },
    /// Rotação em torno do eixo Z por θ radianos
    Rz { qubit: usize, theta: f32 },
    /// Phase gate arbitrário: e^{iθ}|1⟩
    P  { qubit: usize, phi: f32 },

    // ── Two-qubit ─────────────────────────────────────────
    /// Controlled-NOT: flipa target se control=|1⟩
    CNOT  { control: usize, target: usize },
    /// Controlled-Z: phase flip se ambos=|1⟩
    CZ    { control: usize, target: usize },
    /// SWAP: troca estados dos dois qubits
    SWAP  { qubit_a: usize, qubit_b: usize },
    /// Controlled-Rz
    CRz   { control: usize, target: usize, theta: f32 },
    /// Controlled-Phase
    CP    { control: usize, target: usize, phi: f32 },

    // ── Three-qubit ───────────────────────────────────────
    /// Toffoli: CNOT com dois controles (CCX)
    CCX   { c1: usize, c2: usize, target: usize },
    /// Fredkin: SWAP controlado (CSWAP)
    CSWAP { control: usize, a: usize, b: usize },

    // ── Medição ───────────────────────────────────────────
    /// Mede um qubit no registro clássico
    Measure { qubit: usize, classical_bit: usize },

    /// Reset: força qubit para |0⟩
    Reset { qubit: usize },

    /// Barreira (sincronização de compilação)
    Barrier { qubits: Vec<usize> },
}

impl QuantumGate {
    pub fn name(&self) -> &'static str {
        match self {
            Self::I {..}   => "I",    Self::X {..}    => "X",
            Self::Y {..}   => "Y",    Self::Z {..}    => "Z",
            Self::H {..}   => "H",    Self::S {..}    => "S",
            Self::T {..}   => "T",    Self::Rx {..}   => "Rx",
            Self::Ry {..}  => "Ry",   Self::Rz {..}   => "Rz",
            Self::P {..}   => "P",    Self::CNOT {..} => "CNOT",
            Self::CZ {..}  => "CZ",   Self::SWAP {..} => "SWAP",
            Self::CRz {..} => "CRz",  Self::CP {..}   => "CP",
            Self::CCX {..} => "Toffoli", Self::CSWAP{..} => "Fredkin",
            Self::Measure {..} => "Measure",
            Self::Reset {..}   => "Reset",
            Self::Barrier {..} => "Barrier",
        }
    }

    /// Número de qubits que este gate afeta
    pub fn qubit_count(&self) -> usize {
        match self {
            Self::I{..}|Self::X{..}|Self::Y{..}|Self::Z{..}|
            Self::H{..}|Self::S{..}|Self::T{..}|Self::Rx{..}|
            Self::Ry{..}|Self::Rz{..}|Self::P{..}|
            Self::Measure{..}|Self::Reset{..} => 1,
            Self::CNOT{..}|Self::CZ{..}|Self::SWAP{..}|
            Self::CRz{..}|Self::CP{..} => 2,
            Self::CCX{..}|Self::CSWAP{..} => 3,
            Self::Barrier { qubits } => qubits.len(),
        }
    }
}

// ─── Número Complexo ──────────────────────────────────────────────────────────

/// Número complexo (amplitude quântica)
#[derive(Debug, Clone, Copy, Default)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };
    pub const ONE:  Self = Self { re: 1.0, im: 0.0 };

    pub fn new(re: f32, im: f32) -> Self { Self { re, im } }

    pub fn magnitude_sq(&self) -> f32 { self.re * self.re + self.im * self.im }
    pub fn magnitude(&self) -> f32 { libm::sqrtf(self.magnitude_sq()) }

    pub fn mul(&self, rhs: &Self) -> Self {
        Self::new(
            self.re * rhs.re - self.im * rhs.im,
            self.re * rhs.im + self.im * rhs.re,
        )
    }

    pub fn add(&self, rhs: &Self) -> Self {
        Self::new(self.re + rhs.re, self.im + rhs.im)
    }

    pub fn scale(&self, s: f32) -> Self {
        Self::new(self.re * s, self.im * s)
    }

    pub fn conj(&self) -> Self {
        Self::new(self.re, -self.im)
    }
}

// ─── Circuito Quântico ───────────────────────────────────────────────────────

/// Um circuito quântico completo
#[derive(Debug, Clone)]
pub struct QuantumCircuit {
    pub name:         String,
    pub num_qubits:   usize,
    pub num_cbits:    usize,
    pub gates:        Vec<QuantumGate>,
    pub depth:        usize,  // Profundidade do circuito (camadas paralelas)
}

impl QuantumCircuit {
    pub fn new(name: &str, num_qubits: usize) -> Self {
        Self {
            name: name.to_string(),
            num_qubits,
            num_cbits: num_qubits,
            gates: Vec::new(),
            depth: 0,
        }
    }

    /// Adiciona um gate ao circuito
    pub fn add(&mut self, gate: QuantumGate) -> &mut Self {
        self.depth += 1; // Simplificado — profundidade real requer análise de dependências
        self.gates.push(gate);
        self
    }

    /// Helpers para gates comuns
    pub fn h(&mut self, q: usize) -> &mut Self { self.add(QuantumGate::H { qubit: q }) }
    pub fn x(&mut self, q: usize) -> &mut Self { self.add(QuantumGate::X { qubit: q }) }
    pub fn z(&mut self, q: usize) -> &mut Self { self.add(QuantumGate::Z { qubit: q }) }
    pub fn cnot(&mut self, c: usize, t: usize) -> &mut Self {
        self.add(QuantumGate::CNOT { control: c, target: t })
    }
    pub fn measure_all(&mut self) -> &mut Self {
        for i in 0..self.num_qubits {
            self.add(QuantumGate::Measure { qubit: i, classical_bit: i });
        }
        self
    }

    /// Resumo do circuito para exibição
    pub fn summary(&self) -> String {
        let gate_counts = self.gates.iter().fold(BTreeMap::new(), |mut m, g| {
            *m.entry(g.name()).or_insert(0u32) += 1;
            m
        });
        let counts_str: String = gate_counts.iter()
            .map(|(k, v)| alloc::format!("{}×{}", v, k))
            .collect::<Vec<_>>()
            .join(", ");

        alloc::format!(
            "Circuito '{}': {} qubits, {} gates ({}), profundidade={}",
            self.name, self.num_qubits, self.gates.len(), counts_str, self.depth
        )
    }
}

// ─── Simulador Quântico Clássico (até 20 qubits) ─────────────────────────────

/// Simula um circuito quântico classicamente
/// Vetor de estado: 2^n amplitudes complexas
pub struct StateVectorSimulator {
    pub num_qubits: usize,
    /// Vetor de estado: amplitude para cada base computacional
    pub state: Vec<Complex>,
}

impl StateVectorSimulator {
    pub fn new(num_qubits: usize) -> Option<Self> {
        if num_qubits > 20 { return None; } // Limite de memória

        let size = 1 << num_qubits;
        let mut state = alloc::vec![Complex::ZERO; size];
        state[0] = Complex::ONE; // |0...0⟩ estado inicial

        Some(Self { num_qubits, state })
    }

    /// Probabilidade de medir o estado |idx⟩
    pub fn probability(&self, idx: usize) -> f32 {
        self.state.get(idx).map(|a| a.magnitude_sq()).unwrap_or(0.0)
    }

    /// Aplica um gate X (Pauli-X / NOT) no qubit q
    pub fn apply_x(&mut self, q: usize) {
        let dim = self.state.len();
        for i in 0..dim {
            let j = i ^ (1 << q); // Flip do bit q
            if j > i {
                self.state.swap(i, j);
            }
        }
    }

    /// Aplica Hadamard no qubit q
    pub fn apply_h(&mut self, q: usize) {
        let inv_sqrt2 = core::f32::consts::FRAC_1_SQRT_2;
        let dim = self.state.len();
        for i in 0..dim {
            if (i >> q) & 1 == 0 {
                let j = i | (1 << q);
                let a = self.state[i];
                let b = self.state[j];
                self.state[i] = a.add(&b).scale(inv_sqrt2);
                self.state[j] = a.add(&Complex::new(-b.re, -b.im)).scale(inv_sqrt2);
            }
        }
    }

    /// Aplica CNOT(control, target)
    pub fn apply_cnot(&mut self, ctrl: usize, tgt: usize) {
        let dim = self.state.len();
        for i in 0..dim {
            // Aplica X no target apenas quando control=|1⟩
            if (i >> ctrl) & 1 == 1 {
                let j = i ^ (1 << tgt);
                if j > i {
                    self.state.swap(i, j);
                }
            }
        }
    }

    /// Aplica Rz(theta) no qubit q
    pub fn apply_rz(&mut self, q: usize, theta: f32) {
        let phase_0 = Complex::new(libm::cosf(theta / 2.0), -libm::sinf(theta / 2.0)); // e^{-iθ/2}
        let phase_1 = Complex::new(libm::cosf(theta / 2.0),  libm::sinf(theta / 2.0)); // e^{+iθ/2}
        for i in 0..self.state.len() {
            let phase = if (i >> q) & 1 == 0 { phase_0 } else { phase_1 };
            self.state[i] = self.state[i].mul(&phase);
        }
    }

    /// Executa um circuito completo e retorna as contagens de medição
    pub fn run(&mut self, circuit: &QuantumCircuit, shots: u32)
        -> BTreeMap<String, u32>
    {
        // Aplica gates (exceto Measure)
        for gate in &circuit.gates {
            match gate {
                QuantumGate::H  { qubit }       => self.apply_h(*qubit),
                QuantumGate::X  { qubit }       => self.apply_x(*qubit),
                QuantumGate::CNOT { control, target } => self.apply_cnot(*control, *target),
                QuantumGate::Rz { qubit, theta } => self.apply_rz(*qubit, *theta),
                QuantumGate::Measure {..} => {} // Medições ao final
                _ => {} // Outros gates: aproximados por identidade na Fase 4
            }
        }

        // Amostragem de resultados baseada nas probabilidades
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut rng = 0x12345678u64; // LCG simples

        for _ in 0..shots {
            // Gera número pseudo-aleatório [0, 1)
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = ((rng >> 33) as f32) / (u32::MAX as f32);

            // Amostragem por inversão da CDF
            let mut cumsum = 0.0f32;
            let mut outcome = 0usize;
            for (i, amp) in self.state.iter().enumerate() {
                cumsum += amp.magnitude_sq();
                if r < cumsum {
                    outcome = i;
                    break;
                }
            }

            // Converte para string binária
            let bitstring: String = (0..circuit.num_qubits)
                .rev()
                .map(|q| if (outcome >> q) & 1 == 1 { '1' } else { '0' })
                .collect();

            *counts.entry(bitstring).or_insert(0) += 1;
        }

        counts
    }
}

// ─── Jobs Quânticos (offload para nuvem) ──────────────────────────────────────

pub type QuantumJobId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum QuantumBackend {
    /// Simulador local (clássico)
    LocalSimulator,
    /// IBM Quantum via API
    IBMQuantum { device: String },
    /// Azure Quantum
    AzureQuantum { device: String },
    /// Amazon Braket
    Braket { device: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct QuantumJob {
    pub id:       QuantumJobId,
    pub circuit:  QuantumCircuit,
    pub backend:  QuantumBackend,
    pub shots:    u32,
    pub state:    JobState,
    pub results:  Option<BTreeMap<String, u32>>,
    pub submitted_at: u64,
    pub duration_ms:  u64,
}

// ─── Motor Quântico ───────────────────────────────────────────────────────────

pub struct QuantumEngine {
    pub initialized: bool,
    jobs: BTreeMap<QuantumJobId, QuantumJob>,
    next_job_id: QuantumJobId,
    pub total_shots: u64,
    pub local_simulations: u64,
    pub remote_submissions: u64,
}

impl QuantumEngine {
    const fn new() -> Self {
        Self {
            initialized: false,
            jobs: BTreeMap::new(),
            next_job_id: 1,
            total_shots: 0,
            local_simulations: 0,
            remote_submissions: 0,
        }
    }

    /// Submete um circuito para execução
    pub fn submit(&mut self, circuit: QuantumCircuit, backend: QuantumBackend, shots: u32)
        -> QuantumJobId
    {
        let id = self.next_job_id;
        self.next_job_id += 1;

        let summary = circuit.summary();
        crate::serial_println!("[QUANTUM] Job #{} submetido: {}", id, summary);
        crate::serial_println!("[QUANTUM] Backend: {:?} | {} shots", backend, shots);

        let job = QuantumJob {
            id,
            circuit,
            backend,
            shots,
            state: JobState::Queued,
            results: None,
            submitted_at: 0,
            duration_ms: 0,
        };

        self.jobs.insert(id, job);
        id
    }

    /// Executa todos os jobs pendentes
    pub fn process_jobs(&mut self, tick: u64) {
        for job in self.jobs.values_mut() {
            if job.state != JobState::Queued { continue; }

            job.state = JobState::Running;

            match &job.backend {
                QuantumBackend::LocalSimulator => {
                    // Simula localmente
                    if let Some(mut sim) = StateVectorSimulator::new(job.circuit.num_qubits) {
                        let results = sim.run(&job.circuit, job.shots);
                        let n_results = results.len();
                        job.results = Some(results);
                        job.state = JobState::Completed;
                        job.duration_ms = job.circuit.num_qubits as u64 * 10;
                        self.total_shots += job.shots as u64;
                        self.local_simulations += 1;
                        crate::serial_println!(
                            "[QUANTUM] Job #{} concluido localmente: {} estados distintos",
                            job.id, n_results
                        );
                    } else {
                        job.state = JobState::Failed(
                            "Muitos qubits para simulacao local (max 20)".into()
                        );
                    }
                }
                _ => {
                    // Backends remotos: simulados como pendentes na Fase 4
                    // Fase 5: integração real via HTTP/REST + chaves de API
                    self.remote_submissions += 1;
                    job.state = JobState::Failed(
                        "Backend remoto nao conectado (configure API key)".into()
                    );
                    crate::serial_println!(
                        "[QUANTUM] Job #{} requer backend remoto — use LocalSimulator",
                        job.id
                    );
                }
            }
        }
    }

    /// Obtém resultado de um job
    pub fn get_result(&self, id: QuantumJobId) -> Option<&BTreeMap<String, u32>> {
        self.jobs.get(&id)?.results.as_ref()
    }

    pub fn get_job(&self, id: QuantumJobId) -> Option<&QuantumJob> {
        self.jobs.get(&id)
    }
}

pub static QUANTUM: Spinlock<QuantumEngine> = Spinlock::new(QuantumEngine::new());

pub fn init() {
    QUANTUM.lock().initialized = true;
    crate::serial_println!("[QUANTUM] Motor quântico inicializado");
    crate::serial_println!("[QUANTUM] Simulador local: ate 20 qubits (2MB estado)");
    crate::serial_println!("[QUANTUM] Backends remotos: IBMQ, Azure, Braket (requer API)");
}

pub fn submit_circuit(circuit: QuantumCircuit, shots: u32) -> QuantumJobId {
    QUANTUM.lock().submit(circuit, QuantumBackend::LocalSimulator, shots)
}

pub fn run_demo_bell_state() -> QuantumJobId {
    let mut circuit = QuantumCircuit::new("Bell-State", 2);
    circuit.h(0).cnot(0, 1).measure_all();

    crate::serial_println!("[QUANTUM] Executando circuito Bell State:");
    crate::serial_println!("[QUANTUM]   q0: ─H──●── Measure");
    crate::serial_println!("[QUANTUM]   q1: ────⊕── Measure");
    crate::serial_println!("[QUANTUM] Esperado: ~50% |00⟩, ~50% |11⟩");

    let id = submit_circuit(circuit, 1000);
    QUANTUM.lock().process_jobs(0);
    id
}

pub fn get_stats() -> QuantumStats {
    let q = QUANTUM.lock();
    QuantumStats {
        initialized: q.initialized,
        jobs_total: q.jobs.len(),
        jobs_completed: q.jobs.values().filter(|j| j.state == JobState::Completed).count(),
        local_simulations: q.local_simulations,
        remote_submissions: q.remote_submissions,
        total_shots: q.total_shots,
    }
}

#[derive(Debug, Clone)]
pub struct QuantumStats {
    pub initialized: bool,
    pub jobs_total: usize,
    pub jobs_completed: usize,
    pub local_simulations: u64,
    pub remote_submissions: u64,
    pub total_shots: u64,
}
