//! oris-experience-repo
//!
//! HTTP API server for Oris Experience Repository.
//!
//! Provides a REST API for external agents to query and contribute experiences
//! (genes and capsules) to the Oris experience pool.

pub mod api;
pub mod client;
pub mod error;
pub mod server;

pub use client::ExperienceRepoClient;
pub use error::ExperienceRepoError;
pub use server::{ExperienceRepoServer, ServerConfig};
