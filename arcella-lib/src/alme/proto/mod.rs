// arcella-lib/src/alme/proto/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! ALME (Arcella Local Management Extensions) protocol definitions.
//!
//! This crate defines the shared request/response structures used by both
//! the Arcella daemon (server) and clients (e.g., CLI, GUI, tests).

use serde::{Deserialize, Serialize};

/// An ALME request sent by a client.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AlmeRequest {
    /// Command name in hierarchical format, e.g., `"ping"`, `"module:list"`, `"log:tail"`.
    pub cmd: String,

    /// Optional arguments for the command.
    #[serde(default)]
    pub args: serde_json::Value,
}

/// An ALME response returned by the server.
#[derive(Serialize, Deserialize, Debug)]
pub struct AlmeResponse {
    /// Whether the command succeeded.
    pub success: bool,

    /// Human-readable message (e.g., "pong", "Arcella runtime is active").
    pub message: String,

    /// Optional structured data (e.g., status details, log lines, module list).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl AlmeResponse {
    /// Create a successful response.
    pub fn success(message: &str, data: Option<serde_json::Value>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data,
        }
    }

    /// Create an error response.
    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}