use oris_orchestrator::task_spec::TaskSpec;

#[test]
fn task_spec_rejects_empty_allowed_paths() {
    let spec = TaskSpec::new("issue-123", "Fix build", vec![]);
    assert!(spec.is_err());
}
