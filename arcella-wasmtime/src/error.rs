// arcella/arcella-wasmtime/src/error.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for `arcella-wasmtime` operations.
pub type Result<T> = std::result::Result<T, ArcellaWasmtimeError>;

/// Errors that can occur during Wasmtime-to-Arcella conversion.
#[derive(Error, Debug)]
pub enum ArcellaWasmtimeError {
    #[error("Component introspection error: {0}")]
    Introspection(String),

    /// IO error with associated path for better diagnostics
    #[error("I/O error at {path:?}: {source}")]
    IoWithPath {
        source: std::io::Error,
        path: PathBuf,
    },
    
    /// Invalid or missing module manifest.
    #[error("Manifest error: {0}")]
    Manifest(String),

    #[error("Wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),

}

impl From<String> for ArcellaWasmtimeError {
    fn from(s: String) -> Self {
        Self::Introspection(s)
    }
}

impl From<&str> for ArcellaWasmtimeError {
    fn from(s: &str) -> Self {
        Self::Introspection(s.into())
    }
}
