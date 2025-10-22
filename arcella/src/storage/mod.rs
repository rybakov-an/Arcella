// arcella/arcella/src/storage/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::Arc;
use std::path::PathBuf;

use crate::config::ArcellaConfig;
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct StorageManager {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub modules_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl StorageManager {
    pub async fn new(
        config: &Arc<ArcellaConfig>,
    ) -> ArcellaResult<Self> {

        let base_dir = config.base_dir.clone();//.unwrap_or_else(|| PathBuf::from("."));
        let config_dir = config.config_dir.clone();//.unwrap_or_else(|| base_dir.join("config"));
        let modules_dir = config.modules_dir.clone();//.unwrap_or_else(|| base_dir.join("modules"));
        let cache_dir = config.cache_dir.clone();//.unwrap_or_else(|| base_dir.join("cache"));

        let manager = Self {
            base_dir,
            config_dir,
            modules_dir,
            cache_dir,
        };

        manager.ensure_directories().await?;
        Ok(manager)

    }

    async fn ensure_directories(&self) -> ArcellaResult<()> {
        if !self.base_dir.exists() {
            tokio::fs::create_dir_all(&self.base_dir).await?;
            tracing::info!("Created base directory: {:?}", self.base_dir);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = tokio::fs::metadata(&self.base_dir).await?.permissions();
                perms.set_mode(0o700);
                tokio::fs::set_permissions(&self.base_dir, perms).await?;
                tracing::info!("Set permissions for base directory: {:?}", self.base_dir);
            }
        }

        if !self.config_dir.exists() {
            tokio::fs::create_dir_all(&self.config_dir).await?;
            tracing::info!("Created config directory: {:?}", self.config_dir);
        }

        if !self.modules_dir.exists() {
            tokio::fs::create_dir_all(&self.modules_dir).await?;
            tracing::info!("Created modules directory: {:?}", self.modules_dir);
        }

        if !self.cache_dir.exists() {
            tokio::fs::create_dir_all(&self.cache_dir).await?;
            tracing::info!("Created cache directory: {:?}", self.cache_dir);
        }

        Ok(())
    } 

}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /*#[tokio::test]
    async fn test_storage_manager_creates_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("arcella_test");

        let config = Arc::new(ArcellaConfig {
            base_dir: Some(base_path.clone()),
            config_dir: Some(base_path.join("config")),
            log_dir: Some(base_path.join("log")),
            modules_dir: Some(base_path.join("modules")),
            cache_dir: Some(base_path.join("cache")),
            socket_path: Some(base_path.join("alme")),
        });

        let storage = StorageManager::new(&config).await.unwrap();

        assert!(storage.base_dir.exists());
        assert!(storage.config_dir.exists());
        assert!(storage.modules_dir.exists());
        assert!(storage.cache_dir.exists());

        // Проверка прав доступа для base_dir (только на Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&storage.base_dir).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o700);
        }
    }*/
}