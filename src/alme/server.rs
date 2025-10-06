use std::fs;
use std::path::PathBuf;
use crate::alme::protocol::{AlmeRequest, AlmeResponse};
//use crate::runtime::ArcellaRuntime;
//use crate::error::Result;
use anyhow::{Result};
use std::sync::{Arc, RwLock};
use std::thread;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Returns the path to the ALME Unix socket: `~/.arcella/alme`
pub fn get_alme_socket_path() -> Result<PathBuf> {
    let base = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".arcella");

    fs::create_dir_all(&base)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&base)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&base, perms)?;
    }

    Ok(base.join("alme"))
}

/// Spawns the ALME server in a dedicated background thread.
/*pub fn spawn_server(
    sock_path: std::path::PathBuf,
    //runtime: Arc<RwLock<ArcellaRuntime>>,
) -> Result<()> {
    // Remove stale socket file if it exists
    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
    }

    // Launch a dedicated Tokio runtime in a new thread
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime");

        rt.block_on(async move {
            let listener = UnixListener::bind(&sock_path).await
                .expect("Failed to bind ALME Unix socket");

            // Restrict socket permissions to owner-only (Unix systems)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))
                    .expect("Failed to set socket permissions");
            }

            log::info!("ALME server listening on {}", sock_path.display());

            while let Ok((stream, _)) = listener.accept().await {
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, runtime).await {
                        log::error!("ALME connection error: {}", e);
                    }
                });
            }
        });
    });

    Ok(())
}*/

/// Spawns the ALME server in a dedicated background thread (standalone mode, no runtime).
pub fn spawn_server_standalone() -> Result<()> {
    let sock_path = get_alme_socket_path()?;

    if sock_path.exists() {
        if let Err(e) = std::fs::remove_file(&sock_path) {
            eprintln!("Warning: failed to remove stale socket: {}", e);
        }
    }

    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime");

        rt.block_on(async move {

            let listener = match UnixListener::bind(&sock_path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind ALME Unix socket: {}", e);
                    return;
                }
            };

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
            }

            println!("[ALME] Listening on {}", sock_path.display());

            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    if let Err(e) = handle_connection_standalone(stream).await {
                        eprintln!("[ALME] Connection error: {}", e);
                    }
                });
            }
        });
    });

    Ok(())
}

/// Handles a single client connection: reads a command, processes it, and sends a response.
/*async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    runtime: Arc<RwLock<ArcellaRuntime>>,
) -> Result<()> {
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
            //let runtime_guard = runtime.read().unwrap();
            let data = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "modules_loaded": false,//runtime_guard.modules.len(),
                "instances_running": false,//runtime_guard.instances.len(),
            });
            AlmeResponse {
                success: true,
                message: "Arcella runtime is active".to_string(),
                data: Some(data),
            }
        }
        AlmeRequest::ListModules => {
            //let runtime_guard = runtime.read().unwrap();
            let modules: Vec<_> = Vec::new(); //runtime_guard.modules.keys().cloned().collect();
            AlmeResponse {
                success: true,
                message: "Modules loaded".to_string(),
                data: Some(serde_json::json!(modules)),
            }
        }
    };

    send_response(&mut stream, &response).await
}*/

// Standalone handler (no runtime access)
async fn handle_connection_standalone(
    mut stream: tokio::net::UnixStream,
) -> Result<()> {
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
        AlmeRequest::Status => AlmeResponse {
            success: true,
            message: "Arcella runtime is active (standalone mode)".to_string(),
            data: Some(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "modules_loaded": 0,
                "instances_running": 0,
                "mode": "standalone"
            })),
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
) -> Result<()> {
    let json = serde_json::to_string(response)?;
    stream.write_all(json.as_bytes()).await?;
    Ok(())
}