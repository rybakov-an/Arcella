use std::sync::Arc;
use std::path::PathBuf;

use crate::config::ArcellaConfig;
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct StorageManager {
    pub base_dir: PathBuf,
    pub cfg_dir: PathBuf,
    pub modules_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl StorageManager {
    pub async fn new(
        config: &Arc<ArcellaConfig>,
    ) -> ArcellaResult<Self> {
        let manager = Self {
            base_dir: config.base_dir.clone(),
            cfg_dir: config.cfg_dir.clone(),
            modules_dir: config.modules_dir.clone(),
            cache_dir: config.cache_dir.clone(),
        };

        manager.ensure_directories().await?;
        Ok(manager)

    }

    async fn ensure_directories(&self) -> ArcellaResult<()> {
        if !self.base_dir.exists() {
            std::fs::create_dir_all(&self.base_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&self.base_dir)?.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(&self.base_dir, perms)?;
            }
        }

        if !self.cfg_dir.exists() {
            std::fs::create_dir_all(&self.cfg_dir)?;
        }

        if !self.modules_dir.exists() {
            std::fs::create_dir_all(&self.modules_dir)?;
        }

        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        Ok(())

    } 

}