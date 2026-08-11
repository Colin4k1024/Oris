//! oris-experience-repo
//!
//! HTTP API server for Oris Experience Repository.
//!
//! Provides a REST API for external agents to query and contribute experiences
//! (genes and capsules) to the Oris experience pool.

pub mod api;
pub mod client;
pub mod control_plane;
pub mod error;
pub mod key_service;
pub mod mcp;
pub mod migration;
pub mod oen;
pub mod server;
pub mod skill_projection;

pub use client::ExperienceRepoClient;
pub use control_plane::{ExperienceControlPlane, ExperienceSearchQuery, ExperienceSearchResult};
pub use error::ExperienceRepoError;
pub use key_service::{KeyServiceError, KeyStore};
pub use oen::{OenError, OenVerifier};
pub use oris_experience_contract::{CapsuleV1, ExperienceBundleV1, GeneV1, UsageReceiptV1};
pub use server::{ExperienceRepoServer, ServerConfig};
