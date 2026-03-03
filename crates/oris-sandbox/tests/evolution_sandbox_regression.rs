use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use oris_evolution::{
    ArtifactEncoding, MutationArtifact, MutationIntent, MutationTarget, PreparedMutation, RiskLevel,
};
use oris_sandbox::{LocalProcessSandbox, Sandbox, SandboxError, SandboxPolicy};

fn unique_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "oris-sandbox-regression-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn temp_workspace() -> PathBuf {
    let root = unique_path("workspace");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn hello() {}\n").unwrap();
    root
}

fn escape_mutation() -> PreparedMutation {
    PreparedMutation {
        intent: MutationIntent {
            id: "mutation-escape".into(),
            intent: "touch manifest".into(),
            target: MutationTarget::Paths {
                allow: vec!["src".into()],
            },
            expected_effect: "must stay inside src".into(),
            risk: RiskLevel::Low,
            signals: vec!["sandbox-boundary".into()],
            spec_id: None,
        },
        artifact: MutationArtifact {
            encoding: ArtifactEncoding::UnifiedDiff,
            payload: "\
diff --git a/Cargo.toml b/Cargo.toml
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/Cargo.toml
@@ -0,0 +1 @@
+[package]
"
            .into(),
            base_revision: Some("HEAD".into()),
            content_hash: "hash".into(),
        },
    }
}

#[tokio::test]
async fn rejects_escape_patch_before_creating_sandbox_workspace() {
    let workspace = temp_workspace();
    let temp_root = unique_path("sandbox-root");
    fs::create_dir_all(&temp_root).unwrap();
    let sandbox = LocalProcessSandbox::new("run-regression", &workspace, &temp_root);

    let result = sandbox
        .apply(&escape_mutation(), &SandboxPolicy::default())
        .await;
    let sandbox_dir = temp_root.join("run-regression").join("mutation-escape");

    assert!(matches!(result, Err(SandboxError::TargetViolation(_))));
    assert!(
        !sandbox_dir.exists(),
        "target violations should fail before a temp sandbox is created"
    );
    assert!(
        !workspace.join("Cargo.toml").exists(),
        "rejected patches must not touch the source workspace"
    );
}
