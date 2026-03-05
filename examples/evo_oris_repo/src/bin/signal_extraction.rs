use evo_oris_repo::ExampleResult;
use oris_runtime::evolution::{extract_deterministic_signals, SignalExtractionInput};

fn main() -> ExampleResult<()> {
    let extracted = extract_deterministic_signals(&SignalExtractionInput {
        patch_diff: "diff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1,2 @@\n pub fn demo() {}\n+pub fn recover() {}\n".into(),
        intent: "Fix missing symbol E0425 in evolution workflow".into(),
        expected_effect: "cargo check passes for evo examples".into(),
        declared_signals: vec!["missing symbol".into(), "E0425".into()],
        changed_files: vec!["src/lib.rs".into(), "docs/evolution.md".into()],
        validation_success: true,
        validation_logs: "error[E0425]: cannot find value in this scope".into(),
        stage_outputs: vec!["cargo check -p evo_oris_repo".into()],
    });

    println!("signal hash: {}", extracted.hash);
    println!("signal count: {}", extracted.values.len());
    println!("signals: {:?}", extracted.values);

    Ok(())
}
