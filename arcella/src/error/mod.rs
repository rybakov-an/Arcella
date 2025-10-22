// arcella/arcella/src/error/mod.rs
//
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

use arcella_wasmtime::error::ArcellaWasmtimeError;
use arcella_fs_utils::error::ArcellaUtilsError;

/// The root error type for all Arcella-specific failures.
#[derive(Error, Debug)]
pub enum ArcellaError {
    /// General-purpose error for unexpected conditions.
    #[error("Internal error: {0}")]
    Internal(String),

    /// I/O error (file not found, permission denied, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// IO error with associated path for better diagnostics
    #[error("I/O error at {path:?}: {source}")]
    IoWithPath {
        source: std::io::Error,
        path: PathBuf,
    },

    /// Failed to parse WebAssembly Text Format (`.wat`).
    #[error("WAT parsing error: {0}")]
    Wat(#[from] wat::Error),

    /// Configuration loading or parsing error.
    #[error("Config error: {0}")]
    Config(String),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error), 

    /// Task join error.
    #[error("Task join error: {0}")]
    Join(#[from] JoinError),

    /// Runtime error.
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Arcella Wasmtime error: {0}")]
    ArcellaWasmtimeError (#[from] ArcellaWasmtimeError),    

    #[error("Arcella Wasmtime error: {0}")]
    ArcellaUtilsError (#[from] ArcellaUtilsError),    

}

/// Convenient alias for `Result<T, ArcellaError>`.
///
/// Use this in internal module APIs (e.g., `runtime::install_module`).
pub type Result<T> = std::result::Result<T, ArcellaError>;

// Re-export `anyhow::Result` as `AnyResult` for top-level use (optional but clean)
// Alternatively, you can use `anyhow::Result` directly in `main.rs`
pub use anyhow::Result as AnyResult;