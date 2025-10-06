pub mod protocol;
pub mod server;

//use crate::runtime::ArcellaRuntime;
use crate::error::{ArcellaError, Result as ArcellaResult};
use std::sync::{Arc, RwLock};

//pub use server::start;

/// Starts the ALME (Arcella Local Management Extensions) server in the background,
/// providing IPC access to the shared runtime instance.
/*pub fn start(runtime: Arc<RwLock<ArcellaRuntime>>) -> Result<()> {
    let sock_path = get_alme_socket_path()?;
    server::spawn_server(sock_path, runtime)
}*/

pub async fn start() -> ArcellaResult<()>  {
    
    server::spawn_server_standalone().await

}