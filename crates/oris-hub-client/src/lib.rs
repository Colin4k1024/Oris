//! oris-hub-client
//!
//! Client SDK for connecting to the Oris Experience Repository Hub.
//! Provides registration, heartbeat, discovery, and federated search.

pub mod client;
pub mod error;

pub use client::{HubClient, HubClientConfig};
pub use error::HubClientError;
