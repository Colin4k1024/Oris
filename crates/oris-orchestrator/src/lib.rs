//! Autonomous orchestration loop for Oris evolution.
//!
//! This crate coordinates the end-to-end pipeline from issue intake through
//! mutation proposal, evidence collection, release gating, and GitHub delivery.
//! It bridges the evolution kernel with external systems (GitHub, CI) and
//! enforces human-in-the-loop approval at configured checkpoints.
//!
//! # Modules
//!
//! - **autonomous_loop** — Main loop driving detect → plan → mutate → validate → deliver
//! - **acceptance_gate** — Admission logic for incoming tasks
//! - **pipeline_orchestrator** — Coordinates pipeline stages
//! - **task_planner** — Bounded planning contracts for mutation tasks
//! - **evidence** — Evidence bundle assembly for review
//! - **github_delivery** — Branch, PR, and review receipt delivery
//! - **release_gate** — Pre-release validation checks
//! - **publish_gate** — Crate publish readiness checks

pub mod acceptance_gate;
pub mod autonomous_loop;
pub mod autonomous_release;
pub mod coordinator;
pub mod evidence;
pub mod github_adapter;
pub mod github_delivery;
pub mod issue_selection;
pub mod loop_adapters;
pub mod pipeline_orchestrator;
pub mod proposal_generator;
pub mod publish_gate;
#[cfg(feature = "release-automation-experimental")]
pub mod release_executor;
pub mod release_gate;
pub mod runtime_client;
pub mod state;
pub mod task_planner;
pub mod task_spec;
