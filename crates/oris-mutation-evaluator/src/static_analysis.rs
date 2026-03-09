//! oris-mutation-evaluator/src/static_analysis.rs
//!
//! Deterministic static anti-pattern detection. Runs instantly without I/O.

use crate::types::{AntiPattern, AntiPatternKind, MutationProposal};

/// Detect anti-patterns in a mutation proposal using static analysis.
pub fn detect_anti_patterns(proposal: &MutationProposal) -> Vec<AntiPattern> {
    let mut patterns = Vec::new();

    // 1. No-op detection
    if proposal.original == proposal.proposed {
        patterns.push(AntiPattern {
            kind: AntiPatternKind::NoOpMutation,
            description: "Proposed code is identical to original".to_string(),
            is_blocking: true,
        });
    }

    // 2. Hardcoded bypass detection
    if contains_hardcoded_bypass(&proposal.original, &proposal.proposed) {
        patterns.push(AntiPattern {
            kind: AntiPatternKind::HardcodedBypass,
            description: "Hardcoded value detected in proposed code that wasn't in original"
                .to_string(),
            is_blocking: true,
        });
    }

    // 3. Test deletion detection
    if has_test_deletion(&proposal.original, &proposal.proposed) {
        patterns.push(AntiPattern {
            kind: AntiPatternKind::TestDeletion,
            description: "Test functions or attributes were removed".to_string(),
            is_blocking: true,
        });
    }

    // 4. Error suppression detection (soft anti-pattern)
    if introduces_error_suppression(&proposal.proposed, &proposal.original) {
        patterns.push(AntiPattern {
            kind: AntiPatternKind::ErrorSuppression,
            description:
                "New error suppression patterns introduced (unwrap_or_default, let _, etc.)"
                    .to_string(),
            is_blocking: false,
        });
    }

    // 5. Blast radius violation (soft anti-pattern)
    if is_blast_radius_violation(
        &proposal.original,
        &proposal.proposed,
        proposal.signals.len(),
    ) {
        patterns.push(AntiPattern {
            kind: AntiPatternKind::BlastRadiusViolation,
            description: "Change scope significantly exceeds what the signal warrants".to_string(),
            is_blocking: false,
        });
    }

    patterns
}

/// Check if proposed code contains hardcoded values not in original.
fn contains_hardcoded_bypass(original: &str, proposed: &str) -> bool {
    // Simple heuristic: look for numeric literals in proposed that aren't in original
    // This is a simplified check - real implementation would need AST parsing
    let proposed_lines: Vec<_> = proposed.lines().collect();
    let original_lines: Vec<_> = original.lines().collect();

    for p_line in &proposed_lines {
        // Skip lines that exist in original
        if original_lines.contains(p_line) {
            continue;
        }

        // Look for suspicious patterns
        if p_line.contains("return ") && p_line.matches(|c: char| c.is_numeric()).count() > 0 {
            // This is a rough heuristic - proper impl would need actual parsing
            return true;
        }
    }
    false
}

/// Check if tests were deleted.
fn has_test_deletion(original: &str, proposed: &str) -> bool {
    let original_tests = count_tests(original);
    let proposed_tests = count_tests(proposed);
    proposed_tests < original_tests
}

fn count_tests(code: &str) -> usize {
    let mut count = 0;
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[test]") || trimmed.starts_with("#[cfg(test)]") {
            count += 1;
        }
        if trimmed.contains("fn test_") || trimmed.contains("#[tokio::test]") {
            count += 1;
        }
    }
    count
}

/// Check if new error suppression patterns were introduced.
fn introduces_error_suppression(proposed: &str, original: &str) -> bool {
    let suppression_patterns = [
        "unwrap_or_default",
        "unwrap_or",
        "let _ =",
        ".ok()",
        ".expect()",
        "unwrap()",
    ];

    for pattern in &suppression_patterns {
        let proposed_count = proposed.matches(pattern).count();
        let original_count = original.matches(pattern).count();
        if proposed_count > original_count {
            return true;
        }
    }
    false
}

/// Check if blast radius is violated (change too large for signal count).
fn is_blast_radius_violation(original: &str, proposed: &str, signal_count: usize) -> bool {
    if signal_count == 0 {
        return false;
    }

    let original_len = original.len();
    let proposed_len = proposed.len();
    let max_len = original_len.max(proposed_len);

    if max_len == 0 {
        return false;
    }

    let change_ratio = (original_len as f64 - proposed_len as f64).abs() / max_len as f64;

    // If change is > 60% of file size but only triggered by ≤2 signals
    change_ratio > 0.6 && signal_count <= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_op_detection() {
        let proposal = MutationProposal {
            id: uuid::Uuid::new_v4(),
            intent: "fix".to_string(),
            original: "fn foo() {}".to_string(),
            proposed: "fn foo() {}".to_string(),
            signals: vec![],
            source_gene_id: None,
        };
        let patterns = detect_anti_patterns(&proposal);
        assert!(patterns
            .iter()
            .any(|p| matches!(p.kind, AntiPatternKind::NoOpMutation)));
    }

    #[test]
    fn test_test_deletion() {
        let proposal = MutationProposal {
            id: uuid::Uuid::new_v4(),
            intent: "fix".to_string(),
            original: r#"
#[test]
fn test_foo() { assert!(true); }
fn foo() { }
"#
            .to_string(),
            proposed: r#"
fn foo() { }
"#
            .to_string(),
            signals: vec![],
            source_gene_id: None,
        };
        let patterns = detect_anti_patterns(&proposal);
        assert!(patterns
            .iter()
            .any(|p| matches!(p.kind, AntiPatternKind::TestDeletion)));
    }
}
