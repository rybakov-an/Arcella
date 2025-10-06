use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{runtime};
use crate::error::{ArcellaError, Result as ArcellaResult};

use crate::alme::{AlmeServerHandle};
use crate::alme::protocol::{AlmeRequest, AlmeResponse};

/// Ensure the path to the ALME Unix socket: `~/.arcella/alme`
pub fn ensure_sock(sock_path: &PathBuf) -> ArcellaResult<()> {

    println!("Ensure socket path: {:?}", sock_path);

    if sock_path.exists() {
        fs::remove_file(sock_path)?;
    }
    
    Ok(())
}

/// Spawns the ALME server in a dedicated background thread.
pub async fn spawn_server(
    sock_path: PathBuf, 
    runtime: Arc<RwLock<runtime::ArcellaRuntime>>,
) -> ArcellaResult<AlmeServerHandle> {
    let _ = ensure_sock(&sock_path)?;

    if sock_path.exists() {
        if let Err(e) = std::fs::remove_file(&sock_path) {
            eprintln!("Warning: failed to remove stale socket: {}", e);
        }
    }

    let listener = UnixListener::bind(&sock_path)?;

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
    runtime: Arc<RwLock<runtime::ArcellaRuntime>>,
) -> ArcellaResult<()> {
    let mut buffer = vec![0; 4096];
    let n = stream.read(&mut buffer).await?;
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
    let json = serde_json::to_string(response)?;
    stream.write_all(json.as_bytes()).await?;
    Ok(())
}