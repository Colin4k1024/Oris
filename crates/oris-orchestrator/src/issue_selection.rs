use crate::github_adapter::RemoteIssue;

const PRIORITY_P0: &str = "priority/p0";
const PRIORITY_P1: &str = "priority/p1";
const BLOCKED_LABELS: [&str; 4] = ["blocked", "waiting", "duplicate", "status:blocked"];

pub fn select_next_issue(issues: &[RemoteIssue]) -> Option<RemoteIssue> {
    let mut candidates = issues
        .iter()
        .filter(|issue| issue.state.eq_ignore_ascii_case("OPEN"))
        .filter(|issue| !is_rfc(issue))
        .filter(|issue| !has_blocked_label(issue))
        .filter(|issue| priority_rank(issue).is_some())
        .cloned()
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        priority_rank(left)
            .unwrap_or(u8::MAX)
            .cmp(&priority_rank(right).unwrap_or(u8::MAX))
            .then_with(|| {
                left.milestone_number
                    .unwrap_or(u64::MAX)
                    .cmp(&right.milestone_number.unwrap_or(u64::MAX))
            })
            .then_with(|| left.number.cmp(&right.number))
    });

    candidates.into_iter().next()
}

fn is_rfc(issue: &RemoteIssue) -> bool {
    issue
        .title
        .trim_start()
        .to_ascii_uppercase()
        .starts_with("[RFC]")
}

fn has_blocked_label(issue: &RemoteIssue) -> bool {
    issue.labels.iter().any(|label| {
        let normalized = label.trim().to_ascii_lowercase();
        BLOCKED_LABELS.contains(&normalized.as_str())
    })
}

fn priority_rank(issue: &RemoteIssue) -> Option<u8> {
    issue.labels.iter().find_map(|label| {
        let normalized = label.trim().to_ascii_lowercase();
        if normalized == PRIORITY_P0 {
            Some(0)
        } else if normalized == PRIORITY_P1 {
            Some(1)
        } else {
            None
        }
    })
}
