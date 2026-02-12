# Test Documentation

This document describes the test suite structure and provides guidance for running tests.

## Test Organization

### Unit Tests (No External Dependencies)

Unit tests are located inline within modules and run without external services:

```bash
cargo test
```

### Integration Tests (Require API Keys)

Integration tests are marked with `#[ignore]` and require external services:

```bash
cargo test --features openai,ollama -- --ignored
```

### Integration Test Directory

End-to-end workflow tests are in `tests/` directory:
- `architecture.rs`: Architecture and type system tests

## Required Environment Variables

| Test Category | Variables |
|--------------|-----------|
| OpenAI | `OPENAI_API_KEY` |
| Anthropic Claude | `ANTHROPIC_API_KEY` |
| Google Gemini | `GOOGLE_API_KEY` |
| Mistral AI | `MISTRAL_API_KEY` |
| Ollama (local) | `OLLAMA_HOST` (optional, default: `http://localhost:11434`) |
| AWS Bedrock | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION` |
| GitHub | `GITHUB_TOKEN` |
| AWS S3 | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`, `S3_BUCKET` |

## Running Tests by Category

### All Non-Ignored Tests

```bash
cargo test
```

### All Tests (Including Integration)

```bash
cargo test --features openai,ollama,mistralai,gemini,bedrock -- --ignored
```

### OpenAI Tests Only

```bash
cargo test --features openai test_openai -- --ignored
```

### Ollama Tests Only

```bash
cargo test --features ollama test_ollama -- --ignored
```

## Test Categories by Module

### LLM Provider Tests

| File | Provider | Required |
|------|----------|----------|
| `src/llm/openai/mod.rs` | OpenAI | `OPENAI_API_KEY` |
| `src/llm/claude/client.rs` | Anthropic Claude | `ANTHROPIC_API_KEY` |
| `src/llm/gemini/client.rs` | Google Gemini | `GOOGLE_API_KEY` |
| `src/llm/mistralai/client.rs` | Mistral AI | `MISTRAL_API_KEY` |
| `src/llm/ollama/client.rs` | Ollama | Running Ollama (`ollama serve`) |
| `src/llm/deepseek/client.rs` | DeepSeek | `DEEPSEEK_API_KEY` |
| `src/llm/qwen/client.rs` | Alibaba Qwen | `QWEN_API_KEY` |
| `src/llm/bedrock/client.rs` | AWS Bedrock | AWS credentials + Bedrock access |
| `src/llm/huggingface/client.rs` | HuggingFace | `HF_TOKEN` |

### Embedding Tests

| File | Provider | Required |
|------|----------|----------|
| `src/embedding/mistralai/mistralai_embedder.rs` | Mistral AI | `MISTRAL_API_KEY` |
| `src/embedding/ollama/ollama_embedder.rs` | Ollama | Running Ollama |

### Chain Tests

| File | Test | Required |
|------|------|----------|
| `src/chain/llm_chain.rs` | `test_invoke_chain` | `OPENAI_API_KEY` |
| `src/chain/sequential/chain.rs` | Sequential chain tests | API key |
| `src/chain/question_answering.rs` | Q&A chain tests | API key + Vector store |
| `src/chain/conversational_retrieval_qa/` | Conversational RAG | API key + Vector store |

### Agent Tests

| File | Test | Required |
|------|------|----------|
| `src/agent/chat/chat_agent.rs` | Chat agent | API key |
| `tests/architecture.rs` | Architecture tests | None (unit tests) |

### Tool Tests

| File | Tool | Required |
|------|------|----------|
| `src/tools/duckduckgo/duckduckgo_search.rs` | DuckDuckGo | None (should work) |
| `src/tools/wolfram/wolfram.rs` | Wolfram Alpha | `WOLFRAM_APP_ID` |
| `src/tools/serpapi/serpapi.rs` | SerpApi | `SERPAPI_API_KEY` |
| `src/tools/text2speech/openai/client.rs` | OpenAI TTS | `OPENAI_API_KEY` |

### Document Loader Tests

| File | Loader | Required |
|------|--------|----------|
| `src/document_loaders/git_commit_loader/` | Git commits | Git repository |
| `src/document_loaders/github_loader.rs` | GitHub | `GITHUB_TOKEN` |
| `src/document_loaders/aws_s3_loader.rs` | AWS S3 | AWS credentials + bucket |
| `src/document_loaders/web_loaders/` | Web scraping | Network access |
| `src/document_loaders/office_loaders/excel_loader.rs` | Excel | Excel file |

## Vector Store Integration Tests

Vector store tests typically require:
- Running database service (PostgreSQL, MongoDB, etc.)
- Appropriate feature flag enabled

```bash
# PostgreSQL with pgvector
cargo test --features postgres

# Qdrant
cargo test --features qdrant

# MongoDB
cargo test --features mongodb
```

## Semantic Router Tests

| File | Test | Required |
|------|------|----------|
| `src/semantic_router/route_layer/route_layer.rs` | Route layer | API key (for embeddings) |

## Best Practices for Adding Tests

### 1. Adding Unit Tests

Unit tests should be placed in `#[cfg(test)]` modules within the source file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // Test implementation
    }
}
```

### 2. Adding Integration Tests

Integration tests that require API keys should:
- Be marked with `#[ignore]`
- Include the required environment variables in the ignore reason
- Include a comment with setup instructions

```rust
#[tokio::test]
#[ignore = "Requires OPENAI_API_KEY - see tests/README.md"]
async fn test_with_api() {
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY must be set");
    // Test implementation
}
```

### 3. Adding E2E Workflow Tests

End-to-end tests should be added to `tests/` directory:

```rust
// tests/my_workflow_test.rs
use oris::chain::Chain;

#[tokio::test]
#[ignore = "Requires OPENAI_API_KEY"]
async fn test_complete_workflow() {
    // Test implementation
}
```

## CI/CD Configuration

See `.github/workflows/` for CI configuration. Integration tests run on schedule with API keys from secrets.

## Troubleshooting

### Test Fails with "Not implemented"

Some tests may fail with "Not implemented" if the underlying feature is not yet complete. Check:
- Feature flags are enabled
- Required dependencies are present
- The feature is actively being developed

### API Connection Errors

1. Verify environment variables are set
2. Check API keys are valid
3. Ensure network connectivity
4. For local services (Ollama), verify the service is running

### Timeout Errors

Some tests may timeout if:
- API rate limits are hit
- Network latency is high
- Service is unresponsive

Increase timeout or run tests individually:
```bash
cargo test --test my_test -- --nocapture
```
