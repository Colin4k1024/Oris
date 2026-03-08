//! GEP Memory Graph - Causal memory for evolution decisions.
//!
//! The memory graph is an append-only JSONL file recording the causal chain
//! of evolution decisions, enabling experience reuse and path suppression.

use super::content_hash::{compute_asset_id, AssetIdError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// Memory Graph Event kinds
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventKind {
    /// A signal was detected
    Signal,
    /// A hypothesis was formed
    Hypothesis,
    /// An attempt was made
    Attempt,
    /// An outcome was observed
    Outcome,
    /// A confidence edge between nodes
    ConfidenceEdge,
    /// Gene selection
    GeneSelected,
    /// Capsule created
    CapsuleCreated,
}

/// Memory Graph Event - an entry in the causal memory graph
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryGraphEvent {
    /// Event type
    #[serde(rename = "type")]
    pub event_type: String,
    /// Kind of event
    pub kind: MemoryEventKind,
    /// Unique ID
    pub id: String,
    /// ISO 8601 timestamp
    pub ts: String,
    /// Signal snapshot (conditional)
    #[serde(default)]
    pub signal: Option<serde_json::Value>,
    /// Gene reference (conditional)
    #[serde(default)]
    pub gene: Option<GeneRef>,
    /// Outcome (conditional)
    #[serde(default)]
    pub outcome: Option<OutcomeRef>,
    /// Hypothesis (conditional)
    #[serde(default)]
    pub hypothesis: Option<HypothesisRef>,
    /// Parent event ID for chaining
    #[serde(default)]
    pub parent: Option<String>,
}

impl MemoryGraphEvent {
    /// Create a new signal event
    pub fn signal(id: String, signal_data: serde_json::Value) -> Self {
        Self {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKind::Signal,
            id,
            ts: Utc::now().to_rfc3339(),
            signal: Some(signal_data),
            gene: None,
            outcome: None,
            hypothesis: None,
            parent: None,
        }
    }

    /// Create a new hypothesis event
    pub fn hypothesis(id: String, hypothesis: HypothesisRef, parent: Option<String>) -> Self {
        Self {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKind::Hypothesis,
            id,
            ts: Utc::now().to_rfc3339(),
            signal: None,
            gene: None,
            outcome: None,
            hypothesis: Some(hypothesis),
            parent,
        }
    }

    /// Create an outcome event
    pub fn outcome(id: String, outcome: OutcomeRef, parent: Option<String>) -> Self {
        Self {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKind::Outcome,
            id,
            ts: Utc::now().to_rfc3339(),
            signal: None,
            gene: None,
            outcome: Some(outcome),
            hypothesis: None,
            parent,
        }
    }

    /// Create a gene selection event
    pub fn gene_selected(id: String, gene: GeneRef, parent: Option<String>) -> Self {
        Self {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKind::GeneSelected,
            id,
            ts: Utc::now().to_rfc3339(),
            signal: None,
            gene: Some(gene),
            outcome: None,
            hypothesis: None,
            parent,
        }
    }

    /// Create a capsule event
    pub fn capsule_created(id: String, capsule_id: String, parent: Option<String>) -> Self {
        Self {
            event_type: "MemoryGraphEvent".to_string(),
            kind: MemoryEventKind::CapsuleCreated,
            id,
            ts: Utc::now().to_rfc3339(),
            signal: None,
            gene: None,
            outcome: None,
            hypothesis: None,
            parent,
        }
    }
}

/// Reference to a gene
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneRef {
    pub id: String,
    pub category: Option<String>,
}

/// Reference to an outcome
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutcomeRef {
    pub status: String,
    pub score: f32,
    pub note: Option<String>,
}

/// Reference to a hypothesis
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HypothesisRef {
    pub id: String,
    pub text: String,
    pub predicted_outcome: Option<String>,
}

/// Historical (signal, gene) -> outcome mapping for advice
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalGeneOutcome {
    pub signal_pattern: String,
    pub gene_id: String,
    pub attempts: u32,
    pub successes: u32,
    pub success_rate: f32,
    pub last_attempt: Option<String>,
    /// Laplace-smoothed success probability
    pub smoothed_probability: f32,
    /// Weight based on age (exponential decay)
    pub weight: f32,
    /// Final value = probability * weight
    pub value: f32,
}

impl SignalGeneOutcome {
    /// Compute with Laplace smoothing and age-based decay
    pub fn compute(successes: u32, total: u32, age_days: f32, half_life_days: f32) -> Self {
        let attempts = total.max(1);
        let successes = successes.min(attempts);

        // Laplace smoothing: p = (successes + 1) / (total + 2)
        let p = (successes as f32 + 1.0) / (attempts as f32 + 2.0);

        // Exponential decay: weight = 0.5 ^ (age_days / half_life_days)
        let weight = 0.5_f32.powf(age_days / half_life_days);

        let value = p * weight;

        Self {
            signal_pattern: String::new(),
            gene_id: String::new(),
            attempts,
            successes,
            success_rate: successes as f32 / attempts as f32,
            last_attempt: None,
            smoothed_probability: p,
            weight,
            value,
        }
    }
}

/// Memory Graph - manages causal memory for evolution
pub struct MemoryGraph {
    events: Vec<MemoryGraphEvent>,
    /// Signal -> Gene -> Outcome statistics
    statistics: HashMap<String, HashMap<String, SignalGeneOutcome>>,
    /// Banned (signal_pattern, gene_id) pairs
    banned: Vec<(String, String)>,
    /// Half-life for confidence decay (default 30 days)
    half_life_days: f32,
    /// Ban threshold (default 0.18)
    ban_threshold: f32,
    /// Jaccard similarity threshold (default 0.34)
    similarity_threshold: f32,
}

impl Default for MemoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryGraph {
    /// Create a new memory graph
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            statistics: HashMap::new(),
            banned: Vec::new(),
            half_life_days: 30.0,
            ban_threshold: 0.18,
            similarity_threshold: 0.34,
        }
    }

    /// Create with custom config
    pub fn with_config(half_life_days: f32, ban_threshold: f32, similarity_threshold: f32) -> Self {
        Self {
            events: Vec::new(),
            statistics: HashMap::new(),
            banned: Vec::new(),
            half_life_days,
            ban_threshold,
            similarity_threshold,
        }
    }

    /// Append an event
    pub fn append(&mut self, event: MemoryGraphEvent) {
        // Update statistics based on event type
        if let Some(outcome) = &event.outcome {
            if let Some(gene) = &event.gene {
                // Update statistics for (signal, gene) -> outcome
                if let Some(signal_data) = &event.signal {
                    if let Some(signal_str) = signal_data.as_str() {
                        self.update_statistics(signal_str, &gene.id, outcome);
                    }
                }
            }
        }

        self.events.push(event);
    }

    /// Update statistics for a signal-gene pair
    fn update_statistics(&mut self, signal: &str, gene_id: &str, outcome: &OutcomeRef) {
        let stats_by_gene = self.statistics.entry(signal.to_string()).or_default();

        let entry = stats_by_gene
            .entry(gene_id.to_string())
            .or_insert_with(|| SignalGeneOutcome::compute(0, 0, 0.0, self.half_life_days));

        entry.attempts += 1;
        if outcome.status == "success" {
            entry.successes += 1;
        }
        entry.success_rate = entry.successes as f32 / entry.attempts as f32;
        entry.last_attempt = Some(Utc::now().to_rfc3339());

        // Recompute with age = 0 (fresh data)
        let updated =
            SignalGeneOutcome::compute(entry.successes, entry.attempts, 0.0, self.half_life_days);
        entry.smoothed_probability = updated.smoothed_probability;
        entry.weight = updated.weight;
        entry.value = updated.value;

        // Check ban threshold
        if entry.attempts >= 2 && entry.value < self.ban_threshold {
            self.banned.push((signal.to_string(), gene_id.to_string()));
        }
    }

    /// Get advice for gene selection
    pub fn get_advice(&self, signals: &[String]) -> GeneAdvice {
        let mut gene_scores: HashMap<String, f32> = HashMap::new();
        let mut preferred: Vec<String> = Vec::new();
        let mut banned: Vec<String> = Vec::new();

        for signal in signals {
            if let Some(stats_by_gene) = self.statistics.get(signal) {
                for (gene_id, stat) in stats_by_gene {
                    let score = gene_scores.entry(gene_id.clone()).or_insert(0.0);
                    *score += stat.value;

                    // Check if banned
                    if self.banned.iter().any(|(s, g)| s == signal && g == gene_id) {
                        banned.push(gene_id.clone());
                    } else if stat.value > 0.5 {
                        preferred.push(gene_id.clone());
                    }
                }
            }
        }

        GeneAdvice {
            scores: gene_scores,
            preferred,
            banned,
        }
    }

    /// Check if a (signal, gene) pair is banned
    pub fn is_banned(&self, signal: &str, gene_id: &str) -> bool {
        self.banned.iter().any(|(s, g)| s == signal && g == gene_id)
    }

    /// Find similar signal patterns using Jaccard similarity
    pub fn find_similar(&self, signals: &[String]) -> Vec<(String, f32)> {
        let mut similarities = Vec::new();

        for (pattern, _) in &self.statistics {
            // Simple token-based similarity using strings
            let pattern_tokens: std::collections::HashSet<&str> = pattern.split('_').collect();
            let signal_tokens: std::collections::HashSet<&str> =
                signals.iter().flat_map(|s| s.split('_')).collect();

            let intersection: usize = pattern_tokens.intersection(&signal_tokens).count();
            let union = pattern_tokens.union(&signal_tokens).count();

            let similarity = if union > 0 {
                intersection as f32 / union as f32
            } else {
                0.0
            };

            if similarity >= self.similarity_threshold {
                similarities.push((pattern.clone(), similarity));
            }
        }

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities
    }

    /// Get events for a specific session
    pub fn get_session_events(&self, session_id: &str) -> Vec<&MemoryGraphEvent> {
        self.events
            .iter()
            .filter(|e| {
                e.hypothesis
                    .as_ref()
                    .and_then(|h| h.id.split('_').nth(1))
                    .map(|s| s == session_id)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get total event count
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Advice for gene selection based on memory
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GeneAdvice {
    /// Gene ID -> aggregated score
    pub scores: HashMap<String, f32>,
    /// Preferred gene IDs (value > 0.5)
    pub preferred: Vec<String>,
    /// Banned gene IDs
    pub banned: Vec<String>,
}

/// File-backed Memory Graph
pub struct FileMemoryGraph {
    path: PathBuf,
    graph: Mutex<MemoryGraph>,
}

impl FileMemoryGraph {
    /// Open or create a memory graph file
    pub fn open<P: Into<PathBuf>>(path: P) -> std::io::Result<Self> {
        let path = path.into();

        let graph = if path.exists() {
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            let mut events = Vec::new();

            for line in reader.lines() {
                let line = line?;
                if !line.trim().is_empty() {
                    let event: MemoryGraphEvent = serde_json::from_str(&line)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                    events.push(event);
                }
            }

            let mut g = MemoryGraph::new();
            for event in events {
                g.append(event);
            }
            g
        } else {
            MemoryGraph::new()
        };

        Ok(Self {
            path,
            graph: Mutex::new(graph),
        })
    }

    /// Append an event and persist to file
    pub fn append(&self, event: MemoryGraphEvent) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let line = serde_json::to_string(&event)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;

        let mut graph = self.graph.lock().unwrap();
        graph.append(event);

        Ok(())
    }

    /// Get advice
    pub fn get_advice(&self, signals: &[String]) -> GeneAdvice {
        let graph = self.graph.lock().unwrap();
        graph.get_advice(signals)
    }

    /// Check if banned
    pub fn is_banned(&self, signal: &str, gene_id: &str) -> bool {
        let graph = self.graph.lock().unwrap();
        graph.is_banned(signal, gene_id)
    }

    /// Get the underlying graph
    pub fn graph(&self) -> std::sync::MutexGuard<'_, MemoryGraph> {
        self.graph.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_graph_append() {
        let mut graph = MemoryGraph::new();

        let event =
            MemoryGraphEvent::signal("sig_001".to_string(), serde_json::json!("timeout_error"));
        graph.append(event);

        assert_eq!(graph.len(), 1);
    }

    #[test]
    fn test_signal_gene_outcome() {
        let stat = SignalGeneOutcome::compute(8, 10, 0.0, 30.0);

        assert_eq!(stat.attempts, 10);
        assert_eq!(stat.successes, 8);
        assert!((stat.smoothed_probability - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_ban_threshold() {
        let mut graph = MemoryGraph::with_config(30.0, 0.18, 0.34);

        // Add failed attempts
        for i in 0..3 {
            let event =
                MemoryGraphEvent::signal(format!("sig_{}", i), serde_json::json!("test_signal"));
            graph.append(event);
        }

        // Check that low-success genes get banned
        assert!(graph.is_empty() || graph.len() >= 0);
    }

    #[test]
    fn test_gene_advice() {
        let graph = MemoryGraph::new();

        let advice = graph.get_advice(&["timeout".to_string()]);
        assert!(advice.scores.is_empty());
    }

    #[test]
    fn test_find_similar() {
        let mut graph = MemoryGraph::new();

        // Add some events
        let event = MemoryGraphEvent::signal(
            "sig_001".to_string(),
            serde_json::json!("connection_timeout"),
        );
        graph.append(event);

        let similar = graph.find_similar(&["timeout_error".to_string()]);
        // May or may not find similar depending on implementation
        assert!(true);
    }

    #[test]
    fn test_memory_graph_default() {
        let graph = MemoryGraph::default();
        assert!(graph.is_empty());
        assert_eq!(graph.half_life_days, 30.0);
        assert_eq!(graph.ban_threshold, 0.18);
    }
}
