// arcella/arcella/src/config/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use futures::future;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;

use arcella_types::{
    value::Value as TomlValue
};
use arcella_fs_utils as fs_utils;

use crate::error::{ArcellaError, Result as ArcellaResult};

const REDEF_SUFFIX: &str = "#redef";
const DEFAULT_CONFIG_CONTENT: &str = include_str!("default_config.toml");
const TEMPLATE_CONFIG_CONTENT: &str = include_str!("template_config.toml");

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

async fn ensure_config_template(config_dir: &Path) -> ArcellaResult<(PathBuf, Vec<fs_utils::ConfigLoadWarning>)> {
    let main_config_path = config_dir.join("arcella.toml");
    let template_path = config_dir.join("arcella.template.toml");

    let mut warnings: Vec<fs_utils::ConfigLoadWarning> = vec![];

    // Создать config_dir, если не существует
    fs::create_dir_all(config_dir)
        .await
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: config_dir.to_path_buf() })?;

    // Создать шаблон (arcella.template.toml), если он не существует
    if !template_path.exists() {
        fs::write(&template_path, TEMPLATE_CONFIG_CONTENT)
            .await
            .map_err(|e| ArcellaError::IoWithPath { source: e, path: template_path.clone() })?;
        warnings.push(fs_utils::ConfigLoadWarning::Internal(
            format!("Created default config template at {:?}", template_path)
        ));
    }

    // Проверить, существует ли arcella.toml, если не существует, то скопировать файл из template_path
    if !main_config_path.exists() {
        fs::copy(&template_path, &main_config_path).await?;
        warnings.push(fs_utils::ConfigLoadWarning::Internal(
            format!("Created default config at {:?}", main_config_path)
        ));
    }

    Ok((main_config_path, warnings))

}

pub async fn load() -> ArcellaResult<(ArcellaConfig, Vec<fs_utils::ConfigLoadWarning>)> {
    
    // 1. Find base_dir
    let base_dir = fs_utils::find_base_dir().await?;

    // 2. Set config_dir
    let config_dir = base_dir.join("config");    

    // 3. Ensure config_dir exists
    let (main_config_path, mut warnings) = ensure_config_template(&config_dir).await?;

    let mut visited = HashSet::new();

    // 4. Load arcella.toml
    let configs = fs_utils::load_config_recursive_from_content(
        &["arcella".to_string()],
        &DEFAULT_CONFIG_CONTENT,
        &main_config_path,
        &config_dir,
        0,
        &mut visited,
        &mut warnings,
    ).await?;

    let mut final_values: IndexMap<String, (TomlValue, usize)> = IndexMap::new();

    // 5. Merge configs
    // Reverse the order of configs to process them in the correct order
    for (layer_index, config_data) in configs.iter().enumerate().rev() {
        for (key, value) in &config_data.values {
            // Check if the key ends with #redef
            let (actual_key, is_redef) = if key.ends_with(REDEF_SUFFIX) {
                // Extract the original key without the #redef suffix
                let original_key = key[..key.len() - REDEF_SUFFIX.len()].to_string();
                (original_key, true)
            } else {
                (key.clone(), false)
            };

            // Check if the key already exists in final_values
            if let Some((_, existing_layer_index)) = final_values.get(&actual_key) {
                // Key already exists
                // If the current key has #redef flag, do not update it with a new value from a lower layer
                if is_redef { continue; }

                warnings.push(fs_utils::ConfigLoadWarning::ValueError {
                    key: actual_key.clone(),
                    error: format!("Value from layer {} ignored due to #redef flag from layer {}", layer_index, existing_layer_index),
                    file: PathBuf::from(format!("layer_{}.toml", layer_index)),
                });
            }

            // Otherwise, update the value
            final_values.insert(actual_key, (value.clone(), layer_index));

        }
    }

    final_values.sort_keys();

    let log_dir = match final_values.get("arcella.log.dir") {
        Some((TomlValue::String(s) ,_)) => {
            PathBuf::from(s)
        }
        _ => {
            return Err(ArcellaError::Internal("arcella.log.dir is not set".to_string()));
        }
    };

    let modules_dir = match final_values.get("arcella.modules.dir") {
        Some((TomlValue::String(s) ,_)) => {
            PathBuf::from(s)
        }
        _ => {
            return Err(ArcellaError::Internal("arcella.modules.dir is not set".to_string()));
        }
    };

    let cache_dir = match final_values.get("arcella.cache.dir") {
        Some((TomlValue::String(s) ,_)) => {
            PathBuf::from(s)
        }
        _ => {
            return Err(ArcellaError::Internal("arcella.cache.dir is not set".to_string()));
        }
    };

    let socket_path = match final_values.get("arcella.alme.socket.path") {
        Some((TomlValue::String(s) ,_)) => {
            PathBuf::from(s)
        }
        _ => {
            return Err(ArcellaError::Internal("arcella.alme.socket.path is not set".to_string()));
        }
    };

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

