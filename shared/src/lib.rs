//! Shared domain models and DTOs for the Nest project.
//!
//! These types are intentionally transport-agnostic so both the Nest server
//! and (in later phases) the Bird client can depend on them.

pub mod domain;

pub use domain::{Bird, Clutch, Egg, Flock, Platform, SyncStatus};
