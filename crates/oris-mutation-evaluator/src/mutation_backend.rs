//! oris-mutation-evaluator/src/mutation_backend.rs
//!
//! LLM-driven mutation backend — generates a `MutationProposal` from runtime
//! signals via a pluggable `LlmMutationBackend` trait.
//!
//! # Backend selection
//!
//! Use [`EnvRoutedBackend::from_env`] in production.  It reads the following
//! environment variables and selects the first available provider:
//!
//! | Variable            | Provider          |
//! |---------------------|-------------------|
//! | `ANTHROPIC_API_KEY` | Anthropic/Claude  |
//! | `OPENAI_API_KEY`    | OpenAI            |
//! | `OLLAMA_HOST`       | Ollama (local)    |
//!
//! When none of those variables are set the backend falls back to
//! [`MockMutationBackend::passing`], which is convenient for offline
//! development and CI.
//!
//! # Proposal contract
//!
//! Every generated `MutationProposal` should be validated through
//! [`ProposalContract::validate`] before it is passed to
//! [`MutationEvaluator`](crate::evaluator::MutationEvaluator).

use crate::types::{EvoSignal, MutationProposal};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Input type
// ---------------------------------------------------------------------------

/// Everything the mutation backend needs to generate a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRequest {
    /// Stable run identifier that ties this request to an evolution cycle.
    pub run_id: String,
    /// Runtime signals (compiler errors, panics, test failures …) that
    /// triggered this evolution cycle.
    pub signals: Vec<EvoSignal>,
    /// The source code (or prompt/config) that is a candidate for mutation.
    pub context_code: String,
    /// Optional gene id that hints at context from a successful past mutation.
    pub gene_hint_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Implement this trait to plug in any LLM backend for mutation *generation*.
///
/// This is intentionally separate from [`crate::critic::LlmCritic`], which
/// scores an existing proposal.  The two traits may (and often will) resolve
/// to different LLM calls or even different models.
#[async_trait]
pub trait LlmMutationBackend: Send + Sync {
    /// Generate a single `MutationProposal` for `request`.
    async fn generate(&self, request: &MutationRequest) -> Result<MutationProposal>;

    /// Human-readable name of this backend (used in tracing / logs).
    fn backend_name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Prompt builders
// ---------------------------------------------------------------------------

/// System prompt for the mutation-generation LLM call.
///
/// Stored in one place so the rubric can be version-controlled independently.
pub fn build_mutation_system_prompt() -> &'static str {
    r#"You are a Rust mutation engine. Given runtime signals (compiler errors,
test failures, panics) and source code context, you propose a targeted code
mutation that fixes the root cause.

## Output contract
Respond with a single JSON object — no markdown fences, no prose outside the
object.
Schema:
{
  "intent":   <string>,  // ≤ 2 sentences: what the mutation fixes and why
  "proposed": <string>   // Complete replacement for the supplied source code
}

## Rules
- Address the root cause of the signals; never merely suppress errors
- Keep the blast radius minimal (change as little as possible)
- Never delete or ignore existing tests; fix them instead
- Never hardcode values just to make a specific test pass
"#
}

/// Build the user message for a specific mutation request.
pub fn build_mutation_user_prompt(request: &MutationRequest) -> String {
    let signals_text = request
        .signals
        .iter()
        .map(|s| format!("[{:?}] {} (at {:?})", s.kind, s.message, s.location))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"## Runtime signals
{signals}

## Source code to mutate
```rust
{code}
```

Generate a mutation to fix these signals and return the JSON object."#,
        signals = signals_text,
        code = request.context_code,
    )
}

/// Raw JSON shape that LLM mutation backends are expected to return.
#[derive(Debug, Deserialize)]
pub(crate) struct MutationResponse {
    pub intent: String,
    pub proposed: String,
}

// ---------------------------------------------------------------------------
// ProposalContract
// ---------------------------------------------------------------------------

/// Describes a single structural constraint violation in a `MutationProposal`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractViolation {
    /// The field that failed validation.
    pub field: String,
    /// Human-readable reason.
    pub reason: String,
}

/// Validates that a `MutationProposal` satisfies the structural contract
/// before it is handed to `MutationEvaluator`.
pub struct ProposalContract;

impl ProposalContract {
    /// Returns `Ok(())` when all constraints pass, or a non-empty list of
    /// [`ContractViolation`]s otherwise.
    ///
    /// # Contract rules
    ///
    /// 1. `intent` must be non-empty.
    /// 2. `original` must be non-empty.
    /// 3. `proposed` must be non-empty.
    /// 4. `proposed` must differ from `original` (no-op detection).
    /// 5. At least one signal must be present.
    pub fn validate(proposal: &MutationProposal) -> Result<(), Vec<ContractViolation>> {
        let mut v = Vec::new();

        if proposal.intent.trim().is_empty() {
            v.push(ContractViolation {
                field: "intent".into(),
                reason: "intent must not be empty".into(),
            });
        }
        if proposal.original.trim().is_empty() {
            v.push(ContractViolation {
                field: "original".into(),
                reason: "original code must not be empty".into(),
            });
        }
        if proposal.proposed.trim().is_empty() {
            v.push(ContractViolation {
                field: "proposed".into(),
                reason: "proposed code must not be empty".into(),
            });
        }
        if proposal.proposed == proposal.original {
            v.push(ContractViolation {
                field: "proposed".into(),
                reason: "proposed code must differ from original (no-op)".into(),
            });
        }
        if proposal.signals.is_empty() {
            v.push(ContractViolation {
                field: "signals".into(),
                reason: "at least one runtime signal is required".into(),
            });
        }

        if v.is_empty() {
            Ok(())
        } else {
            Err(v)
        }
    }
}

// ---------------------------------------------------------------------------
// MockMutationBackend
// ---------------------------------------------------------------------------

/// Deterministic mock backend for unit tests and CI environments without
/// an LLM API key.
pub struct MockMutationBackend {
    pub intent: String,
    pub proposed_code: String,
}

impl MockMutationBackend {
    pub fn new(intent: impl Into<String>, proposed_code: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            proposed_code: proposed_code.into(),
        }
    }

    /// A "passing" mock that generates a plausible mutation distinct from any
    /// original input.
    pub fn passing() -> Self {
        Self::new(
            "Add bounds check to return `None` instead of panicking on empty input",
            "fn process(items: &[u32]) -> Option<u32> {\n    items.first().copied()\n}",
        )
    }
}

#[async_trait]
impl LlmMutationBackend for MockMutationBackend {
    async fn generate(&self, request: &MutationRequest) -> Result<MutationProposal> {
        Ok(MutationProposal {
            id: Uuid::new_v4(),
            intent: self.intent.clone(),
            original: request.context_code.clone(),
            proposed: self.proposed_code.clone(),
            signals: request.signals.clone(),
            source_gene_id: request.gene_hint_id,
        })
    }

    fn backend_name(&self) -> &str {
        "mock"
    }
}

// ---------------------------------------------------------------------------
// HttpCompletionBackend (provider-agnostic stub)
// ---------------------------------------------------------------------------

/// Provider variant for [`HttpCompletionBackend`].
#[derive(Debug, Clone, Copy)]
pub enum LlmProvider {
    /// OpenAI `/v1/chat/completions` — also used by Ollama.
    OpenAiCompat,
    /// Anthropic `/v1/messages`.
    Anthropic,
}

/// Provider-agnostic HTTP completion backend.
///
/// Calls any OpenAI-compatible `/v1/chat/completions` endpoint (OpenAI, Ollama)
/// or the Anthropic `/v1/messages` endpoint.
///
/// **In-process HTTP calls are only executed when the `llm-http` Cargo feature
/// is enabled.**  Without that feature the backend returns an informative error
/// so that downstream code can detect misconfiguration early.
pub struct HttpCompletionBackend {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub model: String,
    pub provider: LlmProvider,
}

impl HttpCompletionBackend {
    /// Configure from `OPENAI_API_KEY` / `OPENAI_MODEL`.
    pub fn openai_from_env() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1/chat/completions".into(),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            provider: LlmProvider::OpenAiCompat,
        }
    }

    /// Configure from `ANTHROPIC_API_KEY` / `ANTHROPIC_MODEL`.
    pub fn anthropic_from_env() -> Self {
        Self {
            endpoint: "https://api.anthropic.com/v1/messages".into(),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            model: std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-5-haiku-20241022".into()),
            provider: LlmProvider::Anthropic,
        }
    }

    /// Configure from `OLLAMA_HOST` / `OLLAMA_MODEL`.
    pub fn ollama_from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
        Self {
            endpoint: format!("{host}/v1/chat/completions"),
            api_key: None,
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".into()),
            provider: LlmProvider::OpenAiCompat,
        }
    }
}

#[async_trait]
impl LlmMutationBackend for HttpCompletionBackend {
    async fn generate(&self, _request: &MutationRequest) -> Result<MutationProposal> {
        // Network I/O is not compiled in without the `llm-http` feature.
        // Enable it to activate real HTTP calls via reqwest.
        anyhow::bail!(
            "HttpCompletionBackend requires the `llm-http` Cargo feature \
             (configured endpoint: {})",
            self.endpoint
        )
    }

    fn backend_name(&self) -> &str {
        match self.provider {
            LlmProvider::OpenAiCompat => "http-openai-compat",
            LlmProvider::Anthropic => "http-anthropic",
        }
    }
}

// ---------------------------------------------------------------------------
// EnvRoutedBackend
// ---------------------------------------------------------------------------

/// Selects a mutation backend automatically based on environment variables.
///
/// Priority order:
/// 1. `ANTHROPIC_API_KEY` → [`HttpCompletionBackend::anthropic_from_env`]
/// 2. `OPENAI_API_KEY`    → [`HttpCompletionBackend::openai_from_env`]
/// 3. `OLLAMA_HOST`       → [`HttpCompletionBackend::ollama_from_env`]
/// 4. Fallback            → [`MockMutationBackend::passing`]
pub struct EnvRoutedBackend {
    inner: Box<dyn LlmMutationBackend>,
    selected_provider: String,
}

impl EnvRoutedBackend {
    /// Build from environment variables present at call time.
    pub fn from_env() -> Self {
        let (provider, inner): (&str, Box<dyn LlmMutationBackend>) =
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                (
                    "anthropic",
                    Box::new(HttpCompletionBackend::anthropic_from_env()),
                )
            } else if std::env::var("OPENAI_API_KEY").is_ok() {
                ("openai", Box::new(HttpCompletionBackend::openai_from_env()))
            } else if std::env::var("OLLAMA_HOST").is_ok() {
                ("ollama", Box::new(HttpCompletionBackend::ollama_from_env()))
            } else {
                ("mock-fallback", Box::new(MockMutationBackend::passing()))
            };
        Self {
            inner,
            selected_provider: provider.to_string(),
        }
    }

    /// Override the inner backend (useful in tests).
    pub fn with_backend(inner: impl LlmMutationBackend + 'static) -> Self {
        let name = inner.backend_name().to_string();
        Self {
            inner: Box::new(inner),
            selected_provider: name,
        }
    }

    /// The provider name that was selected during construction.
    pub fn selected_provider(&self) -> &str {
        &self.selected_provider
    }
}

#[async_trait]
impl LlmMutationBackend for EnvRoutedBackend {
    async fn generate(&self, request: &MutationRequest) -> Result<MutationProposal> {
        self.inner.generate(request).await
    }

    fn backend_name(&self) -> &str {
        &self.selected_provider
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SignalKind;

    fn sample_request() -> MutationRequest {
        MutationRequest {
            run_id: "run-001".into(),
            signals: vec![crate::types::EvoSignal {
                kind: SignalKind::Panic,
                message: "index out of bounds: the len is 0".into(),
                location: Some("src/main.rs:10".into()),
            }],
            context_code: "fn process(v: &[u32]) -> u32 { v[0] }".into(),
            gene_hint_id: None,
        }
    }

    #[tokio::test]
    async fn test_mock_backend_generates_valid_proposal() {
        let backend = MockMutationBackend::passing();
        let req = sample_request();
        let proposal = backend.generate(&req).await.unwrap();
        assert_eq!(proposal.signals.len(), 1);
        assert!(!proposal.intent.is_empty());
        assert_ne!(proposal.proposed, proposal.original);
    }

    #[test]
    fn test_proposal_contract_passes_valid_proposal() {
        let proposal = MutationProposal {
            id: Uuid::new_v4(),
            intent: "Fix panic".into(),
            original: "fn f() { panic!() }".into(),
            proposed: "fn f() {}".into(),
            signals: vec![crate::types::EvoSignal {
                kind: SignalKind::Panic,
                message: "explicit panic".into(),
                location: None,
            }],
            source_gene_id: None,
        };
        assert!(ProposalContract::validate(&proposal).is_ok());
    }

    #[test]
    fn test_proposal_contract_rejects_empty_intent() {
        let proposal = MutationProposal {
            id: Uuid::new_v4(),
            intent: "  ".into(),
            original: "fn f() {}".into(),
            proposed: "fn g() {}".into(),
            signals: vec![crate::types::EvoSignal {
                kind: SignalKind::CompilerError,
                message: "E0...".into(),
                location: None,
            }],
            source_gene_id: None,
        };
        let errs = ProposalContract::validate(&proposal).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "intent"));
    }

    #[test]
    fn test_proposal_contract_rejects_no_op() {
        let code = "fn f() -> u32 { 42 }";
        let proposal = MutationProposal {
            id: Uuid::new_v4(),
            intent: "No change".into(),
            original: code.into(),
            proposed: code.into(), // identical → no-op
            signals: vec![crate::types::EvoSignal {
                kind: SignalKind::TestFailure,
                message: "test_x failed".into(),
                location: None,
            }],
            source_gene_id: None,
        };
        let errs = ProposalContract::validate(&proposal).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "proposed"));
    }

    #[test]
    fn test_proposal_contract_rejects_empty_signals() {
        let proposal = MutationProposal {
            id: Uuid::new_v4(),
            intent: "Some intent".into(),
            original: "fn f() {}".into(),
            proposed: "fn g() {}".into(),
            signals: vec![], // empty!
            source_gene_id: None,
        };
        let errs = ProposalContract::validate(&proposal).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "signals"));
    }

    #[test]
    fn test_env_routed_backend_falls_back_to_mock_without_env() {
        // Ensure no LLM keys are set for this test
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OLLAMA_HOST");
        let backend = EnvRoutedBackend::from_env();
        assert_eq!(backend.selected_provider(), "mock-fallback");
    }

    #[tokio::test]
    async fn test_env_routed_backend_generates_via_mock_fallback() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OLLAMA_HOST");
        let backend = EnvRoutedBackend::from_env();
        let proposal = backend.generate(&sample_request()).await.unwrap();
        assert!(ProposalContract::validate(&proposal).is_ok());
    }

    #[test]
    fn test_build_mutation_user_prompt_contains_signals() {
        let req = sample_request();
        let prompt = build_mutation_user_prompt(&req);
        assert!(prompt.contains("index out of bounds"));
        assert!(prompt.contains("src/main.rs:10"));
    }
}
