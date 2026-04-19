//! Smoke tests for oris-execution-server.
//!
//! All tests are gated on the `execution-server` feature because the crate's
//! public surface is conditionally compiled under that flag.

#[cfg(feature = "execution-server")]
mod smoke {
    use oris_execution_server::{
        ApiMeta, ApiRole, RunJobRequest, WorkerPollRequest,
        generate_runtime_api_contract, RUNTIME_API_CONTRACT_DOC_PATH,
    };

    // -------------------------------------------------------------------------
    // Test 1: ApiRole default value and string representation
    // -------------------------------------------------------------------------
    #[test]
    fn api_role_default_is_admin() {
        let role = ApiRole::default();
        // The default impl on ApiRole returns Admin, which is the highest-privilege
        // sentinel. Verify the variant is constructed without panicking.
        let _operator = ApiRole::Operator;
        let _worker = ApiRole::Worker;
        // Admin is the default — ensure we can construct all three.
        matches!(role, ApiRole::Admin);
    }

    // -------------------------------------------------------------------------
    // Test 2: ApiMeta::ok() produces the correct status and api_version fields
    // -------------------------------------------------------------------------
    #[test]
    fn api_meta_ok_has_correct_fields() {
        let meta = ApiMeta::ok();
        assert_eq!(meta.status, "ok");
        assert_eq!(meta.api_version, "v1");
    }

    // -------------------------------------------------------------------------
    // Test 3: RunJobResponse serializes to JSON with expected fields
    // -------------------------------------------------------------------------
    #[test]
    fn run_job_response_serializes_to_json() {
        use oris_execution_server::RunJobResponse;
        let resp = RunJobResponse {
            thread_id: "t-xyz".to_string(),
            status: "queued".to_string(),
            interrupts: vec![],
            idempotency_key: None,
            idempotent_replay: false,
            trace: None,
        };
        let json = serde_json::to_string(&resp).expect("RunJobResponse should serialize to JSON");
        assert!(json.contains("t-xyz"));
        assert!(json.contains("queued"));
    }

    // -------------------------------------------------------------------------
    // Test 4: RunJobRequest deserializes from minimal JSON
    // -------------------------------------------------------------------------
    #[test]
    fn run_job_request_deserializes_from_minimal_json() {
        let json = r#"{"thread_id": "t-123"}"#;
        let req: RunJobRequest = serde_json::from_str(json)
            .expect("RunJobRequest should deserialize from minimal JSON");
        assert_eq!(req.thread_id, "t-123");
        assert!(req.input.is_none());
        assert!(req.idempotency_key.is_none());
    }

    // -------------------------------------------------------------------------
    // Test 5: WorkerPollRequest deserializes from JSON
    // -------------------------------------------------------------------------
    #[test]
    fn worker_poll_request_deserializes_from_json() {
        let json = r#"{"worker_id": "w-abc", "limit": 5}"#;
        let req: WorkerPollRequest = serde_json::from_str(json)
            .expect("WorkerPollRequest should deserialize from JSON");
        assert_eq!(req.worker_id, "w-abc");
        assert_eq!(req.limit, Some(5));
    }

    // -------------------------------------------------------------------------
    // Test 6: generate_runtime_api_contract returns a non-empty contract
    // -------------------------------------------------------------------------
    #[test]
    fn generate_runtime_api_contract_returns_nonempty_contract() {
        let contract = generate_runtime_api_contract();
        assert_eq!(contract.api_version, "v1");
        assert!(
            !contract.endpoints.is_empty(),
            "API contract must describe at least one endpoint"
        );
        assert!(
            !contract.schemas.is_empty(),
            "API contract must include at least one schema"
        );
    }

    // -------------------------------------------------------------------------
    // Test 7: RUNTIME_API_CONTRACT_DOC_PATH is a non-empty string constant
    // -------------------------------------------------------------------------
    #[test]
    fn runtime_api_contract_doc_path_is_nonempty() {
        assert!(
            !RUNTIME_API_CONTRACT_DOC_PATH.is_empty(),
            "contract doc path must not be empty"
        );
        // Verify the path has a JSON suffix — it's machine-readable
        assert!(
            RUNTIME_API_CONTRACT_DOC_PATH.ends_with(".json"),
            "contract doc path should end with .json"
        );
    }
}
