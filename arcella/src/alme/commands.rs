// arcella/arcella/src/alme/command.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! Command dispatching logic for the ALME (Arcella Local Management Extensions) protocol.
//!
//! This module implements the core command router that maps incoming ALME commands
//! (e.g., `"ping"`, `"status"`, `"log:tail"`) to their respective handler functions.
//! Each handler interacts with the shared [`ArcellaRuntime`] or other subsystems
//! (e.g., the logging buffer) and returns a structured [`AlmeResponse`].
//!
//! The entry point is [`dispatch_command`], which is called by the ALME server
//! for every valid incoming request.

use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use alme_proto::{AlmeRequest, AlmeResponse};

use crate::log;
use crate::runtime::ArcellaRuntime;

/// Dispatches an ALME command to the appropriate handler function.
///
/// This function acts as the central command router for the ALME server.
/// It matches the command name (`cmd`) against a set of known operations
/// and delegates execution to the corresponding async handler.
///
/// Unknown commands result in an error response.
///
/// # Arguments
///
/// * `cmd` — The command name in hierarchical format (e.g., `"module:install"`, `"log:tail"`).
/// * `args` — Optional command arguments, represented as a generic JSON value.
/// * `runtime` — A thread-safe shared reference to the main Arcella runtime instance.
///
/// # Returns
///
/// An [`AlmeResponse`] indicating success or failure, optionally carrying structured data.
pub async fn dispatch_command(
    cmd: &str,
    args: &Value,
    runtime: &Arc<RwLock<ArcellaRuntime>>,
) -> AlmeResponse {
    match cmd {
        "ping" => handle_ping(),
        "status" => handle_status(runtime).await,
        "log:tail" => handle_log_tail(args).await,
        "module:list" => handle_module_list(runtime).await,
        // ... other command
        _ => AlmeResponse::error(&format!("Unknown command: {}", cmd)),
    }
}

/// Handles the `"ping"` ALME command.
///
/// This is a lightweight health-check command that verifies the ALME server is responsive.
/// It requires no arguments and returns a simple `"pong"` message.
///
/// # Returns
///
/// A successful [`AlmeResponse`] with message `"pong"` and no data.
fn handle_ping() -> AlmeResponse {
    AlmeResponse::success("pong", None)
}

/// Handles the `"status"` ALME command.
///
/// Retrieves high-level diagnostic information about the Arcella runtime,
/// including process ID, uptime, version, and configuration paths.
///
/// # Arguments
///
/// * `runtime` — Shared access to the runtime state.
///
/// # Returns
///
/// A successful [`AlmeResponse`] containing a JSON object with fields:
/// - `version`: current Arcella version (from `CARGO_PKG_VERSION`)
/// - `pid`: OS process ID
/// - `start_time`: RFC3339-formatted startup timestamp
/// - `uptime`: runtime duration in seconds
/// - `socket_path`: filesystem path of the ALME Unix socket
///
/// Returns an error response if the runtime status cannot be retrieved
/// (e.g., due to a poisoned lock).
async fn handle_status(
    runtime: &Arc<RwLock<ArcellaRuntime>>,
) -> AlmeResponse {
    
    let runtime_guard = runtime.read().await;

    let runtime_status = match runtime_guard.status(){
        Ok(status) => status,
        Err(e) => {
            let message = format!("Arcella runtime is fault: {} ", e);
            tracing::debug!("{}", message);
            return AlmeResponse::error(&message)
        }
    };

    let start_time_rfc3339 = runtime_status.start_time.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "<invalid-timestamp>".to_string());

    let data = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "pid": runtime_status.pid,
        "start_time": format!("{}", start_time_rfc3339),
        "uptime": runtime_status.uptime.as_secs(),
        "socket_path": runtime_guard.config.socket_path.to_string_lossy(),
        "worker_groups": "",
        "modules": "",
    });

    AlmeResponse::success("Arcella runtime is active", Some(data))

}

/// Handles the `"log:tail"` ALME command.
///
/// Retrieves the most recent log entries from the in-memory ring buffer
/// (populated by the tracing layer when `alme_buffer_size > 0` in `tracing.cfg`).
///
/// # Arguments
///
/// * `args` — Expected to contain an optional `"n"` field (unsigned integer)
///            specifying the number of log lines to return. Defaults to 100.
///
/// # Returns
///
/// A successful [`AlmeResponse`] containing a JSON object with a `"lines"` array
/// of log strings (most recent first). Returns an empty array if the buffer is
/// disabled or uninitialized.
async fn handle_log_tail(args: &Value) -> AlmeResponse {
    let n = args.get("n")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(100); // default 100 lines

    let lines = log::get_recent_logs(n);

    let data = serde_json::json!({
        "lines": lines
    });

    AlmeResponse::success("Log tail retrieved", Some(data))
}

/// Handles the `"module:list"` ALME command.
///
/// Returns a list of all currently installed and/or active WebAssembly modules.
///
/// # Arguments
///
/// * `runtime` — Shared access to the runtime state (not yet used).
///
/// # Returns
///
/// A successful [`AlmeResponse`] containing a JSON array of module descriptors.
/// Currently returns an empty array as module management is not yet implemented.
///
/// # TODO
///
/// Implement actual module enumeration by querying the runtime's module registry.
async fn handle_module_list(
    _runtime: &Arc<RwLock<ArcellaRuntime>>,
) -> AlmeResponse {
    // TODO: реализовать
    AlmeResponse::success("Module list", Some(serde_json::json!([])))
}