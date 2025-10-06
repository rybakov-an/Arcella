use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{runtime};

use crate::error::{ArcellaError, Result as ArcellaResult};

pub mod protocol;
pub mod server;

pub struct AlmeServerHandle {
    socket_path: std::path::PathBuf,
}

/// Starts the ALME (Arcella Local Management Extensions) server in the background,
/// providing IPC access to the shared runtime instance.
pub async fn start(runtime: Arc<RwLock<runtime::ArcellaRuntime>>) -> ArcellaResult<AlmeServerHandle>  {
    let sock_path = runtime.read().await.config.sock_path.clone();

    server::spawn_server(sock_path, runtime).await    

}