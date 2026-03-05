use evo_oris_repo::{build_demo_evo, ExampleResult};
use oris_runtime::agent_contract::{
    AgentRole, CoordinationPlan, CoordinationPrimitive, CoordinationTask,
};

fn print_result(label: &str, result: &oris_runtime::agent_contract::CoordinationResult) {
    println!("=== {label} ===");
    println!("summary: {}", result.summary);
    println!("completed: {:?}", result.completed_tasks);
    println!("failed: {:?}", result.failed_tasks);
    println!("messages: {}", result.messages.len());
}

fn main() -> ExampleResult<()> {
    let evo = build_demo_evo("coordination-matrix", 1)?;

    let sequential = CoordinationPlan {
        root_goal: "sequential docs patch".into(),
        primitive: CoordinationPrimitive::Sequential,
        tasks: vec![
            CoordinationTask {
                id: "planner-1".into(),
                role: AgentRole::Planner,
                description: "plan task breakdown".into(),
                depends_on: vec![],
            },
            CoordinationTask {
                id: "coder-1".into(),
                role: AgentRole::Coder,
                description: "implement docs patch".into(),
                depends_on: vec!["planner-1".into()],
            },
            CoordinationTask {
                id: "optimizer-1".into(),
                role: AgentRole::Optimizer,
                description: "optimize phrasing".into(),
                depends_on: vec!["coder-1".into()],
            },
        ],
        timeout_ms: 120_000,
        max_retries: 1,
    };

    let parallel_with_retry = CoordinationPlan {
        root_goal: "parallel coding with one retry".into(),
        primitive: CoordinationPrimitive::Parallel,
        tasks: vec![
            CoordinationTask {
                id: "planner-2".into(),
                role: AgentRole::Planner,
                description: "prepare plan".into(),
                depends_on: vec![],
            },
            CoordinationTask {
                id: "coder-2".into(),
                role: AgentRole::Coder,
                description: "fail-once then recover".into(),
                depends_on: vec!["planner-2".into()],
            },
            CoordinationTask {
                id: "repair-2".into(),
                role: AgentRole::Repair,
                description: "repair path after coder failure".into(),
                depends_on: vec!["coder-2".into()],
            },
        ],
        timeout_ms: 120_000,
        max_retries: 1,
    };

    let conditional_skip = CoordinationPlan {
        root_goal: "conditional skip chain".into(),
        primitive: CoordinationPrimitive::Conditional,
        tasks: vec![
            CoordinationTask {
                id: "planner-3".into(),
                role: AgentRole::Planner,
                description: "force-fail at planning phase".into(),
                depends_on: vec![],
            },
            CoordinationTask {
                id: "coder-3".into(),
                role: AgentRole::Coder,
                description: "this task should be skipped".into(),
                depends_on: vec!["planner-3".into()],
            },
            CoordinationTask {
                id: "optimizer-3".into(),
                role: AgentRole::Optimizer,
                description: "this task should also be skipped".into(),
                depends_on: vec!["coder-3".into()],
            },
        ],
        timeout_ms: 120_000,
        max_retries: 0,
    };

    let sequential_result = evo.coordinate(sequential);
    let parallel_result = evo.coordinate(parallel_with_retry);
    let conditional_result = evo.coordinate(conditional_skip);

    print_result("sequential", &sequential_result);
    print_result("parallel-with-retry", &parallel_result);
    print_result("conditional-skip", &conditional_result);

    Ok(())
}
