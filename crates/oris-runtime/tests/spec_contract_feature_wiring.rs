#![cfg(feature = "spec-contract")]

fn assert_type<T>() {}

#[test]
fn spec_contract_standard_feature_paths_resolve() {
    let doc = oris_runtime::spec_contract::SpecCompiler::from_yaml(
        r#"
id: repair-test
version: "0.1"
intent: "repair a recurring test failure"
signals:
  - "test failed"
mutation:
  strategy: "update assertion"
validation:
  - "cargo test"
"#,
    )
    .expect("sample OUSL document should parse");

    let plan = oris_runtime::spec_contract::SpecCompiler::compile(&doc)
        .expect("sample OUSL document should compile");
    assert_eq!(plan.mutation_intent.spec_id.as_deref(), Some("repair-test"));
    assert_eq!(plan.validation_profile, "cargo test");

    assert_type::<oris_runtime::spec_contract::SpecDocument>();
    assert_type::<oris_runtime::spec_contract::SpecMutation>();
    assert_type::<oris_runtime::spec_contract::CompiledMutationPlan>();
    assert_type::<oris_runtime::spec_contract::SpecCompileError>();
}
