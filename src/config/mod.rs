use std::path::PathBuf;
use serde::Deserialize;

use crate::error::{ArcellaError, Result as ArcellaResult};

#[derive(Debug, Clone, Deserialize)]
pub struct ArcellaConfig {
    pub base_dir: PathBuf,
    pub cfg_dir: PathBuf,
    pub modules_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub sock_path: PathBuf,
}

impl Default for ArcellaConfig {
    fn default() -> Self {
        let base = dirs::home_dir().unwrap().join(".arcella");
        Self {
            base_dir: base.clone(),
            cfg_dir: base.join("config"),
            modules_dir: base.join("modules"),
            cache_dir: base.join("cache"),
            sock_path: base.join("alme"),
        }
    }

}

pub async fn load() -> ArcellaResult<ArcellaConfig> {
    // Пока просто возвращаем default
    Ok(ArcellaConfig::default())
}

