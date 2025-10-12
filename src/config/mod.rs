use std::path::PathBuf;
use serde::Deserialize;

use crate::error::{ArcellaError, Result as ArcellaResult};

#[derive(Debug, Clone, Deserialize)]
pub struct ArcellaConfig {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub modules_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub socket_path: PathBuf,
}

impl Default for ArcellaConfig {
    fn default() -> Self {
        let base = dirs::home_dir().unwrap().join(".arcella");
        Self {
            base_dir: base.clone(),
            config_dir: base.join("config"),
            log_dir: base.join("log"),
            modules_dir: base.join("modules"),
            cache_dir: base.join("cache"),
            socket_path: base.join("alme"),
        }
    }

}

pub async fn load() -> ArcellaResult<ArcellaConfig> {
    // Пока просто возвращаем default
    Ok(ArcellaConfig::default())
}

