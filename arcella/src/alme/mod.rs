// arcella/arcella/src/alme/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;

use crate::runtime::ArcellaRuntime;
use crate::error::Result as ArcellaResult;

mod commands;
mod server;

pub struct AlmeServerHandle {
    shutdown_tx: Option<broadcast::Sender<()>>,
    join_handle: Option<JoinHandle<ArcellaResult<()>>>,
}

impl AlmeServerHandle {
    /// Gracefully shuts down the ALME server and waits for it to finish.
    pub async fn shutdown(mut self) -> ArcellaResult<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            tracing::debug!("Sending shutdown signal to ALME server");
       }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.await?;
        }
        Ok(())
    }
}

impl Drop for AlmeServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            tracing::debug!("Sending shutdown signal to ALME server on drop");
        }
    }
}

/// Starts the ALME (Arcella Local Management Extensions) server in the background,
/// providing IPC access to the shared runtime instance.
pub async fn start(runtime: Arc<RwLock<ArcellaRuntime>>) -> ArcellaResult<AlmeServerHandle>  {
    let socket_path = runtime.read().await.config.socket_path.clone();

    server::spawn_server(socket_path, runtime).await    

}