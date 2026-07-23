extern crate alloc;
// ============================================================
// SOC-D — Motor de Sugestões ao Usuário
// ============================================================
// Gera sugestões inteligentes baseadas no contexto do sistema.
// Fase 3: Integrar com interface gráfica via API de notificações
// ============================================================

use alloc::{string::{String, ToString}, vec::Vec};
use spinning_top::Spinlock;

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionKind {
    Performance,
    Storage,
    Security,
    Network,
    Energy,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub id: u64,
    pub kind: SuggestionKind,
    pub title: String,
    pub description: String,
    pub confidence: u8,  // 0–100
    pub tick: u64,
    pub auto_apply: bool,
}

pub struct SuggestEngine {
    pub suggestions: Vec<Suggestion>,
    pub total_generated: u64,
    pub accepted: u64,
    pub dismissed: u64,
    next_id: u64,
}

impl SuggestEngine {
    const fn new() -> Self {
        Self {
            suggestions: Vec::new(),
            total_generated: 0,
            accepted: 0,
            dismissed: 0,
            next_id: 1,
        }
    }

    pub fn evaluate(&mut self, tick: u64) {
        let stats = super::get_stats();

        // Sugestão: Sincronização P2P se há peers ativos há muito tempo
        if tick % 60_000 == 0 {
            let (_, active) = crate::p2p::peer::count_peers();
            if active > 0 {
                self.add(Suggestion {
                    id: self.next_id,
                    kind: SuggestionKind::Network,
                    title: "Sincronizacao disponivel".to_string(),
                    description: alloc::format!(
                        "{} dispositivo(s) online. Sincronizar arquivos recentes?", active
                    ),
                    confidence: 80,
                    tick,
                    auto_apply: false,
                });
            }
        }

        // Sugestão: Memoria alta
        if let Some(sample) = crate::ia::collector::get_latest() {
            let heap_pct = sample.heap_usage_pct;
            if heap_pct > 80 {
                self.add(Suggestion {
                    id: self.next_id,
                    kind: SuggestionKind::Performance,
                    title: "Memoria em uso elevado".to_string(),
                    description: alloc::format!(
                        "Heap em {}%. Considere fechar processos em segundo plano.", heap_pct
                    ),
                    confidence: 90,
                    tick,
                    auto_apply: true,
                });
            }
        }

        // Sugestão: Anomalia de segurança
        if stats.inferences_total > 0 {
            // Fase 3: usar score real do AnomalyDetector
        }
    }

    fn add(&mut self, mut s: Suggestion) {
        s.id = self.next_id;
        self.next_id += 1;
        self.total_generated += 1;

        // Mantém apenas as últimas 10 sugestões ativas
        if self.suggestions.len() >= 10 {
            self.suggestions.remove(0);
        }
        self.suggestions.push(s);
    }

    pub fn list(&self) -> &[Suggestion] {
        &self.suggestions
    }

    pub fn accept(&mut self, id: u64) -> bool {
        if let Some(pos) = self.suggestions.iter().position(|s| s.id == id) {
            self.suggestions.remove(pos);
            self.accepted += 1;
            return true;
        }
        false
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        if let Some(pos) = self.suggestions.iter().position(|s| s.id == id) {
            self.suggestions.remove(pos);
            self.dismissed += 1;
            return true;
        }
        false
    }
}

static SUGGEST: Spinlock<SuggestEngine> = Spinlock::new(SuggestEngine::new());

pub fn init() {
    crate::serial_println!("[IA][SUGGEST] Motor de sugestoes ativo");
}

pub fn evaluate(tick: u64) {
    SUGGEST.lock().evaluate(tick);
}

pub fn get_suggestions() -> Vec<Suggestion> {
    SUGGEST.lock().list().to_vec()
}
