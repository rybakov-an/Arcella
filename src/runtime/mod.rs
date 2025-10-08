use std::sync::Arc;

use crate::{storage, cache};
use crate::config::ArcellaConfig;
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct ArcellaRuntime {
    pub config: Arc<ArcellaConfig>,
    pub storage: Arc<storage::StorageManager>,
    pub cache: Arc<cache::ModuleCache>,
    // Позже: modules, instances, engine и т.д.
}

impl ArcellaRuntime{
    pub async fn new(
        config: Arc<ArcellaConfig>,
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

    #[cfg(test)]
    pub async fn new_for_tests(config: Arc<ArcellaConfig>) -> ArcellaResult<Self> {

        let storage = Arc::new(storage::StorageManager::new(&config).await?);
        let cache = Arc::new(cache::ModuleCache::new(&config).await?);

        Ok(Self {
            config,
            storage,
            cache,
        })
    }

}