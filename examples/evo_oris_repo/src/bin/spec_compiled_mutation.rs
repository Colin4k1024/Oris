use evo_oris_repo::{build_demo_evo, current_git_head, proposal_diff, ExampleResult};
use oris_runtime::evolution::{
    prepare_mutation_from_spec, EvoSelectorInput as SelectorInput, SpecCompiler,
};

const SAMPLE_SPEC: &str = r#"
id: spec-evo-docs
version: "1.0"
intent: Update evolution docs example via compiled spec
signals:
  - spec docs update
  - evolution spec
constraints:
  - key: path
    value: docs/
mutation:
  strategy: docs-single-file-evolution
validation:
  - cargo check -p evo_oris_repo
"#;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("spec-compiled", 1)?;

    let spec_doc = SpecCompiler::from_yaml(SAMPLE_SPEC)?;
    let compiled = SpecCompiler::compile(&spec_doc)?;
    let selector_spec_id = compiled.mutation_intent.spec_id.clone();

    let mutation = prepare_mutation_from_spec(
        compiled,
        proposal_diff(
            "docs/evolution-spec-compiled.md",
            "Spec Compiled Evolution Example",
            "spec-compiler",
        ),
        base_revision,
    );

    let capture = evo
        .capture_mutation_with_governor(&"spec-capture-run".into(), mutation)
        .await?;
    println!(
        "captured from spec: gene_id={}, capsule_id={}, state={:?}",
        capture.gene.id, capture.capsule.id, capture.governor_decision.target_state
    );

    let replay = evo
        .replay_or_fallback_for_run(
            &"spec-replay-run".into(),
            SelectorInput {
                signals: capture.gene.signals.clone(),
                env: capture.capsule.env.clone(),
                spec_id: selector_spec_id,
                limit: 1,
            },
        )
        .await?;
    println!(
        "spec-linked replay: used_capsule={}, capsule_id={:?}, fallback_to_planner={}, reason={}",
        replay.used_capsule, replay.capsule_id, replay.fallback_to_planner, replay.reason
    );

    Ok(())
}
