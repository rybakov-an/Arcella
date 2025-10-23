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
use toml_edit::{DocumentMut, Item as TomlItem, Value as TomlValue, Array as TomlArray};

use arcella_types::{
    value::Value as TValue
};
use arcella_fs_utils as fs_utils;
use arcella_fs_utils::{toml::ValueExt};

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

pub async fn load() -> ArcellaResult<ArcellaConfig> {
    
    // 1. Find base_dir
    let base_dir = fs_utils::find_base_dir().await?;

    // 2. Set config_dir
    let config_dir = base_dir.join("config");    

    // 3. Ensure config_dir exists
    //ensure_config_template(&config_dir).await?;

    // 4. Load arcella.toml
    let config_file_path = config_dir.join("arcella.toml");
    if !config_file_path.exists() {
        return Err(ArcellaError::Config(
            format!("Main config file not found: {:?}", config_file_path)
        ));
    }

    let content = tokio::fs::read_to_string(&config_file_path).await
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: config_file_path.clone() })?;

    let mut main_doc = content.parse::<DocumentMut>()
        .map_err(|e| ArcellaError::Config(e.to_string()))?;

    /*let config_file_path = default_config.config_dir.as_ref().unwrap().join("arcella.toml");

    let config = if config_file_path.exists() {
        let content = tokio::fs::read_to_string(&config_file_path)
            .await
            .map_err(|e| ArcellaError::IoWithPath { source: e, path: config_file_path.clone() })?;

        let file_config: ConfigFile = toml::from_str(&content)
            .map_err(|e| ArcellaError::Config(e.to_string()))?;

        ArcellaConfig {
            base_dir: default_config.base_dir,
            config_dir: default_config.config_dir,
            log_dir: file_config.log_dir.or(default_config.log_dir),
            modules_dir: file_config.modules_dir.or(default_config.modules_dir),
            cache_dir: file_config.cache_dir.or(default_config.cache_dir),
            socket_path: file_config.socket_path.or(default_config.socket_path),
        }

    } else {
        default_config
    };*/

    Ok(ArcellaConfig {
        base_dir: base_dir,
        config_dir: config_dir,
        log_dir: PathBuf::from("log"),
        modules_dir: PathBuf::from("modules"),
        cache_dir: PathBuf::from("cache"),
        socket_path: PathBuf::from("alme"),
        integrity_check_paths: vec![],
    })
}


#[derive(Debug, Clone)]
struct KeyValueWithLevel {
    level: i8,
    value: TValue,
}


/*pub async fn collect_includes_recursive(
    includes: &Vec<String>,
    config_dir: &Path,
    max_depth: usize,
) -> ArcellaResult<(Vec<String>, HashMap<String, KeyValueWithLevel>)> {
    //let mut full_includes = Vec::new();
    //let mut values = HashMap::new();

    // Step 1: Resolve all include paths (both files and dirs) from the main config
    let all_paths = resolve_include_paths(includes, config_dir)?;

    // Step 2: Separate files and directories
    let mut include_files = Vec::new();
    let mut include_dirs = Vec::new();

    for path in &all_paths {
        if path.is_file() {
            include_files.push(path.clone());
        } else if path.is_dir() {
            include_dirs.push(path.clone());
        }
    }

    // Step 3: Process all resolved paths asynchronously and in parallel
    // a) Process individual files: filter using is_valid_toml_file_path
    let mut valid_file_paths = Vec::new();
    for file_path in include_files {
        if is_valid_toml_file_path(&file_path) {
            valid_file_paths.push(file_path);
        }
    }


    Ok((Vec::new(), HashMap::new()))
}*/


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    /*#[tokio::test]
    async fn test_load_default() {
        let config = load().await.unwrap();
        let base = dirs::home_dir().unwrap().join(".arcella");
        assert_eq!(config.base_dir, Some(base.clone()));
        assert_eq!(config.config_dir, Some(base.join("config")));
        assert_eq!(config.modules_dir, Some(base.join("modules")));
        assert_eq!(config.cache_dir, Some(base.join("cache")));
        assert_eq!(config.socket_path, Some(base.join("alme")));
    }

    #[tokio::test]
    async fn test_load_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();
        let config_dir = base_dir.join("custom_config");
        let config_file_path = config_dir.join("arcella.toml");

        fs::create_dir_all(&base_dir).unwrap();
        fs::create_dir_all(config_dir.as_path()).unwrap();

        let config_content = r#"
            modules_dir = "/tmp/arcella_test/modules"
            socket_path = "/tmp/arcella_test/custom_alme.sock"
        "#;
        fs::write(&config_file_path, config_content).unwrap();

        let default_config = ArcellaConfig {
            base_dir: Some(base_dir.clone().into()),
            config_dir: Some(config_dir.clone().into()),
            log_dir: Some(base_dir.join("log")),
            modules_dir: Some(base_dir.join("modules")),
            cache_dir: Some(base_dir.join("cache")),
            socket_path: Some(base_dir.join("alme")),
        };

        let content = tokio::fs::read_to_string(&config_file_path).await.unwrap();
        let file_config: ConfigFile = toml::from_str(&content).unwrap();

        let final_config = ArcellaConfig {
            base_dir: default_config.base_dir,
            config_dir: default_config.config_dir,
            log_dir: file_config.log_dir.or(default_config.log_dir),
            modules_dir: file_config.modules_dir.or(default_config.modules_dir),
            cache_dir: file_config.cache_dir.or(default_config.cache_dir),
            socket_path: file_config.socket_path.or(default_config.socket_path),
        };

        assert_eq!(final_config.modules_dir, Some(PathBuf::from("/tmp/arcella_test/modules")));
        assert_eq!(final_config.socket_path, Some(PathBuf::from("/tmp/arcella_test/custom_alme.sock")));
        // config_dir осталось из default
        assert_eq!(final_config.config_dir, Some(config_dir.into()));
    }*/
}

