// ============================================================
// SOC-D Kernel — Motor Cognitivo + Automação IA (Fase 5)
// ============================================================
//
// O "sistema nervoso digital" do SOC-D — a IA que aprende,
// decide e age autonomamente em nome do utilizador.
//
// Subsistemas:
//
//   1. PatternEngine   — deteta padrões de uso do utilizador
//   2. KnowledgeGraph  — grafo de relações entre entidades
//   3. AutomationEngine— executa tarefas automaticamente
//   4. CognitiveLoop   — ciclo perceber→raciocinar→agir
//   5. MemoryBank      — memória episódica e semântica
//
// Exemplo de autonomia:
//   "O utilizador abre sempre o editor às 09:00"
//   → IA deteta padrão → pré-carrega editor às 08:55
//   → Notifica via AR: "Editor pronto para si"
//
// Exemplo de raciocínio:
//   CPU > 80% + RAM > 70% + P2P ativo
//   → IA decide → offload de cálculos para edge nodes
//   → Notifica: "Distribuí 3 tarefas para socd-server"
//
// Arquitetura do ciclo cognitivo (60Hz):
//
//   Sensores (métricas do kernel)
//        ↓
//   PatternEngine (reconhece situação)
//        ↓
//   KnowledgeGraph (contextualiza)
//        ↓
//   ReasoningEngine (decide ação)
//        ↓
//   AutomationEngine (executa)
//        ↓
//   MemoryBank (aprende com resultado)
// ============================================================

extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
    collections::BTreeMap,
};
use spinning_top::Spinlock;

// ─── Padrão Comportamental ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct UsagePattern {
    pub id:          u64,
    pub name:        String,
    /// Condições que activam o padrão
    pub triggers:    Vec<PatternTrigger>,
    /// Número de vezes observado
    pub occurrences: u64,
    /// Confiança (0.0–1.0) — aumenta com ocorrências
    pub confidence:  f32,
    /// Última vez que foi observado (tick)
    pub last_seen:   u64,
    /// Acção automática associada
    pub action:      Option<AutoAction>,
    /// Aprovado pelo utilizador para execução automática
    pub approved:    bool,
    /// Cooldown em ticks entre execuções do mesmo padrão (evita spam)
    pub cooldown:    u64,
    /// Tick da última vez que a acção foi executada
    pub last_executed: u64,
}

impl UsagePattern {
    pub fn new(id: u64, name: &str, triggers: Vec<PatternTrigger>) -> Self {
        Self {
            id,
            name: name.to_string(),
            triggers,
            occurrences: 0,
            confidence:  0.0,
            last_seen:   0,
            action:      None,
            approved:    false,
            cooldown:    18000, // 5 minutos a 60Hz por defeito
            last_executed: 0,
        }
    }

    pub fn observe(&mut self, tick: u64) {
        self.occurrences += 1;
        self.last_seen = tick;
        // Confiança aumenta logaritmicamente com ocorrências
        self.confidence = 1.0 - 1.0 / (1.0 + self.occurrences as f32 * 0.1);
    }

    pub fn is_confident(&self) -> bool {
        self.confidence >= 0.7 && self.occurrences >= 3
    }

    /// Verifica se o padrão pode executar (respeita cooldown)
    pub fn can_execute(&self, tick: u64) -> bool {
        tick.saturating_sub(self.last_executed) >= self.cooldown
    }
}

#[derive(Debug, Clone)]
pub enum PatternTrigger {
    /// Tick de um certo intervalo (ex: às 09:00 = tick ~32400*60)
    TimeOfDay { hour_approx: u64 },
    /// CPU acima de threshold %
    CpuHigh { threshold: u8 },
    /// RAM acima de threshold %
    RamHigh { threshold: u8 },
    /// App específica aberta
    AppOpened { name: String },
    /// P2P peers ativos
    PeersActive { min: usize },
    /// DAG com blocos pendentes
    DagPending { min: usize },
    /// Dispositivo específico online
    DeviceOnline { kind: String },
    /// Ameaça detetada
    ThreatDetected,
    /// Sempre verdadeiro (ação periódica)
    Periodic { interval_ticks: u64 },
}

impl PatternTrigger {
    pub fn evaluate(&self, ctx: &CognitiveContext) -> bool {
        match self {
            PatternTrigger::CpuHigh { threshold }  => ctx.cpu_pct >= *threshold,
            PatternTrigger::RamHigh { threshold }  => ctx.ram_pct >= *threshold,
            PatternTrigger::PeersActive { min }    => ctx.peers_active >= *min,
            PatternTrigger::DagPending { min }     => ctx.dag_pending >= *min,
            PatternTrigger::ThreatDetected         => ctx.threats_active > 0,
            PatternTrigger::Periodic { interval_ticks } => {
                ctx.tick % interval_ticks == 0
            }
            PatternTrigger::TimeOfDay { hour_approx } => {
                // Tick aproximado de hora (60Hz * 3600s = 216000 ticks/hora)
                let tick_hour = ctx.tick / 216000;
                tick_hour % 24 == *hour_approx
            }
            PatternTrigger::AppOpened { name } => ctx.open_apps.contains(name),
            PatternTrigger::DeviceOnline { kind } => ctx.online_device_kinds.contains(kind),
        }
    }
}

// ─── Contexto Cognitivo ──────────────────────────────────────
// Snapshot do estado do sistema para raciocínio

#[derive(Debug, Clone, Default)]
pub struct CognitiveContext {
    pub tick:              u64,
    pub cpu_pct:           u8,
    pub ram_pct:           u8,
    pub peers_active:      usize,
    pub dag_pending:       usize,
    pub threats_active:    usize,
    pub edge_nodes_free:   usize,
    pub open_apps:         Vec<String>,
    pub online_device_kinds: Vec<String>,
    pub battery_pct:       Option<u8>,
}

impl CognitiveContext {
    pub fn snapshot(tick: u64) -> Self {
        let sched = crate::modules::scheduler::get_stats();
        let p2p   = crate::p2p::get_stats();
        let dag   = crate::p2p::dag::stats();
        let threat = crate::security::threat::stats();
        let devices = crate::modules::xdev::online_devices();

        Self {
            tick,
            cpu_pct:    crate::modules::monitor::real_cpu_pct(),
            ram_pct:    crate::modules::monitor::real_ram_pct(),
            peers_active: p2p.peers_active,
            dag_pending:  dag.sync_blocks,
            threats_active: threat.quarantined_procs,
            edge_nodes_free: 2, // placeholder
            open_apps:    alloc::vec!["shell".to_string()],
            online_device_kinds: devices.iter()
                .map(|d| d.kind.as_str().to_string()).collect(),
            battery_pct: None,
        }
    }
}

// ─── Acções Automáticas ──────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AutoAction {
    /// Pré-carrega uma app em memória
    PreloadApp { name: String },
    /// Offload de tarefa para edge node
    OffloadToEdge { task: String },
    /// Sync P2P forçado
    ForcePeerSync,
    /// Backup automático via DAG
    BackupToDag { path: String },
    /// Notificação ao utilizador
    Notify { title: String, body: String },
    /// Ajuste de política de privacidade
    AdjustPrivacy { level: String },
    /// Limpeza de processos suspeitos
    IsolateThreats,
    /// Distribuição de carga entre containers
    RebalanceContainers,
    /// Sincronização cross-device
    SyncToDevice { kind: String },
    /// Optimização de memória
    GarbageCollect,
    /// Composto: múltiplas acções
    Sequence(Vec<AutoAction>),
}

impl AutoAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutoAction::PreloadApp{..}        => "preload-app",
            AutoAction::OffloadToEdge{..}     => "offload-edge",
            AutoAction::ForcePeerSync         => "p2p-sync",
            AutoAction::BackupToDag{..}       => "backup-dag",
            AutoAction::Notify{..}            => "notify",
            AutoAction::AdjustPrivacy{..}     => "adjust-privacy",
            AutoAction::IsolateThreats        => "isolate-threats",
            AutoAction::RebalanceContainers   => "rebalance-ct",
            AutoAction::SyncToDevice{..}      => "sync-device",
            AutoAction::GarbageCollect        => "gc",
            AutoAction::Sequence(_)           => "sequence",
        }
    }

    /// Executa a acção no sistema
    pub fn execute(&self, tick: u64) {
        match self {
            AutoAction::PreloadApp { name } => {
                crate::serial_println!("[COGN] Pre-carregando app '{}'", name);
            }
            AutoAction::OffloadToEdge { task } => {
                crate::serial_println!("[COGN] Offload '{}' para edge node", task);
            }
            AutoAction::ForcePeerSync => {
                crate::serial_println!("[COGN] Forcando sync P2P");
                crate::p2p::dag::sync_tick(tick);
            }
            AutoAction::BackupToDag { path } => {
                crate::serial_println!("[COGN] Backup '{}' → DAG", path);
                if let Ok(data) = crate::modules::tmpfs::read(path) {
                    crate::p2p::dag::write(path, data);
                }
            }
            AutoAction::Notify { title, body } => {
                crate::serial_println!("[COGN] Notificacao: {} — {}", title, body);
                crate::ui::ar::show_toast(
                    &alloc::format!("{}: {}", title, body),
                    crate::ui::ar::ToastLevel::Info,
                    180,
                );
            }
            AutoAction::AdjustPrivacy { level } => {
                crate::serial_println!("[COGN] Ajustando privacidade: {}", level);
            }
            AutoAction::IsolateThreats => {
                crate::serial_println!("[COGN] Isolando processos ameaca");
                let tick2 = tick;
                crate::security::threat::threat_tick(tick2);
            }
            AutoAction::RebalanceContainers => {
                crate::serial_println!("[COGN] Rebalancando containers");
            }
            AutoAction::SyncToDevice { kind } => {
                crate::serial_println!("[COGN] Sync cross-device → {}", kind);
            }
            AutoAction::GarbageCollect => {
                crate::serial_println!("[COGN] Optimizando memoria (GC)");
            }
            AutoAction::Sequence(actions) => {
                for a in actions { a.execute(tick); }
            }
        }
    }
}

// ─── Grafo de Conhecimento ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct KnowledgeNode {
    pub id:         u64,
    pub label:      String,
    pub kind:       NodeKind,
    pub properties: BTreeMap<String, String>,
    pub weight:     f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    App, Device, User, File, Service, Event, Concept
}

#[derive(Debug, Clone)]
pub struct KnowledgeEdge {
    pub from:   u64,
    pub to:     u64,
    pub rel:    String,
    pub weight: f32,
}

pub struct KnowledgeGraph {
    nodes:    BTreeMap<u64, KnowledgeNode>,
    edges:    Vec<KnowledgeEdge>,
    next_id:  u64,
}

impl KnowledgeGraph {
    pub const fn new() -> Self {
        Self { nodes: BTreeMap::new(), edges: Vec::new(), next_id: 1 }
    }

    pub fn add_node(&mut self, label: &str, kind: NodeKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(id, KnowledgeNode {
            id, label: label.to_string(), kind,
            properties: BTreeMap::new(), weight: 1.0,
        });
        id
    }

    pub fn add_edge(&mut self, from: u64, to: u64, rel: &str, weight: f32) {
        self.edges.push(KnowledgeEdge {
            from, to, rel: rel.to_string(), weight,
        });
    }

    pub fn strengthen(&mut self, from: u64, to: u64, rel: &str) {
        for e in self.edges.iter_mut() {
            if e.from == from && e.to == to && e.rel == rel {
                e.weight = (e.weight + 0.1).min(1.0);
                return;
            }
        }
        self.add_edge(from, to, rel, 0.1);
    }

    pub fn related(&self, node_id: u64) -> Vec<(&KnowledgeNode, &str, f32)> {
        self.edges.iter()
            .filter(|e| e.from == node_id || e.to == node_id)
            .filter_map(|e| {
                let other_id = if e.from == node_id { e.to } else { e.from };
                self.nodes.get(&other_id).map(|n| (n, e.rel.as_str(), e.weight))
            })
            .collect()
    }

    pub fn node_count(&self) -> usize { self.nodes.len() }
    pub fn edge_count(&self) -> usize { self.edges.len() }
}

// ─── Memória Episódica ───────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Episode {
    pub tick:     u64,
    pub context:  String,
    pub action:   String,
    pub outcome:  EpisodeOutcome,
    pub reward:   f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EpisodeOutcome { Success, Failure, Neutral }

pub struct MemoryBank {
    episodes:    Vec<Episode>,
    /// Resumos semânticos (conceitos aprendidos)
    semantic:    BTreeMap<String, f32>,
    max_episodes: usize,
}

impl MemoryBank {
    pub const fn new() -> Self {
        Self {
            episodes: Vec::new(),
            semantic: BTreeMap::new(),
            max_episodes: 256,
        }
    }

    pub fn record(&mut self, tick: u64, context: &str,
                  action: &str, outcome: EpisodeOutcome, reward: f32) {
        if self.episodes.len() >= self.max_episodes {
            self.episodes.remove(0);
        }
        self.episodes.push(Episode {
            tick,
            context: context.to_string(),
            action:  action.to_string(),
            outcome, reward,
        });
        // Actualiza memória semântica
        let entry = self.semantic.entry(action.to_string()).or_insert(0.0);
        *entry = (*entry + reward) / 2.0;
    }

    pub fn recall_best(&self, context: &str) -> Option<&str> {
        // Retorna a acção com maior recompensa para contextos similares
        self.episodes.iter()
            .filter(|e| e.context.contains(context))
            .max_by(|a, b| a.reward.partial_cmp(&b.reward).unwrap())
            .map(|e| e.action.as_str())
    }

    pub fn episode_count(&self) -> usize { self.episodes.len() }
}

// ─── Motor Cognitivo ─────────────────────────────────────────

pub struct CognitiveEngine {
    pub patterns:   Vec<UsagePattern>,
    pub knowledge:  KnowledgeGraph,
    pub memory:     MemoryBank,
    pub action_log: Vec<(u64, String, String)>, // (tick, pattern, action)
    pub stats:      CognitiveStats,
    last_tick:      u64,
    cycle_interval: u64,
    next_pattern_id: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CognitiveStats {
    pub cycles_run:       u64,
    pub patterns_matched: u64,
    pub actions_executed: u64,
    pub suggestions_made: u64,
    pub episodes_stored:  usize,
}

impl CognitiveEngine {
    pub const fn new() -> Self {
        Self {
            patterns:    Vec::new(),
            knowledge:   KnowledgeGraph::new(),
            memory:      MemoryBank::new(),
            action_log:  Vec::new(),
            stats:       CognitiveStats {
                cycles_run: 0,
                patterns_matched: 0,
                actions_executed: 0,
                suggestions_made: 0,
                episodes_stored: 0,
            },
            last_tick:       0,
            cycle_interval:  60, // 1 segundo a 60Hz
            next_pattern_id: 1,
        }
    }

    pub fn add_pattern(&mut self, name: &str,
                       triggers: Vec<PatternTrigger>,
                       action: AutoAction, approved: bool) -> u64 {
        let id = self.next_pattern_id;
        self.next_pattern_id += 1;
        let mut p = UsagePattern::new(id, name, triggers);
        p.action   = Some(action);
        p.approved = approved;
        crate::serial_println!("[COGN] Padrao registado: '{}' id={} aprovado={}",
            name, id, approved);
        self.patterns.push(p);
        id
    }

    /// Ciclo principal: perceber → raciocinar → agir
    pub fn tick(&mut self, tick: u64) {
        if tick.saturating_sub(self.last_tick) < self.cycle_interval { return; }
        self.last_tick = tick;
        self.stats.cycles_run += 1;

        // 1. Perceber — snapshot do contexto
        let ctx = CognitiveContext::snapshot(tick);

        // 2. Raciocinar — avaliar padrões
        let mut actions_to_run: Vec<(String, AutoAction)> = Vec::new();

        for pattern in self.patterns.iter_mut() {
            let all_match = pattern.triggers.iter()
                .all(|t| t.evaluate(&ctx));

            if all_match {
                pattern.observe(tick);
                self.stats.patterns_matched += 1;

                if pattern.is_confident() && pattern.approved && pattern.can_execute(tick) {
                    if let Some(action) = &pattern.action {
                        actions_to_run.push((pattern.name.clone(), action.clone()));
                        pattern.last_executed = tick;
                    }
                } else if pattern.is_confident() && !pattern.approved && pattern.can_execute(tick) {
                    // Sugere ao utilizador
                    crate::serial_println!(
                        "[COGN] Sugestao: padrao '{}' conf={:.0}% — aprovar com 'cogn approve {}' ",
                        pattern.name,
                        pattern.confidence * 100.0,
                        pattern.id
                    );
                    self.stats.suggestions_made += 1;
                }
            }
        }

        // 3. Agir — executa acções aprovadas
        for (name, action) in actions_to_run {
            crate::serial_println!("[COGN] Executando '{}' → {}", name, action.as_str());
            action.execute(tick);

            // Regista na memória episódica
            self.memory.record(
                tick, &name, action.as_str(),
                EpisodeOutcome::Success, 0.8,
            );
            self.stats.actions_executed += 1;

            if self.action_log.len() > 50 { self.action_log.remove(0); }
            self.action_log.push((tick, name, action.as_str().to_string()));
        }

        self.stats.episodes_stored = self.memory.episode_count();
    }

    pub fn approve_pattern(&mut self, id: u64) -> bool {
        if let Some(p) = self.patterns.iter_mut().find(|p| p.id == id) {
            p.approved = true;
            crate::serial_println!("[COGN] Padrao '{}' aprovado para auto-execucao", p.name);
            true
        } else { false }
    }

    pub fn recent_actions(&self, n: usize) -> Vec<&(u64, String, String)> {
        let start = self.action_log.len().saturating_sub(n);
        self.action_log[start..].iter().collect()
    }
}

// ─── Instância Global ─────────────────────────────────────────

pub static COGNITIVE: Spinlock<CognitiveEngine> =
    Spinlock::new(CognitiveEngine::new());

// ─── API Pública ─────────────────────────────────────────────

pub fn init() {
    let mut engine = COGNITIVE.lock();

    // ── Padrões built-in (pré-aprovados) ─────────────────────

    // 1. CPU alta → offload para edge (cooldown: 10 min)
    let id1 = engine.add_pattern(
        "cpu-overload-offload",
        alloc::vec![PatternTrigger::CpuHigh { threshold: 80 }],
        AutoAction::OffloadToEdge { task: "heavy-compute".to_string() },
        true,
    );
    if let Some(p) = engine.patterns.iter_mut().find(|p| p.id == id1) {
        p.cooldown = 36000; // 10 min
        p.occurrences = 10; p.confidence = 0.9; // pré-confiante
    }

    // 2. Ameaças ativas → isolamento imediato (cooldown: 2 min)
    let id2 = engine.add_pattern(
        "auto-isolate-threats",
        alloc::vec![PatternTrigger::ThreatDetected],
        AutoAction::IsolateThreats,
        true,
    );
    if let Some(p) = engine.patterns.iter_mut().find(|p| p.id == id2) {
        p.cooldown = 7200;
        p.occurrences = 10; p.confidence = 0.9;
    }

    // 3. Sync periódico P2P — trigger já é Periodic, cooldown extra segurança
    let id3 = engine.add_pattern(
        "periodic-p2p-sync",
        alloc::vec![PatternTrigger::Periodic { interval_ticks: 18000 }],
        AutoAction::ForcePeerSync,
        true,
    );
    if let Some(p) = engine.patterns.iter_mut().find(|p| p.id == id3) {
        p.cooldown = 18000; // 5 min
        p.occurrences = 10; p.confidence = 0.9;
    }

    // 4. Mobile online → sync cross-device (cooldown: 30 min — não spama)
    let id4 = engine.add_pattern(
        "mobile-sync-on-connect",
        alloc::vec![PatternTrigger::DeviceOnline { kind: "mobile".to_string() }],
        AutoAction::Sequence(alloc::vec![
            AutoAction::SyncToDevice { kind: "mobile".to_string() },
            AutoAction::Notify {
                title: "Sync".to_string(),
                body: "Sincronizado com mobile".to_string(),
            },
        ]),
        true,
    );
    if let Some(p) = engine.patterns.iter_mut().find(|p| p.id == id4) {
        p.cooldown = 108000; // 30 min — executa no máximo 1 vez por meia hora
        p.occurrences = 10; p.confidence = 0.9;
    }

    // 5. RAM alta → GC + rebalance (necessita aprovação)
    let id5 = engine.add_pattern(
        "ram-pressure-gc",
        alloc::vec![PatternTrigger::RamHigh { threshold: 75 }],
        AutoAction::Sequence(alloc::vec![
            AutoAction::GarbageCollect,
            AutoAction::RebalanceContainers,
        ]),
        false,
    );
    if let Some(p) = engine.patterns.iter_mut().find(|p| p.id == id5) {
        p.cooldown = 36000;
        p.occurrences = 10; p.confidence = 0.9;
    }

    // ── Grafo de Conhecimento inicial ─────────────────────────
    let user   = engine.knowledge.add_node("utilizador",   NodeKind::User);
    let shell  = engine.knowledge.add_node("shell",        NodeKind::App);
    let editor = engine.knowledge.add_node("text-editor",  NodeKind::App);
    let phone  = engine.knowledge.add_node("socd-phone",   NodeKind::Device);
    let server = engine.knowledge.add_node("socd-server",  NodeKind::Device);
    let dag    = engine.knowledge.add_node("dag-sync",     NodeKind::Service);

    engine.knowledge.add_edge(user, shell,  "usa",          0.9);
    engine.knowledge.add_edge(user, editor, "usa",          0.7);
    engine.knowledge.add_edge(user, phone,  "possui",       1.0);
    engine.knowledge.add_edge(user, server, "possui",       0.8);
    engine.knowledge.add_edge(shell, dag,   "usa",          0.6);
    engine.knowledge.add_edge(phone, server,"sincroniza",   0.5);

    crate::serial_println!("[COGN] Motor cognitivo inicializado");
    crate::serial_println!("[COGN] {} padroes | {} nos | {} arestas no knowledge graph",
        engine.patterns.len(),
        engine.knowledge.node_count(),
        engine.knowledge.edge_count());
}

pub fn cognitive_tick(tick: u64) {
    COGNITIVE.lock().tick(tick);
}

pub fn approve(pattern_id: u64) -> bool {
    COGNITIVE.lock().approve_pattern(pattern_id)
}

pub fn stats() -> CognitiveStats {
    COGNITIVE.lock().stats.clone()
}

pub fn run_demo() {
    crate::serial_println!("\n[FASE5] === Motor Cognitivo + Automacao IA ===");

    let tick = crate::modules::scheduler::get_stats().current_tick;

    // Simula múltiplos ciclos cognitivos
    for i in 0..5u64 {
        cognitive_tick(tick + i * 60);
    }

    // Reforça relação no knowledge graph
    {
        let mut e = COGNITIVE.lock();
        e.knowledge.strengthen(1, 3, "usa"); // user → text-editor
        e.knowledge.strengthen(1, 3, "usa");
        e.knowledge.strengthen(1, 3, "usa");
    }

    let s = stats();
    crate::serial_println!("[FASE5] Ciclos: {} | Padroes match: {} | Acoes: {} | Sugestoes: {}",
        s.cycles_run, s.patterns_matched, s.actions_executed, s.suggestions_made);

    // Memória episódica
    {
        let binding = COGNITIVE.lock();
        let best = binding.memory.recall_best("cpu");
        if let Some(action) = best {
            crate::serial_println!("[FASE5] Memoria: melhor acao para 'cpu' = '{}'", action);
        }
    }

    crate::serial_println!("[FASE5] Use 'cogn' no shell para estado cognitivo");
    crate::serial_println!("[FASE5] Use 'cogn approve <id>' para aprovar padroes");
    crate::serial_println!("[FASE5] ==========================================\n");
}
