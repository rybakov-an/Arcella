// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! Centralized error handling for Arcella.
//!
//! Uses `thiserror` to define structured errors and `anyhow` for convenient propagation.
//! All modules should return `Result<T, ArcellaError>` for internal logic,
//! and `anyhow::Result<T>` (aliased as `Result<T>`) for top-level functions like `main`.
//! 

use std::path::PathBuf;
use thiserror::Error;
use tokio::task::JoinError;

/// The root error type for all Arcella-specific failures.
#[derive(Error, Debug)]
pub enum ArcellaError {
    /// Failed to determine or access the user's home directory.
    #[error("Home directory not found")]
    HomeDirNotFound,

    /// I/O error (file not found, permission denied, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// WASM compilation or runtime error from Wasmtime.
    #[error("Wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),

    /// Failed to parse WebAssembly Text Format (`.wat`).
    #[error("WAT parsing error: {0}")]
    Wat(#[from] wat::Error),

    /// Invalid or missing module manifest.
    #[error("Manifest error: {0}")]
    Manifest(String),

    /// Module with the given ID was not found in the runtime.
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Instance lifecycle error (e.g., start/stop on invalid state).
    #[error("Instance error: {0}")]
    Instance(String),

    /// ALME (IPC) communication error.
    #[error("ALME error: {0}")]
    Alme(String),

    /// Configuration loading or parsing error.
    #[error("Config error: {0}")]
    Config(String),

    /// General-purpose error for unexpected conditions.
    #[error("Internal error: {0}")]
    Internal(String),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error), 

    /// Request is failed;
    #[error("JRequest error: {0}")]
    InvalidRequest(String),

    /// Task join error.
    #[error("Task join error: {0}")]
    Join(#[from] JoinError),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Runtime error.
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// IO error with associated path for better diagnostics
    #[error("I/O error at {0:?}: {1}")]
    IoWithPath (std::io::Error, PathBuf),

}

/// Convenient alias for `Result<T, ArcellaError>`.
///
/// Use this in internal module APIs (e.g., `runtime::install_module`).
pub type Result<T> = std::result::Result<T, ArcellaError>;

// Re-export `anyhow::Result` as `AnyResult` for top-level use (optional but clean)
// Alternatively, you can use `anyhow::Result` directly in `main.rs`
pub use anyhow::Result as AnyResult;