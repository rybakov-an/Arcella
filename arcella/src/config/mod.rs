// arcella/arcella/src/config/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use futures::future;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use serde::Deserialize;
use tokio::fs;
use toml_edit::DocumentMut;

use arcella_types::{
    value::Value as TomlValue
};
use arcella_fs_utils as fs_utils;
use arcella_fs_utils::toml as arcella_toml;

use crate::error::{ArcellaError, Result as ArcellaResult};

#[derive(Deserialize, Default)]
pub struct ConfigFile {
    #[serde(default)]
    pub base_dir: Option<PathBuf>,
    #[serde(default)]
    pub log_dir: Option<PathBuf>,
    #[serde(default)]
    pub modules_dir: Option<PathBuf>,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub socket_path: Option<PathBuf>,
    #[serde(default)]
    pub includes: Includes,
    #[serde(default)]
    pub integrity_check: IntegrityCheck,
}

#[derive(Deserialize, Default)]
struct Includes {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    dirs: Vec<String>,
}

#[derive(Deserialize, Default)]
struct IntegrityCheck {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    dirs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArcellaConfig {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub modules_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub socket_path: PathBuf,
    pub integrity_check_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct IntegrityChecker {
    paths: Vec<PathBuf>,
    initial_mtimes: HashMap<PathBuf, SystemTime>,
}

impl IntegrityChecker {
    pub fn new(paths: Vec<PathBuf>) -> ArcellaResult<Self> {
        let mut initial_mtimes = HashMap::new();
        for path in &paths {
            let metadata = std::fs::metadata(path)
                .map_err(|e| ArcellaError::IoWithPath { source: e, path: path.clone() })?;
            let mtime = metadata.modified()
                .map_err(|e| ArcellaError::Internal(format!("Cannot get mtime for {:?}: {}", path, e)))?;
            initial_mtimes.insert(path.clone(), mtime);
        }
        Ok(IntegrityChecker { paths, initial_mtimes })
    }

    pub async fn check(&self) -> ArcellaResult<()> {
        let current_mtimes = get_current_mtimes(&self.paths).await?;
        check_mtimes_changed(&self.initial_mtimes, &current_mtimes)
    }
}

fn check_mtimes_changed(
    initial_mtimes: &HashMap<PathBuf, std::time::SystemTime>,
    current_mtimes: &HashMap<PathBuf, std::time::SystemTime>,
) -> ArcellaResult<()> {
    for (path, current_mtime) in current_mtimes {
        if let Some(initial_mtime) = initial_mtimes.get(path) {
            if current_mtime != initial_mtime {
                return Err(ArcellaError::Internal(
                    format!("Config integrity violation: file {:?} was modified after startup", path)
                ));
            }
        } else {
            return Err(ArcellaError::Internal(
                format!("Config integrity violation: file {:?} not found in initial list", path)
            ));
        }
    }
    Ok(())
}

async fn get_current_mtimes(paths: &[PathBuf]) -> ArcellaResult<HashMap<PathBuf, std::time::SystemTime>> {
    // Создадим вектор future'ов для каждой проверки mtime
    let checks: Vec<_> = paths.iter().map(|path| {
        let path = path.clone();
        async move {
            let metadata = tokio::fs::metadata(&path).await
                .map_err(|e| ArcellaError::IoWithPath { source: e, path: path.clone() })?;
            let mtime = metadata.modified()
                .map_err(|e| ArcellaError::Internal(format!("Cannot get mtime for {:?}: {}", path, e)))?;
            Ok::<(PathBuf, std::time::SystemTime), ArcellaError>((path, mtime))
        }
    }).collect();

    // Запускаем все future'ы параллельно и ждем их завершения
    let results = future::join_all(checks).await;

    let mut current_mtimes = HashMap::with_capacity(results.len());
    for result in results {
        let (path, mtime) = result?; // Если одна из проверок завершится с ошибкой, propagate
        current_mtimes.insert(path, mtime);
    }

    Ok(current_mtimes)
}

const REDEF_SUFFIX: &str = "#redef";

pub async fn load() -> ArcellaResult<(ArcellaConfig, Vec<fs_utils::ConfigLoadWarning>)> {
    
    // 1. Find base_dir
    let base_dir = fs_utils::find_base_dir().await?;

    // 2. Set config_dir
    let config_dir = base_dir.join("config");    

    // 3. Ensure config_dir exists
    //ensure_config_template(&config_dir).await?;

    // 4. Load arcella.toml
    let main_config_path = config_dir.join("arcella.toml");
    let (configs, mut warnings) = fs_utils::load_config_recursive_from_file(&main_config_path, &config_dir).await?;

    let mut final_values: HashMap<String, (TomlValue, usize)> = HashMap::new();

    // Обходим configs в обратном порядке (начиная с большего индекса)
    for (layer_index, config_data) in configs.iter().enumerate().rev() {
        for (key, value) in &config_data.values {
            // Проверяем, заканчивается ли ключ на #redef
            let (actual_key, is_redef) = if key.ends_with(REDEF_SUFFIX) {
                // Извлекаем оригинальный ключ без суффикса
                let original_key = key[..key.len() - REDEF_SUFFIX.len()].to_string();
                (original_key, true)
            } else {
                (key.clone(), false)
            };

            // Проверяем, существует ли ключ в final_values
            if let Some((existing_value, existing_layer_index)) = final_values.get(&actual_key) {
                // Ключ уже существует
                // Если у текущего ключа был флаг #redef, не обновляем его новым значением из более низкого слоя
                if is_redef { continue; }

                warnings.push(fs_utils::ConfigLoadWarning::ValueError {
                    key: actual_key.clone(),
                    error: format!("Value from layer {} ignored due to #redef flag from layer {}", layer_index, existing_layer_index),
                    file: PathBuf::from(format!("layer_{}.toml", layer_index)),
                });
            }

            // В противном случае, обновляем значение
            final_values.insert(actual_key, (value.clone(), layer_index));

        }
    }

    let log_dir = final_values.get("arcella.log.dir")
        .and_then(|(v, _)| if let TomlValue::String(s) = v { Some(PathBuf::from(s)) } else { None })
        .unwrap_or_else(|| base_dir.join("log"));

    let modules_dir = final_values.get("arcella.modules.dir")
        .and_then(|(v, _)| if let TomlValue::String(s) = v { Some(PathBuf::from(s)) } else { None })
        .unwrap_or_else(|| base_dir.join("modules"));

    let cache_dir = final_values.get("arcella.cache.dir")
        .and_then(|(v, _)| if let TomlValue::String(s) = v { Some(PathBuf::from(s)) } else { None })
        .unwrap_or_else(|| base_dir.join("cache"));

    let socket_path = final_values.get("arcella.alme.socket.path")
        .and_then(|(v, _)| if let TomlValue::String(s) = v { Some(PathBuf::from(s)) } else { None })
        .unwrap_or_else(|| base_dir.join("alme"));

    Ok((ArcellaConfig {
        base_dir: base_dir,
        config_dir: config_dir,
        log_dir: log_dir,
        modules_dir: modules_dir,
        cache_dir: cache_dir,
        socket_path: socket_path,
        integrity_check_paths: vec![],
    }, warnings))
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

}

