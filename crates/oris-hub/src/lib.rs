//! oris-hub
//!
//! Oris Experience Repository Hub — a lightweight central registry for node
//! discovery, federated gene queries, and subscription-based push notifications.

pub mod api;
pub mod dashboard;
pub mod discovery;
pub mod error;
pub mod federation;
pub mod middleware;
pub mod registry;
pub mod server;
pub mod subscription;
pub mod validation;

pub use error::HubError;
pub use server::{HubConfig, HubServer};
