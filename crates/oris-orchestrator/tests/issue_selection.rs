use oris_orchestrator::github_adapter::RemoteIssue;
use oris_orchestrator::issue_selection::select_next_issue;

fn issue(
    number: u64,
    title: &str,
    labels: &[&str],
    milestone_number: Option<u64>,
    state: &str,
) -> RemoteIssue {
    RemoteIssue {
        number,
        title: title.to_string(),
        state: state.to_string(),
        url: format!("https://github.com/Colin4k1024/Oris/issues/{}", number),
        labels: labels.iter().map(|label| (*label).to_string()).collect(),
        milestone_number,
        milestone_title: milestone_number.map(|value| format!("Sprint {}", value)),
        created_at: Some("2026-03-05T14:00:00Z".to_string()),
    }
}

#[test]
fn issue_selection_prefers_p0_then_milestone_then_issue_number() {
    let issues = vec![
        issue(109, "[RFC] MCP support", &["enhancement"], None, "OPEN"),
        issue(111, "[EVMAP-02]", &["priority/P0"], Some(7), "OPEN"),
        issue(110, "[EVMAP-01]", &["priority/P0"], Some(7), "OPEN"),
        issue(115, "[EVMAP-06]", &["priority/P0"], Some(8), "OPEN"),
        issue(116, "[EVMAP-07]", &["priority/P1"], Some(9), "OPEN"),
    ];

    let selected = select_next_issue(&issues).expect("expected selected issue");
    assert_eq!(selected.number, 110);
}

#[test]
fn issue_selection_skips_blocked_and_closed_issues() {
    let issues = vec![
        issue(
            200,
            "[EVMAP-X]",
            &["priority/P0", "blocked"],
            Some(1),
            "OPEN",
        ),
        issue(201, "[EVMAP-Y]", &["priority/P0"], Some(1), "CLOSED"),
        issue(202, "[EVMAP-Z]", &["priority/P1"], Some(2), "OPEN"),
    ];

    let selected = select_next_issue(&issues).expect("expected selected issue");
    assert_eq!(selected.number, 202);
}

#[test]
fn issue_selection_returns_none_without_p0_or_p1() {
    let issues = vec![
        issue(300, "[RFC] roadmap", &["enhancement"], None, "OPEN"),
        issue(301, "Other", &["type/feature"], Some(1), "OPEN"),
    ];
    assert!(select_next_issue(&issues).is_none());
}
