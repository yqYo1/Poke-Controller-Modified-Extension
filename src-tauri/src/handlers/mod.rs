//! Handlers sub-module — all HTTP request handlers organized by domain.
//!
//! This module provides shared types used across handlers.

// Shared request types used in multiple handlers
pub use serde::{Deserialize, Serialize};
pub use utoipa::ToSchema;

/// A request containing just a name — used for command loading, profile activation, etc.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NameRequest {
    pub name: String,
}

pub mod camera;
pub mod commands;
pub mod controller;
pub mod core;
pub mod input;
pub mod notifications;
pub mod profile;
pub mod serial;
pub mod websocket;
