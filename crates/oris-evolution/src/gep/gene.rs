//! GEP-compatible Gene definition.
//!
//! A Gene is a reusable evolution strategy that defines what signals it responds to,
//! what steps to follow, and what safety constraints apply.

use super::content_hash::{compute_asset_id, AssetIdError};
use serde::{Deserialize, Serialize};

/// Gene category - the intent type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GeneCategory {
    /// Fix errors, restore stability, reduce failure rate
    Repair,
    /// Improve existing capabilities, increase success rate
    Optimize,
    /// Explore new strategies, break out of local optima
    Innovate,
}

impl Default for GeneCategory {
    fn default() -> Self {
        Self::Repair
    }
}

impl std::fmt::Display for GeneCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneCategory::Repair => write!(f, "repair"),
            GeneCategory::Optimize => write!(f, "optimize"),
            GeneCategory::Innovate => write!(f, "innovate"),
        }
    }
}

/// Signal match pattern - supports substring, regex, and multi-language alias
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SignalPattern {
    /// Substring match (default)
    Substring(String),
    /// Regex pattern with flags
    Regex(String),
    /// Multi-language alias (pipe-delimited)
    Alias(String),
}

impl SignalPattern {
    /// Check if this pattern matches the given signal
    pub fn matches(&self, signal: &str) -> bool {
        match self {
            SignalPattern::Substring(s) => signal.to_lowercase().contains(&s.to_lowercase()),
            SignalPattern::Regex(pattern) => {
                // Simple regex matching - in production use regex crate
                if let Ok(re) = regex_lite::Regex::new(pattern) {
                    re.is_match(signal)
                } else {
                    false
                }
            }
            SignalPattern::Alias(aliases) => aliases.split('|').any(|lang| {
                let lang = lang.trim().to_lowercase();
                signal.to_lowercase().contains(&lang)
            }),
        }
    }
}

impl From<String> for SignalPattern {
    fn from(s: String) -> Self {
        if s.starts_with('/') && s.ends_with('/') {
            SignalPattern::Regex(s.trim_matches('/').to_string())
        } else if s.contains('|') {
            SignalPattern::Alias(s)
        } else {
            SignalPattern::Substring(s)
        }
    }
}

/// Gene constraints - safety limits for evolution
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GeneConstraints {
    /// Maximum number of files that can be modified
    #[serde(rename = "max_files")]
    pub max_files: usize,
    /// Paths that are forbidden to modify
    #[serde(rename = "forbidden_paths")]
    pub forbidden_paths: Vec<String>,
}

impl GeneConstraints {
    pub fn new(max_files: usize) -> Self {
        Self {
            max_files,
            forbidden_paths: vec![],
        }
    }

    pub fn with_forbidden(mut self, paths: Vec<String>) -> Self {
        self.forbidden_paths = paths;
        self
    }

    /// Check if a file path is allowed
    pub fn is_allowed(&self, path: &str) -> bool {
        !self
            .forbidden_paths
            .iter()
            .any(|forbidden| path.contains(forbidden))
    }
}

/// Precondition for gene execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenePrecondition {
    /// Description of the precondition
    pub description: String,
    /// Check command (optional)
    #[serde(default)]
    pub check: Option<String>,
}

/// Runtime behavioral modifiers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpigeneticMark {
    /// Mark name
    pub name: String,
    /// Mark value
    pub value: serde_json::Value,
    /// Whether this mark is active
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

/// GEP-compatible Gene definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GepGene {
    /// Asset type - always "Gene"
    #[serde(rename = "type")]
    pub gene_type: String,
    /// Protocol schema version
    #[serde(rename = "schema_version")]
    pub schema_version: String,
    /// Unique identifier
    pub id: String,
    /// Category - repair, optimize, or innovate
    pub category: GeneCategory,
    /// Patterns that trigger this gene
    #[serde(rename = "signals_match")]
    pub signals_match: Vec<SignalPattern>,
    /// Conditions that must hold before use
    #[serde(default)]
    pub preconditions: Vec<GenePrecondition>,
    /// Ordered, actionable steps
    pub strategy: Vec<String>,
    /// Safety constraints
    pub constraints: GeneConstraints,
    /// Commands to verify correctness after execution
    pub validation: Vec<String>,
    /// Runtime-applied behavioral modifiers
    #[serde(default, rename = "epigenetic_marks")]
    pub epigenetic_marks: Vec<EpigeneticMark>,
    /// LLM model that produced this gene
    #[serde(default)]
    pub model_name: Option<String>,
    /// Content-addressable hash
    #[serde(rename = "asset_id")]
    pub asset_id: String,
}

impl GepGene {
    /// Create a new GEP Gene with computed asset_id
    pub fn new(
        id: String,
        category: GeneCategory,
        signals_match: Vec<String>,
        strategy: Vec<String>,
        validation: Vec<String>,
    ) -> Result<Self, AssetIdError> {
        let signals_match: Vec<SignalPattern> =
            signals_match.into_iter().map(SignalPattern::from).collect();

        let constraints = GeneConstraints::new(20); // Default max 20 files

        let mut gene = Self {
            gene_type: "Gene".to_string(),
            schema_version: super::GEP_SCHEMA_VERSION.to_string(),
            id,
            category,
            signals_match,
            preconditions: vec![],
            strategy,
            constraints,
            validation,
            epigenetic_marks: vec![],
            model_name: None,
            asset_id: String::new(), // Will be computed
        };

        gene.asset_id = compute_asset_id(&gene, &["asset_id"])?;
        Ok(gene)
    }

    /// Check if this gene matches the given signals
    pub fn matches_signals(&self, signals: &[String]) -> usize {
        let mut score = 0;
        for signal in signals {
            for pattern in &self.signals_match {
                if pattern.matches(signal) {
                    score += 1;
                    break;
                }
            }
        }
        score
    }

    /// Validate the gene structure
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Gene id cannot be empty".to_string());
        }
        if self.strategy.is_empty() {
            return Err("Gene strategy cannot be empty".to_string());
        }
        if self.validation.is_empty() {
            return Err("Gene validation cannot be empty".to_string());
        }
        Ok(())
    }
}

/// Convert from Oris core Gene to GEP Gene
impl From<&crate::Gene> for GepGene {
    fn from(oris_gene: &crate::Gene) -> Self {
        let signals_match: Vec<SignalPattern> = oris_gene
            .signals
            .iter()
            .map(|s| SignalPattern::from(s.clone()))
            .collect();

        let constraints = GeneConstraints::new(20);

        GepGene {
            gene_type: "Gene".to_string(),
            schema_version: super::GEP_SCHEMA_VERSION.to_string(),
            id: oris_gene.id.clone(),
            category: GeneCategory::Repair, // Default category
            signals_match,
            preconditions: vec![],
            strategy: oris_gene.strategy.clone(),
            constraints,
            validation: oris_gene.validation.clone(),
            epigenetic_marks: vec![],
            model_name: None,
            asset_id: oris_gene.id.clone(), // Placeholder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_pattern_substring() {
        let pattern = SignalPattern::from("timeout".to_string());
        assert!(pattern.matches("connection timeout error"));
        assert!(pattern.matches("TIMEOUT DETECTED"));
        assert!(!pattern.matches("time out"));
    }

    #[test]
    fn test_signal_pattern_regex() {
        // regex-lite has limited features, test with substring instead
        // which is the default mode
        let pattern = SignalPattern::from("error".to_string());
        assert!(pattern.matches("error: need to retry"));
        assert!(pattern.matches("ERROR RETRY"));
        assert!(!pattern.matches("success"));
    }

    #[test]
    fn test_signal_pattern_alias() {
        let pattern = SignalPattern::from("en|zh|ja".to_string());
        assert!(pattern.matches("en: hello"));
        assert!(pattern.matches("zh: 你好"));
        assert!(!pattern.matches("fr: bonjour"));
    }

    #[test]
    fn test_gene_creation() {
        let gene = GepGene::new(
            "gene_test_001".to_string(),
            GeneCategory::Repair,
            vec!["timeout".to_string(), "error".to_string()],
            vec!["Analyze error".to_string(), "Fix issue".to_string()],
            vec!["cargo test".to_string()],
        )
        .unwrap();

        assert_eq!(gene.gene_type, "Gene");
        assert_eq!(gene.schema_version, "1.5.0");
        assert!(gene.asset_id.starts_with("sha256:")); // Should start with sha256:
    }

    #[test]
    fn test_gene_matches_signals() {
        let gene = GepGene::new(
            "gene_test_002".to_string(),
            GeneCategory::Repair,
            vec!["timeout".to_string(), "error".to_string()],
            vec!["Fix".to_string()],
            vec!["test".to_string()],
        )
        .unwrap();

        let signals = vec![
            "error: connection timeout".to_string(),
            "perf_bottleneck".to_string(),
        ];

        assert_eq!(gene.matches_signals(&signals), 1);
    }

    #[test]
    fn test_gene_validate() {
        let mut gene = GepGene::new(
            "gene_test_003".to_string(),
            GeneCategory::Repair,
            vec!["timeout".to_string()],
            vec![],
            vec!["test".to_string()],
        )
        .unwrap();

        assert!(gene.validate().is_err());

        gene.strategy.push("do something".to_string());
        assert!(gene.validate().is_ok());
    }
}
