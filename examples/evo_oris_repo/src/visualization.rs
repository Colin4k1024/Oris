//! ASCII visualization helpers for the self-evolution demo.
//!
//! Provides rendering utilities for:
//! - Evolution pipeline stage diagrams
//! - Confidence gauges
//! - Gene pool tables

use oris_evolution::{PipelineResult, PipelineStageState};

/// Render an ASCII diagram of the evolution pipeline with stage markers.
pub fn render_evolution_cycle(result: &PipelineResult, active_stage: Option<&str>) -> String {
    let stages = [
        ("detect", "Extract signals from errors"),
        ("select", "Find matching genes"),
        ("mutate", "Generate solution proposals"),
        ("execute", "Apply in sandbox"),
        ("validate", "Verify compilation"),
        ("evaluate", "Assess quality"),
        ("solidify", "Save to gene pool"),
        ("reuse", "Mark capsule as reusable"),
    ];

    let mut diagram = String::new();
    diagram.push_str("\n");
    diagram.push_str("  ╔══════════════════════════════════════════════════════════════╗\n");
    diagram.push_str("  ║           SELF-EVOLUTION PIPELINE                         ║\n");
    diagram.push_str("  ╠══════════════════════════════════════════════════════════════╣\n");

    for (name, desc) in &stages {
        let state = result
            .stage_states
            .iter()
            .find(|s| s.stage_name == *name)
            .map(|s| &s.state);

        let state_str = match state {
            Some(PipelineStageState::Completed) => "completed".to_string(),
            Some(PipelineStageState::Failed(e)) => format!("failed: {e}"),
            Some(PipelineStageState::Running) => "running".to_string(),
            Some(PipelineStageState::Skipped(s)) => format!("skipped: {s}"),
            Some(PipelineStageState::Pending) => "pending".to_string(),
            None => "not executed".to_string(),
        };

        let is_active = active_stage == Some(*name);
        let prefix = if is_active { ">>>" } else { "   " };

        diagram.push_str(&format!(
            "  ║  {} {:<8} {:<30} {:<15} ║\n",
            prefix, name, desc, state_str
        ));
    }

    diagram.push_str("  ╚══════════════════════════════════════════════════════════════╝\n");
    diagram
}

/// Render a confidence gauge as ASCII art.
pub fn render_confidence_gauge(gene_id: &str, confidence: f32, width: usize) -> String {
    let filled = ((confidence * width as f32) as usize).min(width);
    let empty = width.saturating_sub(filled);

    let bar: String = std::iter::repeat('█')
        .take(filled)
        .chain(std::iter::repeat('░').take(empty))
        .collect();

    format!(
        "\n  Gene: {}\n  Confidence: [{}] {:.1}%\n",
        gene_id,
        bar,
        confidence * 100.0
    )
}

/// Render a gene pool table.
pub fn render_gene_pool(genes: &[(&str, f32, usize)]) -> String {
    let mut table = String::new();
    table.push_str("\n  ╔══════════════════════════════════════════════════════════╗\n");
    table.push_str("  ║                    GENE POOL                             ║\n");
    table.push_str("  ╠══════════════════════════════════════════════════════════╣\n");
    table.push_str("  ║ Gene ID                          Confidence    Uses     ║\n");
    table.push_str("  ╟──────────────────────────────────────────────────────────╢\n");

    for (id, conf, uses) in genes {
        let id_display = if id.len() > 32 { &id[..32] } else { id };
        table.push_str(&format!(
            "  ║ {:<32} {:>9.2} {:>8}    ║\n",
            id_display, conf, uses
        ));
    }

    table.push_str("  ╚══════════════════════════════════════════════════════════╝\n");
    table
}

/// Render stage execution summary.
pub fn render_stage_summary(result: &PipelineResult) -> String {
    let mut summary = String::new();
    summary.push_str("\n  Stage Results:\n");
    summary.push_str("  ┌────────────┬─────────────────────┬──────────┐\n");
    summary.push_str("  │ Stage      │ State               │ Duration │\n");
    summary.push_str("  ├────────────┼─────────────────────┼──────────┤\n");

    for stage in &result.stage_states {
        let state_str = match &stage.state {
            PipelineStageState::Completed => "Completed",
            PipelineStageState::Failed(e) => &e[..e.len().min(17)],
            PipelineStageState::Running => "Running",
            PipelineStageState::Pending => "Pending",
            PipelineStageState::Skipped(s) => &s[..s.len().min(17)],
        };
        let duration = stage
            .duration_ms
            .map(|d| format!("{}ms", d))
            .unwrap_or_else(|| "N/A".to_string());

        summary.push_str(&format!(
            "  │ {:<10} │ {:<19} │ {:<8} │\n",
            stage.stage_name, state_str, duration
        ));
    }

    summary.push_str("  └────────────┴─────────────────────┴──────────┘\n");
    summary
}

/// Print a phase header with decorative borders.
pub fn print_phase_header(phase: u32, title: &str, description: &str) {
    println!("\n");
    println!("  ╔══════════════════════════════════════════════════════════════╗");
    println!(
        "  ║  PHASE {}: {}                                       ║",
        phase, title
    );
    println!("  ║  {}", description);
    println!("  ╚══════════════════════════════════════════════════════════════╝");
    println!();
}
