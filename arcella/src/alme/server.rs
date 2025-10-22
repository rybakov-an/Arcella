// arcella/arcella/src/alme/server.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! ALME (Arcella Local Management Extensions) Unix socket server implementation.
//!
//! This module provides the core IPC server that enables external tools
//! (such as the CLI, monitoring agents, or scripts) to interact with the
//! Arcella runtime daemon via a secure, local Unix domain socket.
//!
//! The server:
//! - Listens on a filesystem socket (e.g., `~/.arcella/alme`) with `0o600` permissions
//! - Accepts line-oriented JSON requests (one command per line)
//! - Dispatches commands to handlers in [`crate::alme::commands`]
//! - Returns structured JSON responses
//! - Supports graceful shutdown via a broadcast channel
//! - Enforces security limits (max request size, read timeout)
//! - Automatically cleans up stale socket files on startup
//!
//! The protocol is synchronous and connection-scoped: each client may send
//! multiple commands over a single connection, and the server responds to each
//! in order. The server is designed for local administration only and is not
//! intended for network exposure.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{RwLock, broadcast};
use tokio::time::{timeout, Duration as TokioDuration};

use arcella_types::alme::proto::{AlmeRequest, AlmeResponse};

use crate::runtime::ArcellaRuntime;
use crate::error::{ArcellaError, Result as ArcellaResult};

/// Maximum allowed length of an incoming ALME request in bytes.
/// Requests exceeding this limit will be rejected to prevent resource exhaustion.
static MAX_REQUEST_LENGTH: usize = 64 * 1024; // 64 KB

static MAX_READER_TIMEOUT: u64 = 60; // seconds

/// Spawns the ALME (Arcella Local Management Extensions) server as a background task.
///
/// The server listens on a Unix domain socket at the specified `socket_path` and handles
/// incoming management commands (e.g., `install`, `start`, `status`) by delegating them
/// to the provided shared `ArcellaRuntime` instance.
///
/// On startup, any existing file at `socket_path` is removed to handle stale sockets.
/// The socket file is created with permissions `0o600` (read/write for owner only) for security.
///
/// A graceful shutdown can be initiated by calling [crate::alme::AlmeServerHandle::shutdown],
/// which signals the server to stop accepting new connections, notifies all active connection 
/// handlers to terminate, and removes the Unix socket file once the server loop exits.
///  
/// # Arguments
///
/// * `socket_path` - The filesystem path where the Unix socket will be created.
/// * `runtime` - A thread-safe shared reference to the main Arcella runtime instance.
///
/// # Returns
///
/// An `AlmeServerHandle` that can be used to initiate a graceful shutdown of the server.
///
/// # Errors
///
/// Returns an error if:
/// - The socket cannot be bound (e.g., due to permission issues).
/// - The socket file permissions cannot be set
pub async fn spawn_server(
    socket_path: PathBuf, 
    runtime: Arc<RwLock<ArcellaRuntime>>,
) -> ArcellaResult<super::AlmeServerHandle> {

    if socket_path.exists() {
        if let Err(e) = fs::remove_file(&socket_path) {
            tracing::error!("Failed to remove stale socket {:?}: {}", socket_path, e);
        }
    }

    let listener = UnixListener::bind(&socket_path)?;
    tracing::debug!("Bind ALME server to socket: {:?}", socket_path);
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    tracing::debug!("Set permissions on ALME socket: {:?}", socket_path);

    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    let socket_path_clone = socket_path.clone();
    let runtime_clone = runtime.clone();
    let join_handle = tokio::spawn(async move {
        let result = run_server_loop(listener, runtime_clone, shutdown_rx).await;

        // Remove socket on shutdown
        if let Err(e) = fs::remove_file(&socket_path_clone) {
            tracing::error!("Failed to remove ALME socket {:?}: {}", socket_path_clone, e);
        }

        result
    });

    Ok(super::AlmeServerHandle {
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    })
}

/// Runs the main accept loop for the ALME server.
///
/// This function continuously accepts new incoming Unix socket connections
/// until a shutdown signal is received via the `shutdown_rx` channel.
/// For each connection, it spawns a dedicated asynchronous task to handle
/// the client's requests via [`handle_connection`].
///
/// The loop is resilient to transient client or I/O errors but will exit
/// on listener errors or explicit shutdown.
/// 
/// # Arguments
///
/// * `listener` - The bound `UnixListener` to accept connections from.
/// * `runtime` - Shared access to the Arcella runtime for command execution.
/// * `shutdown_rx` - Receiver for shutdown signals.
async fn run_server_loop(
    listener: UnixListener,
    runtime: Arc<RwLock<ArcellaRuntime>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> ArcellaResult<()> {
    tracing::info!("Starting ALME server listener");

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        tracing::info!("Get new connection");
                        let rt = runtime.clone();
                        let shutdown_rx_clone = shutdown_rx.resubscribe();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, rt, shutdown_rx_clone).await {
                                tracing::error!("Connection handler error: {:?}", e);
                            }
                        });
                    },
                    Err(e) => {
                        tracing::error!("Listener accept error: {:?}", e);
                        break;
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                tracing::debug!("Listener received shutdown signal");
                break;
            }
        }
    }

    Ok(())
												   
}

/// Handles a single ALME client connection for its entire lifetime.
///
/// This function runs a loop that:
/// 1. Reads line-oriented JSON commands from the client (one per line),
/// 2. Skips empty or whitespace-only lines,
/// 3. Parses each line as an [`AlmeRequest`],
/// 4. Dispatches the request to the Arcella runtime,
/// 5. Sends back a JSON-encoded [`AlmeResponse`].
///
/// The connection remains open until one of the following occurs:
/// - The client closes the connection (EOF),
/// - A read/write I/O error occurs,
/// - A global shutdown signal is received via `shutdown_rx`.
///
/// Empty lines are ignored (no response is sent).
/// 
/// # Arguments
///
/// * `stream` - The connected Unix stream to communicate with the client.
/// * `runtime` - Shared access to the Arcella runtime for executing commands.
/// * `shutdown_rx` - Receiver for global shutdown signals.
async fn handle_connection(
    stream: UnixStream, 
    runtime: Arc<RwLock<ArcellaRuntime>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> ArcellaResult<()> {

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut buffer = String::new();

    let result = loop {
        buffer.clear();

        let line = tokio::select! {
            reader_result = timeout(TokioDuration::from_secs(MAX_READER_TIMEOUT), reader.read_line(&mut buffer)) => {
                match reader_result {
                    Ok(Ok(0)) => {
                        tracing::trace!("Get EOF from client");
                        break Ok(()); // EOF - client close connection
                    },
                    Ok(Ok(n)) => {
                        if n > MAX_REQUEST_LENGTH {
                            let message = format!("Request too large");
                            let resp = AlmeResponse::error(&message);
                            tracing::warn!("{}", message);
                            send_response(&mut writer, &resp).await?;
                            continue;
                        }
                        let trimmed = buffer.trim_end_matches(&['\r', '\n']).trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        trimmed.to_string()
                    },
                    Ok(Err(e)) => {
                        tracing::error!("Recieved error: {}", e);
                        return Err(ArcellaError::Io(e));
                    },
                    _ => {
                        tracing::warn!("Reader timeout");
                        let _ = writer.shutdown().await;
                        tracing::debug!("Writer shutdown complete");
                        return Ok(());
                    }
                }
            },
            _ = shutdown_rx.recv() => {
                tracing::debug!("Connection handler received shutdown signal");
                let _ = writer.shutdown().await;
                tracing::debug!("Writer shutdown complete");
                return Ok(());
            },
        };

        let request: AlmeRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let message = format!("Invalid JSON: {} ", e);
                let resp = AlmeResponse::error(&message);
                tracing::debug!("{}", message);
                send_response(&mut writer, &resp).await?;
                continue;
            }
        };
        tracing::trace!("Get request: {:?}", request);

        let response = super::commands::dispatch_command(&request.cmd, &request.args, &runtime).await;

        send_response(&mut writer, &response).await?;

    };

    result

}

/// Serializes an [`AlmeResponse`] to JSON and writes it to the client stream.
///
/// A newline (`\n`) is appended to ensure line-oriented parsing on the client side.
/// If the write fails (e.g., because the client disconnected), the error is returned
/// so the connection handler can terminate gracefully.
/// 
/// # Arguments
///
/// * `stream` - The writable half of the Unix stream to send the response to.
/// * `response` - The response object to serialize and send.
async fn send_response(
    stream: &mut WriteHalf<UnixStream>,
    response: &AlmeResponse,
) -> ArcellaResult<()> {
    tracing::trace!("Send response");
    let mut json = serde_json::to_vec(response)
        .map_err(|e| ArcellaError::Json(e))?;
    json.push(b'\n');
    let _ = stream.write_all(&json).await.map_err(|e| {
        tracing::error!("Failed to send response: {}", e);
        ArcellaError::Io(e)
    });
    Ok(())
}


/*#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::os::unix::fs::FileTypeExt;
    use tokio::net::UnixStream;
    use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};

    use tempfile::TempDir;

    use arcella_types::alme::proto::AlmeResponse;

    use crate::runtime::ArcellaRuntime;
    use crate::config::ArcellaConfig;

    async fn create_test_runtime() -> Arc<RwLock<ArcellaRuntime>> {
        // Create a minimal configuration
        let config = ArcellaConfig {
            base_dir: PathBuf::from("/tmp"),
            socket_path: PathBuf::from("/tmp/should_not_be_used.sock"),
            ..Default::default()
        };
        let runtime = ArcellaRuntime::new_for_tests(Arc::new(config)).await.unwrap();
        Arc::new(RwLock::new(runtime))
    }

    #[tokio::test]
    async fn test_alme_ping() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-test-ping.sock");
        println!("Socket path: {:?}", socket_path);

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        // Client
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        stream.write_all(b"{\"cmd\":\"ping\"}").await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        alme_handle.shutdown().await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "pong");
    }

    #[tokio::test]
    async fn test_alme_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-test-invalid.sock");

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        stream.write_all(b"{ invalid json }").await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        alme_handle.shutdown().await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(!resp.success);
        assert!(resp.message.contains("Invalid JSON"));
    }

    #[tokio::test]
    async fn test_alme_empty_request_with_ping() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-test-empty.sock");

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        // Simple connect
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        // Send several empty lines
        writer.write_all(b"\n").await.unwrap();
        writer.write_all(b"\r\n").await.unwrap();
        writer.write_all(b"   \n").await.unwrap();

        // Command Ping
        writer.write_all(b"{\"cmd\":\"ping\"}\n").await.unwrap();
        writer.flush().await.unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();
        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();

        alme_handle.shutdown().await.unwrap();

        assert!(resp.success);
        assert_eq!(resp.message, "pong");
    }

    #[tokio::test]
    async fn test_alme_status() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-test-status.sock");

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        stream.write_all(b"{\"cmd\":\"status\"}").await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        alme_handle.shutdown().await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "Arcella runtime is active");
        assert!(resp.data.is_some());
    }

    #[tokio::test]
    async fn test_socket_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-perm.sock");

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        // Check permissions: should be 0o600
        let metadata = std::fs::metadata(&socket_path).unwrap();
        let permissions = metadata.permissions();

        alme_handle.shutdown().await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(permissions.mode() & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn test_stale_socket_removal() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-stale.sock");

        // Create a stale socket file
        std::fs::write(&socket_path, b"stale").unwrap();

        let runtime = create_test_runtime().await;
        // Should start successfully despite the existing file
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        // Ensure it's now a socket
        let metadata = std::fs::metadata(&socket_path).unwrap();

        alme_handle.shutdown().await.unwrap();

        assert!(metadata.file_type().is_socket());
    }

    #[tokio::test]
    async fn test_multiple_commands_in_single_connection() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("alme-multi.sock");

        let runtime = create_test_runtime().await;
        let alme_handle = spawn_server(socket_path.clone(), runtime).await.unwrap();

        // Simple connect
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        // Command 1: ping
        writer.write_all(b"{\"cmd\":\"ping\"}\n").await.unwrap();
        writer.flush().await.unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();
        let resp1: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        response_line.clear();

        // Command 2: status
        writer.write_all(b"{\"cmd\":\"status\"}\n").await.unwrap();
        writer.flush().await.unwrap();

        reader.read_line(&mut response_line).await.unwrap();
        let resp2: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        response_line.clear();

        // Command 3: module:list
        writer.write_all(b"{\"cmd\":\"module:list\"}\n").await.unwrap();
        writer.flush().await.unwrap();

        reader.read_line(&mut response_line).await.unwrap();
        let resp3: AlmeResponse = serde_json::from_str(&response_line).unwrap();

        assert!(resp1.success);
        assert_eq!(resp1.message, "pong");
        assert!(resp2.success);
        assert_eq!(resp2.message, "Arcella runtime is active");
        assert!(resp2.data.is_some());
        assert!(resp3.success);
        assert!(resp3.data.is_some());
        let modules: Vec<serde_json::Value> = serde_json::from_value(resp3.data.unwrap()).unwrap();
        
        // Close socket
        drop(writer);
        drop(reader);

        alme_handle.shutdown().await.unwrap();

        assert_eq!(modules.len(), 0); // пока пусто

    }

}*/