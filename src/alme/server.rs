// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::os::unix::fs::PermissionsExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::runtime::ArcellaRuntime;
use crate::error::{ArcellaError, Result as ArcellaResult};

use crate::alme::{AlmeServerHandle};
use crate::alme::protocol::{AlmeRequest, AlmeResponse};

/// Spawns the ALME server in a dedicated background thread.
pub async fn spawn_server(
    sock_path: PathBuf, 
    runtime: Arc<RwLock<ArcellaRuntime>>,
) -> ArcellaResult<AlmeServerHandle> {

    if sock_path.exists() {
        if let Err(e) = fs::remove_file(&sock_path) {
            eprintln!("Warning: failed to remove stale socket: {}", e);
        }
    }

    let listener = UnixListener::bind(&sock_path)?;
    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600))?;

    let runtime_clone = runtime.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let rt = runtime_clone.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, rt).await {
                    eprintln!("[ALME] Connection error: {}", e);
                }
            });
        }
    });

    Ok(AlmeServerHandle { socket_path: sock_path })
}


/// Handles a single client connection: reads a command, processes it, and sends a response.
async fn handle_connection(
    mut stream: tokio::net::UnixStream, 
    runtime: Arc<RwLock<ArcellaRuntime>>,
) -> ArcellaResult<()> {
    let mut buffer = Vec::new();
    let n = stream.read_to_end(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    let request_str = String::from_utf8_lossy(&buffer[..n]);
    let request: AlmeRequest = match serde_json::from_str(&request_str) {
        Ok(req) => req,
        Err(e) => {
            let resp = AlmeResponse {
                success: false,
                message: format!("Invalid JSON: {}", e),
                data: None,
            };
            send_response(&mut stream, &resp).await?;
            return Ok(());
        }
    };

    let response = match request {
        AlmeRequest::Ping => AlmeResponse {
            success: true,
            message: "pong".to_string(),
            data: None,
        },
        AlmeRequest::Status => {
            let runtime_guard = runtime.read().await;

            let data = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "info": runtime_guard.test()?,
            });

            AlmeResponse {
                success: true,
                message: "Arcella runtime is active".to_string(),
                data: Some(data),
            }
        },
        AlmeRequest::ListModules => AlmeResponse {
            success: true,
            message: "No modules (standalone mode)".to_string(),
            data: Some(serde_json::json!([])),
        },
    };

    send_response(&mut stream, &response).await
}

/// Serializes and sends an ALME response back to the client.
async fn send_response(
    stream: &mut tokio::net::UnixStream,
    response: &AlmeResponse,
) -> ArcellaResult<()> {
    let json = serde_json::to_vec(response)?;
    stream.write_all(&json).await?;
    stream.write_all(b"\n").await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::os::unix::fs::FileTypeExt;
    use tokio::net::UnixStream;
    use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};

    use tempfile::TempDir;

    use crate::runtime::ArcellaRuntime;
    use crate::config::ArcellaConfig;
    use crate::error::{ArcellaError, Result as ArcellaResult};

    use crate::alme::{AlmeServerHandle};
    use crate::alme::protocol::{AlmeRequest, AlmeResponse};


    async fn create_test_runtime() -> Arc<RwLock<ArcellaRuntime>> {
        // Create a minimal configuration
        let config = ArcellaConfig {
            base_dir: PathBuf::from("/tmp"),
            sock_path: PathBuf::from("/tmp/should_not_be_used.sock"),
            ..Default::default()
        };
        let runtime = ArcellaRuntime::new_for_tests(Arc::new(config)).await.unwrap();
        Arc::new(RwLock::new(runtime))
    }

    #[tokio::test]
    async fn test_alme_ping() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-test-ping.sock");
        println!("Socket path: {:?}", sock_path);

        let runtime = create_test_runtime().await;
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        // Client
        let mut stream = UnixStream::connect(&sock_path).await.unwrap();
        stream.write_all(b"{\"cmd\":\"Ping\"}").await.unwrap();
        stream.shutdown().await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "pong");
    }

    #[tokio::test]
    async fn test_alme_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-test-invalid.sock");

        let runtime = create_test_runtime().await;
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        let mut stream = UnixStream::connect(&sock_path).await.unwrap();
        stream.write_all(b"{ invalid json }").await.unwrap();
        stream.shutdown().await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(!resp.success);
        assert!(resp.message.contains("Invalid JSON"));
    }

    #[tokio::test]
    async fn test_alme_empty_request() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-test-empty.sock");

        let runtime = create_test_runtime().await;
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        let mut stream = UnixStream::connect(&sock_path).await.unwrap();
        stream.write_all(b"").await.unwrap();
        stream.shutdown().await.unwrap();
        stream.flush().await.unwrap();

        // The server should not panic and should send no response
        let mut buf = Vec::new();
        let n = stream.read_to_end(&mut buf).await.unwrap();
        assert_eq!(n, 0); // connection closed with no response
    }

    #[tokio::test]
    async fn test_alme_status() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-test-status.sock");

        let runtime = create_test_runtime().await;
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        let mut stream = UnixStream::connect(&sock_path).await.unwrap();
        stream.write_all(b"{\"cmd\":\"Status\"}").await.unwrap();
        stream.shutdown().await.unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        let resp: AlmeResponse = serde_json::from_str(&response_line).unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "Arcella runtime is active");
        assert!(resp.data.is_some());
    }

    #[tokio::test]
    async fn test_socket_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-perm.sock");

        let runtime = create_test_runtime().await;
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        // Check permissions: should be 0o600
        let metadata = std::fs::metadata(&sock_path).unwrap();
        let permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(permissions.mode() & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn test_stale_socket_removal() {
        let temp_dir = TempDir::new().unwrap();
        let sock_path = temp_dir.path().join("alme-stale.sock");

        // Create a stale socket file
        std::fs::write(&sock_path, b"stale").unwrap();

        let runtime = create_test_runtime().await;
        // Should start successfully despite the existing file
        let _handle = spawn_server(sock_path.clone(), runtime).await.unwrap();

        // Ensure it's now a socket
        let metadata = std::fs::metadata(&sock_path).unwrap();
        assert!(metadata.file_type().is_socket());
    }
}