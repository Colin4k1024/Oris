use evo_oris_repo::{
    build_demo_evo, current_git_head, proposal_diff, proposal_for, single_path, ExampleResult,
};
use oris_runtime::agent_contract::{
    AgentTask, HumanApproval, ProposalTarget, SupervisedDevloopRequest,
};

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("supervised-devloop", 1)?;

    let docs_task = AgentTask {
        id: "docs-approved-task".into(),
        description: "Add a supervised docs note for devloop".into(),
    };
    let docs_target = ProposalTarget::Paths(vec!["docs/evolution-supervised-demo.md".into()]);
    let docs_proposal = proposal_for(
        &docs_task,
        &docs_target,
        "docs-agent",
        "record supervised execution sample",
    );

    let awaiting = evo
        .run_supervised_devloop(
            &"supervised-awaiting".into(),
            &SupervisedDevloopRequest {
                task: docs_task.clone(),
                proposal: docs_proposal.clone(),
                approval: HumanApproval {
                    approved: false,
                    approver: Some("maintainer".into()),
                    note: Some("waiting for sign-off".into()),
                },
            },
            proposal_diff(
                single_path(&docs_target),
                "Supervised Devloop Awaiting Approval",
                "docs-agent",
            ),
            base_revision.clone(),
        )
        .await?;
    println!(
        "awaiting approval: task_id={}, status={:?}, summary={}",
        awaiting.task_id, awaiting.status, awaiting.summary
    );

    let rejected_task = AgentTask {
        id: "out-of-scope-task".into(),
        description: "Attempt a non-doc task that policy should reject".into(),
    };
    let rejected_target = ProposalTarget::Paths(vec!["src/lib.rs".into()]);
    let rejected_proposal = proposal_for(
        &rejected_task,
        &rejected_target,
        "code-agent",
        "modify runtime code path",
    );
    let rejected = evo
        .run_supervised_devloop(
            &"supervised-rejected".into(),
            &SupervisedDevloopRequest {
                task: rejected_task,
                proposal: rejected_proposal,
                approval: HumanApproval {
                    approved: true,
                    approver: Some("maintainer".into()),
                    note: Some("approved but outside bounded policy".into()),
                },
            },
            proposal_diff(
                single_path(&rejected_target),
                "Supervised Devloop Rejected",
                "code-agent",
            ),
            base_revision.clone(),
        )
        .await?;
    println!(
        "policy rejection: task_id={}, status={:?}, summary={}",
        rejected.task_id, rejected.status, rejected.summary
    );

    let executed = evo
        .run_supervised_devloop(
            &"supervised-executed".into(),
            &SupervisedDevloopRequest {
                task: docs_task,
                proposal: docs_proposal,
                approval: HumanApproval {
                    approved: true,
                    approver: Some("maintainer".into()),
                    note: Some("final execution approval".into()),
                },
            },
            proposal_diff(
                single_path(&docs_target),
                "Supervised Devloop Executed",
                "docs-agent",
            ),
            base_revision,
        )
        .await?;
    println!(
        "approved execution: task_id={}, status={:?}, feedback={:?}",
        executed.task_id, executed.status, executed.execution_feedback
    );

    Ok(())
}
