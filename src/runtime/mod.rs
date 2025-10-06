use std::sync::Arc;

use crate::{config, storage, cache};
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct ArcellaRuntime {
    pub config: Arc<config::Config>,
    pub storage: Arc<storage::StorageManager>,
    pub cache: Arc<cache::ModuleCache>,
    // Позже: modules, instances, engine и т.д.
}

impl ArcellaRuntime{
    pub async fn new(
        config: Arc<config::Config>,
        storage: Arc<storage::StorageManager>,
        cache: Arc<cache::ModuleCache>,
    ) -> ArcellaResult<Self> {
        Ok(Self {
            config,
            storage,
            cache,
        })
    }

    pub async fn shutdown(&mut self) -> ArcellaResult<()> {
        // To be added stopping modules, instances, and the engine
        Ok(())
    }

    pub fn test(&self) -> ArcellaResult<String> {
        Ok("Runtime message".to_string())
    }
}