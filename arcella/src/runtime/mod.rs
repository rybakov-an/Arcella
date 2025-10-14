// arcella/arcella/src/runtime/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::{
    collections::HashMap,
    path::{Path},
    sync::Arc,
    time::{Duration, Instant}
};
use time::OffsetDateTime;
use tokio::sync::{RwLock, broadcast};

use crate::{storage, cache};
use crate::config::ArcellaConfig;
use crate::error::{ArcellaError, Result as ArcellaResult};
use crate::manifest::ModuleManifest;

struct ArcellaRuntimeEnvironment {
    pub pid: u32,
    pub start_instant: Instant,
    pub start_utc: OffsetDateTime,
}

pub struct ArcellaRuntimeStatus {
    pub pid: u32,
    pub start_time: OffsetDateTime,
    pub uptime: Duration,
}

pub struct ArcellaRuntime {
    pub config: Arc<ArcellaConfig>,
    pub storage: Arc<storage::StorageManager>,
    pub cache: Arc<cache::ModuleCache>,
    pub environment: Arc<RwLock<ArcellaRuntimeEnvironment>>,
    pub modules: HashMap<String, ModuleManifest>, // key = name@version
    // Позже: instances, engine и т.д.

}

impl ArcellaRuntime{
    pub async fn new(
        config: Arc<ArcellaConfig>,
        storage: Arc<storage::StorageManager>,
        cache: Arc<cache::ModuleCache>,
    ) -> ArcellaResult<Self> {

        let env = ArcellaRuntimeEnvironment {
            pid: std::process::id(),
            start_instant: Instant::now(),
            start_utc: OffsetDateTime::now_utc(),
        };

        let runtime = Self {
            config,
            storage,
            cache,
            environment: Arc::new(RwLock::new(env)),
            modules: HashMap::new(),
        };

        Ok(runtime)
    }

    pub async fn shutdown(&mut self) -> ArcellaResult<()> {
        // To be added stopping modules, instances, and the engine
        Ok(())
    }

    pub fn status(&self) -> ArcellaResult<ArcellaRuntimeStatus> {

        let env = self.environment.try_read().expect("Runtime environment poisoned");

        return Ok(ArcellaRuntimeStatus {
            pid: env.pid,
            start_time: env.start_utc,
            uptime: self.uptime(),
        });

    }

    pub fn uptime(&self) -> std::time::Duration {
        let env = self.environment.try_read().expect("Runtime environment poisoned");
        env.start_instant.elapsed()
    }

    pub async fn install_module_from_path(
        &mut self,
        wasm_path: &Path,
    ) -> ArcellaResult<()> {
        let manifest = ModuleManifest::from_wasm_path(wasm_path)?;
        manifest.validate()?;

        let key = manifest.module.id();
        self.modules.insert(key.clone(), manifest);

        tracing::info!("Installed module metadata: {}", key);
        Ok(())
    }

    #[cfg(test)]
    pub async fn new_for_tests(config: Arc<ArcellaConfig>) -> ArcellaResult<Self> {

        let storage = Arc::new(storage::StorageManager::new(&config).await?);
        let cache = Arc::new(cache::ModuleCache::new(&config).await?);
        let test_runtime = Self::new(config, storage, cache).await?;

        Ok(test_runtime)
    }

}
