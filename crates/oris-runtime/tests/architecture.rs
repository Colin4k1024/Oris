//! Architecture tests
//!
//! Tests to verify architectural patterns and module organization.

use oris_runtime::error::{error_info, ChainError, ErrorCode, LangChainError};

#[test]
fn test_error_unification() {
    // Test that all error types can be converted to LangChainError
    let chain_error = ChainError::OtherError("test".to_string());
    let langchain_error: LangChainError = chain_error.into();

    match langchain_error {
        LangChainError::ChainError(_) => {}
        _ => panic!("Expected ChainError variant"),
    }
}

#[test]
fn test_error_code_system() {
    let error = LangChainError::ConfigurationError("test".to_string());
    let code = ErrorCode::from_error(&error);
    assert_eq!(code, ErrorCode::ConfigurationError);
    assert_eq!(code.as_u32(), 9000);
}

#[test]
fn test_error_info() {
    let error = LangChainError::ConfigurationError("test config".to_string());
    let info = error_info(&error);
    assert!(info.contains("E9000"));
    assert!(info.contains("test config"));
}

#[test]
fn test_utils_similarity() {
    use oris_runtime::utils::{cosine_similarity_f64, text_similarity};

    // Test cosine similarity
    let vec1 = vec![1.0, 0.0];
    let vec2 = vec![1.0, 0.0];
    let similarity = cosine_similarity_f64(&vec1, &vec2);
    assert!((similarity - 1.0).abs() < 1e-10);

    // Test text similarity
    let text1 = "hello world";
    let text2 = "world hello";
    let text_sim = text_similarity(text1, text2);
    assert!((text_sim - 1.0).abs() < 1e-10);
}

#[test]
fn test_utils_vectors() {
    use oris_runtime::utils::{mean_embedding_f64, sum_vectors_f64};

    let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];

    let mean = mean_embedding_f64(&vectors);
    assert_eq!(mean, vec![2.0, 3.0]);

    let sum = sum_vectors_f64(&vectors);
    assert_eq!(sum, vec![4.0, 6.0]);
}

#[test]
fn test_type_aliases() {
    use oris_runtime::{Messages, Tools};

    // Verify type aliases are accessible
    let _tools: Tools = vec![];
    let _messages: Messages = vec![];
}

#[cfg(all(
    feature = "execution-server",
    feature = "agent-contract-experimental",
    feature = "evolution-network-experimental"
))]
mod gep_interop_golden_tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use tower::util::ServiceExt;

    use oris_runtime::evolution::{EvoEvolutionStore, EvolutionNetworkNode, JsonlEvolutionStore};
    use oris_runtime::evolution_network::FetchQuery;
    use oris_runtime::execution_server::{build_router, ExecutionApiState};
    use oris_runtime::graph::{
        function_node, InMemorySaver, MessagesState, StateGraph, END, START,
    };
    use oris_runtime::schemas::messages::Message;

    const GEP_PROFILE_CURRENT: &str = "gep-a2a-envelope-schema@1";
    const GEP_PROFILE_LEGACY: &str = "gep-a2a-envelope-schema@0";

    fn next_id(prefix: &str) -> String {
        static COUNTER: AtomicUsize = AtomicUsize::new(1);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}-{nanos}-{seq}")
    }

    fn gep_schema_hash(profile: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(profile.as_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    async fn build_test_graph() -> Arc<oris_runtime::graph::CompiledGraph<MessagesState>> {
        let node = function_node("research", |_state: &MessagesState| async move {
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message("ok")]).unwrap(),
            );
            Ok(update)
        });
        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("research", node).unwrap();
        graph.add_edge(START, "research");
        graph.add_edge("research", END);
        let saver = Arc::new(InMemorySaver::new());
        Arc::new(graph.compile_with_persistence(Some(saver), None).unwrap())
    }

    async fn build_runtime_router() -> (axum::Router, ExecutionApiState) {
        let evolution_store: Arc<dyn EvoEvolutionStore> =
            Arc::new(JsonlEvolutionStore::new(test_store_root("api-state")));
        let state =
            ExecutionApiState::new(build_test_graph().await).with_evolution_store(evolution_store);
        let router = build_router(state.clone());
        (router, state)
    }

    async fn post_json(
        router: &axum::Router,
        endpoint: &str,
        body: Value,
        case_id: &str,
        step: &str,
    ) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(Method::POST)
            .uri(endpoint)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap_or_else(|err| {
                panic!("case={case_id} step={step}: failed to build request: {err}")
            });
        let resp = router
            .clone()
            .oneshot(req)
            .await
            .unwrap_or_else(|err| panic!("case={case_id} step={step}: request failed: {err}"));
        let status = resp.status();
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|err| panic!("case={case_id} step={step}: read body failed: {err}"));
        let json: Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|err| {
            panic!(
                "case={case_id} step={step}: invalid json response: {err}; raw={}",
                String::from_utf8_lossy(&body_bytes)
            )
        });
        (status, json)
    }

    fn assert_pointer_eq(case_id: &str, step: &str, body: &Value, pointer: &str, expected: Value) {
        let actual = body.pointer(pointer).cloned().unwrap_or(Value::Null);
        assert_eq!(
            actual, expected,
            "case={case_id} step={step}: pointer={pointer} mismatch, body={body}"
        );
    }

    fn assert_has_gene(body: &Value, pointer: &str, gene_id: &str, case_id: &str, step: &str) {
        let assets = body
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| {
                panic!("case={case_id} step={step}: expected array at {pointer}, body={body}")
            });
        let matched = assets.iter().any(|asset| {
            asset
                .pointer("/kind")
                .and_then(Value::as_str)
                .map(|kind| kind == "gene")
                .unwrap_or(false)
                && asset
                    .pointer("/gene/id")
                    .and_then(Value::as_str)
                    .map(|id| id == gene_id)
                    .unwrap_or(false)
        });
        assert!(
            matched,
            "case={case_id} step={step}: gene_id={gene_id} missing at {pointer}, body={body}"
        );
    }

    async fn hello_gep(
        router: &axum::Router,
        sender_id: &str,
        schema_hash: Option<String>,
        include_mutation_proposal: bool,
        case_id: &str,
        step: &str,
    ) -> Value {
        let mut caps = vec![
            "coordination",
            "supervised_devloop",
            "replay_feedback",
            "evolution_fetch",
        ];
        if include_mutation_proposal {
            caps.push("mutation_proposal");
        }
        let payload = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": schema_hash,
            "capabilities": caps,
            "message_type": "hello",
            "message_id": next_id("gep-hello"),
            "sender_id": sender_id,
            "timestamp": "2026-03-13T00:00:00Z",
            "payload": {
                "capabilities": {
                    "coordination": true,
                    "supervised_devloop": true,
                    "replay_feedback": true,
                    "evolution_fetch": true,
                    "mutation_proposal": include_mutation_proposal
                }
            }
        });
        let (status, body) = post_json(router, "/a2a/hello", payload, case_id, step).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "case={case_id} step={step}: expected 200, body={body}"
        );
        body
    }

    fn test_store_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("oris-gep-golden-{label}-{}", next_id("store")))
    }

    #[tokio::test]
    async fn gep_interop_golden_fetch_delta_resume_uses_gep_envelope_contract() {
        let case_id = "golden_fetch_delta_resume";
        let sender_id = next_id("gep-fetch-agent");
        let (router, state) = build_runtime_router().await;
        let current_schema = gep_schema_hash(GEP_PROFILE_CURRENT);

        state
            .evolution_node
            .record_reported_experience(
                &sender_id,
                "golden-delta-gene-a".to_string(),
                vec!["golden.delta".into()],
                vec![
                    "task_class=golden.delta".into(),
                    "task_label=Golden delta".into(),
                ],
                vec!["a2a.tasks.report".into()],
            )
            .expect("seed golden delta gene a");

        let hello = hello_gep(
            &router,
            &sender_id,
            Some(current_schema.clone()),
            false,
            case_id,
            "hello-current",
        )
        .await;
        assert_pointer_eq(
            case_id,
            "hello-current",
            &hello,
            "/protocol_negotiation/schema_mode",
            json!("current"),
        );
        assert_pointer_eq(
            case_id,
            "hello-current",
            &hello,
            "/protocol_negotiation/downgraded",
            json!(false),
        );

        let first_fetch_payload = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": current_schema,
            "message_type": "fetch",
            "message_id": next_id("gep-fetch"),
            "sender_id": sender_id,
            "timestamp": "2026-03-13T00:00:10Z",
            "payload": {
                "signals": ["golden.delta"]
            }
        });
        let (first_status, first_body) = post_json(
            &router,
            "/a2a/fetch",
            first_fetch_payload,
            case_id,
            "fetch-first",
        )
        .await;
        assert_eq!(
            first_status,
            StatusCode::OK,
            "case={case_id} step=fetch-first: body={first_body}"
        );
        assert_has_gene(
            &first_body,
            "/data/assets",
            "golden-delta-gene-a",
            case_id,
            "fetch-first",
        );
        let next_cursor = first_body
            .pointer("/data/next_cursor")
            .and_then(Value::as_str)
            .expect("case=golden_fetch_delta_resume step=fetch-first: missing next_cursor")
            .to_string();
        let resume_token = first_body
            .pointer("/data/resume_token")
            .and_then(Value::as_str)
            .expect("case=golden_fetch_delta_resume step=fetch-first: missing resume_token")
            .to_string();

        state
            .evolution_node
            .record_reported_experience(
                &sender_id,
                "golden-delta-gene-b".to_string(),
                vec!["golden.delta".into()],
                vec![
                    "task_class=golden.delta".into(),
                    "task_label=Golden delta".into(),
                ],
                vec!["a2a.tasks.report".into()],
            )
            .expect("seed golden delta gene b");

        let second_fetch_payload = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": gep_schema_hash(GEP_PROFILE_CURRENT),
            "message_type": "fetch",
            "message_id": next_id("gep-fetch"),
            "sender_id": sender_id,
            "timestamp": "2026-03-13T00:00:20Z",
            "payload": {
                "signals": ["golden.delta"],
                "resume_token": resume_token
            }
        });
        let (second_status, second_body) = post_json(
            &router,
            "/a2a/fetch",
            second_fetch_payload,
            case_id,
            "fetch-second",
        )
        .await;
        assert_eq!(
            second_status,
            StatusCode::OK,
            "case={case_id} step=fetch-second: body={second_body}"
        );
        assert_pointer_eq(
            case_id,
            "fetch-second",
            &second_body,
            "/data/sync_audit/requested_cursor",
            json!(next_cursor),
        );
        assert_has_gene(
            &second_body,
            "/data/assets",
            "golden-delta-gene-b",
            case_id,
            "fetch-second",
        );
        let still_has_old_gene = second_body
            .pointer("/data/assets")
            .and_then(Value::as_array)
            .map(|assets| {
                assets.iter().any(|asset| {
                    asset.pointer("/kind").and_then(Value::as_str) == Some("gene")
                        && asset.pointer("/gene/id").and_then(Value::as_str)
                            == Some("golden-delta-gene-a")
                })
            })
            .unwrap_or(false);
        assert!(
            !still_has_old_gene,
            "case={case_id} step=fetch-second: expected delta to exclude old gene, body={second_body}"
        );
    }

    #[tokio::test]
    async fn gep_interop_golden_downgrade_and_error_contract_cases_are_stable() {
        let case_id = "golden_downgrade_and_error_contract";
        let sender_id = next_id("gep-downgrade-agent");
        let (router, _) = build_runtime_router().await;

        let legacy_hello = hello_gep(
            &router,
            &sender_id,
            Some(gep_schema_hash(GEP_PROFILE_LEGACY)),
            true,
            case_id,
            "hello-legacy",
        )
        .await;
        assert_pointer_eq(
            case_id,
            "hello-legacy",
            &legacy_hello,
            "/protocol_negotiation/schema_mode",
            json!("legacy-compat"),
        );
        assert_pointer_eq(
            case_id,
            "hello-legacy",
            &legacy_hello,
            "/protocol_negotiation/downgraded",
            json!(true),
        );
        let negotiated_caps = legacy_hello
            .pointer("/protocol_negotiation/negotiated_capabilities")
            .and_then(Value::as_array)
            .expect("case=golden_downgrade_and_error_contract step=hello-legacy: missing negotiated capabilities");
        assert!(
            !negotiated_caps
                .iter()
                .any(|item| item == &json!("mutation_proposal")),
            "case={case_id} step=hello-legacy: mutation_proposal should be downgraded out, body={legacy_hello}"
        );

        let unsupported_schema_fetch = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": "sha256:not-supported",
            "message_type": "fetch",
            "message_id": next_id("gep-fetch"),
            "sender_id": next_id("gep-unknown-schema-agent"),
            "payload": {
                "include_tasks": true
            }
        });
        let (bad_schema_status, bad_schema_body) = post_json(
            &router,
            "/a2a/fetch",
            unsupported_schema_fetch,
            case_id,
            "fetch-unsupported-schema",
        )
        .await;
        assert_eq!(
            bad_schema_status,
            StatusCode::BAD_REQUEST,
            "case={case_id} step=fetch-unsupported-schema: body={bad_schema_body}"
        );
        assert_pointer_eq(
            case_id,
            "fetch-unsupported-schema",
            &bad_schema_body,
            "/error/details/a2a_error_code",
            json!("UnsupportedProtocol"),
        );
        assert_pointer_eq(
            case_id,
            "fetch-unsupported-schema",
            &bad_schema_body,
            "/error/details/actual_schema_hash",
            json!("sha256:not-supported"),
        );
        let expected_hashes = bad_schema_body
            .pointer("/error/details/expected_schema_hashes")
            .and_then(Value::as_array)
            .expect("case=golden_downgrade_and_error_contract step=fetch-unsupported-schema: missing expected_schema_hashes");
        assert!(
            expected_hashes
                .iter()
                .any(|value| value == &json!(gep_schema_hash(GEP_PROFILE_CURRENT))),
            "case={case_id} step=fetch-unsupported-schema: expected current schema hash missing, body={bad_schema_body}"
        );

        let invalid_payload_fetch = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "message_type": "fetch",
            "message_id": next_id("gep-fetch"),
            "sender_id": next_id("gep-invalid-payload-agent"),
            "payload": "not-an-object"
        });
        let (invalid_payload_status, invalid_payload_body) = post_json(
            &router,
            "/a2a/fetch",
            invalid_payload_fetch,
            case_id,
            "fetch-invalid-payload",
        )
        .await;
        assert_eq!(
            invalid_payload_status,
            StatusCode::BAD_REQUEST,
            "case={case_id} step=fetch-invalid-payload: body={invalid_payload_body}"
        );
        assert_pointer_eq(
            case_id,
            "fetch-invalid-payload",
            &invalid_payload_body,
            "/error/details/a2a_error_code",
            json!("ValidationFailed"),
        );
        let invalid_payload_message = invalid_payload_body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            invalid_payload_message.contains("invalid gep-a2a envelope payload"),
            "case={case_id} step=fetch-invalid-payload: expected payload error message, body={invalid_payload_body}"
        );
    }

    #[tokio::test]
    async fn gep_interop_golden_manifest_cases_capture_tamper_failures() {
        let case_id = "golden_manifest_cases";
        let source_store: Arc<dyn EvoEvolutionStore> =
            Arc::new(JsonlEvolutionStore::new(test_store_root("manifest-source")));
        let source_node = EvolutionNetworkNode::new(source_store);

        source_node
            .record_reported_experience(
                "golden-manifest-source",
                "golden-manifest-gene".to_string(),
                vec!["golden.manifest".into()],
                vec![
                    "task_class=golden.manifest".into(),
                    "task_label=Golden manifest".into(),
                ],
                vec!["a2a.tasks.report".into()],
            )
            .expect("seed golden manifest gene");

        let envelope = source_node
            .publish_local_assets("golden-manifest-source")
            .expect("publish local assets with manifest");
        assert!(
            envelope.verify_content_hash(),
            "case={case_id}: published envelope content hash should validate"
        );
        assert!(
            envelope.verify_manifest().is_ok(),
            "case={case_id}: published envelope manifest should validate"
        );

        let mut tampered = envelope.clone();
        if let Some(manifest) = tampered.manifest.as_mut() {
            manifest.asset_hash = "tampered-hash".to_string();
        }
        tampered.content_hash = tampered.compute_content_hash();
        let tampered_err = tampered
            .verify_manifest()
            .expect_err("tampered manifest must fail");
        assert!(
            tampered_err.contains("manifest"),
            "case={case_id}: tampered manifest error should mention manifest, error={tampered_err}"
        );

        let mut missing_manifest = envelope.clone();
        missing_manifest.manifest = None;
        missing_manifest.content_hash = missing_manifest.compute_content_hash();
        let missing_manifest_err = missing_manifest
            .verify_manifest()
            .expect_err("missing manifest must fail");
        assert_eq!(
            missing_manifest_err, "missing manifest",
            "case={case_id}: missing manifest error mismatch"
        );
    }

    #[tokio::test]
    async fn gep_interop_golden_duplicate_complete_conflict_is_deterministic() {
        let case_id = "golden_duplicate_complete_conflict";
        let sender_id = next_id("gep-complete-agent");
        let task_id = next_id("gep-task");
        let (router, _) = build_runtime_router().await;

        let _ = hello_gep(
            &router,
            &sender_id,
            Some(gep_schema_hash(GEP_PROFILE_CURRENT)),
            false,
            case_id,
            "hello",
        )
        .await;

        let distribute_payload = json!({
            "sender_id": sender_id,
            "task_id": task_id,
            "task_summary": "golden duplicate complete"
        });
        let (distribute_status, distribute_body) = post_json(
            &router,
            "/a2a/tasks/distribute",
            distribute_payload,
            case_id,
            "distribute",
        )
        .await;
        assert_eq!(
            distribute_status,
            StatusCode::OK,
            "case={case_id} step=distribute: body={distribute_body}"
        );

        let claim_payload = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": gep_schema_hash(GEP_PROFILE_CURRENT),
            "message_type": "fetch",
            "message_id": next_id("gep-claim"),
            "sender_id": sender_id,
            "payload": {}
        });
        let (claim_status, claim_body) =
            post_json(&router, "/a2a/task/claim", claim_payload, case_id, "claim").await;
        assert_eq!(
            claim_status,
            StatusCode::OK,
            "case={case_id} step=claim: body={claim_body}"
        );
        assert_pointer_eq(case_id, "claim", &claim_body, "/data/claimed", json!(true));

        let complete_payload = json!({
            "protocol": "gep-a2a",
            "protocol_version": "1.0.0",
            "schema_hash": gep_schema_hash(GEP_PROFILE_CURRENT),
            "message_type": "report",
            "message_id": next_id("gep-complete"),
            "sender_id": sender_id,
            "payload": {
                "task_id": task_id,
                "success": true,
                "summary": "golden duplicate complete succeeded",
                "used_capsule": true,
                "capsule_id": "golden-capsule-1",
                "reasoning_steps_avoided": 1
            }
        });
        let (first_complete_status, first_complete_body) = post_json(
            &router,
            "/a2a/task/complete",
            complete_payload.clone(),
            case_id,
            "complete-first",
        )
        .await;
        assert_eq!(
            first_complete_status,
            StatusCode::OK,
            "case={case_id} step=complete-first: body={first_complete_body}"
        );

        let (second_complete_status, second_complete_body) = post_json(
            &router,
            "/a2a/task/complete",
            complete_payload,
            case_id,
            "complete-duplicate",
        )
        .await;
        assert_eq!(
            second_complete_status,
            StatusCode::CONFLICT,
            "case={case_id} step=complete-duplicate: body={second_complete_body}"
        );
        assert_pointer_eq(
            case_id,
            "complete-duplicate",
            &second_complete_body,
            "/error/details/reason",
            json!("already_completed_or_unknown"),
        );
    }

    #[test]
    fn gep_interop_golden_sample_set_is_enumerated_for_ci_gate_visibility() {
        let cases = vec![
            "success.delta_resume",
            "failure.unsupported_schema_hash",
            "failure.invalid_payload_contract",
            "downgrade.legacy_schema_window",
            "idempotent_conflict.duplicate_complete",
            "manifest.validation_tamper",
        ];
        assert_eq!(
            cases.len(),
            6,
            "golden sample set count changed; update issue #197 contract if intentional"
        );
    }

    #[test]
    fn fetch_query_resume_token_round_trip_keeps_delta_inputs() {
        let query = FetchQuery {
            sender_id: "sender-a".to_string(),
            signals: vec!["delta.signal".to_string()],
            since_cursor: Some("10".to_string()),
            resume_token: Some("sender-a:10".to_string()),
        };
        let value = serde_json::to_value(&query).expect("serialize fetch query");
        assert_eq!(value["resume_token"], json!("sender-a:10"));
    }
}
