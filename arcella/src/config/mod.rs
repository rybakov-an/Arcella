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
    let base_dir = find_base_dir().await?;

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


pub fn collect_paths_recursive(
    item: &TomlItem,
    current_path: &[String],
    includes: &mut Vec<String>,
    values: &mut HashMap<String, TValue>,
    depth: usize,
    max_depth: usize,
) -> ArcellaResult<()> {
    if depth > max_depth {
        return Ok(());
    }

    match item {
        TomlItem::Table(table) => {
            for (key, value) in table {
                let mut key_path = current_path.to_vec();
                key_path.push(key.into());

                if key == "includes" {
                    match value {
                        TomlItem::Value(TomlValue::Array(includes_array)) => {
                            for include in includes_array {
                                if let Some(str_val) = include.as_str() {
                                    includes.push(str_val.to_owned());
                                }
                            }
                        },
                        // Also handle a single string value for 'includes'
                        TomlItem::Value(include) => {
                            if let Some(str_val) = include.as_str() {
                                includes.push(str_val.to_owned());
                            }
                        },
                        _ => {} 
                    };
                } else if let TomlItem::Value(subvalue) = value {
                    values.insert(
                        key_path.join("."), 
                        TValue::from_toml_value(subvalue)?
                    );
                } else {
                    collect_paths_recursive(
                        value,
                        &key_path,
                        includes,
                        values,
                        depth + 1,
                        max_depth,
                    )?;                    
                }
            }
        },
        _ => {}
    }        

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum TValue {
    Array(Vec<TValue>),
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl TValue {
    pub fn from_toml_value(value: &TomlValue) -> ArcellaResult<TValue> {
        let result = match value {
            TomlValue::String(s) => Self::String(s.value().into()),
            TomlValue::Integer(i) => Self::Integer(*i.value()),
            TomlValue::Float(f) => Self::Float(*f.value()),
            TomlValue::Boolean(b) => Self::Boolean(*b.value()),
            TomlValue::Array(array) => {
                let inner_values: ArcellaResult<Vec<TValue>> = array
                    .iter()
                    .map(|v| Self::from_toml_value(v)) 
                    .collect();
                Self::Array(inner_values?)
            },
            _ => { 
                return Err(ArcellaError::Config(
                    format!("Unsupported TOML value type: {:?}", value)
                ));
            },
        };

        Ok(result)
    }
}

#[derive(Debug, Clone)]
struct KeyValueWithLevel {
    level: i8,
    value: TValue,
}

fn resolve_include_paths(
    includes: &Vec<String>,
    config_dir: &Path
) -> ArcellaResult<HashSet<PathBuf>> {
    let mut all_paths = HashSet::new();
    for include_pattern in includes {
        let pattern_path = PathBuf::from(include_pattern);
        if pattern_path.is_absolute() {
            // If the path is absolute, leave it as is.
            all_paths.insert(pattern_path);
        } else {
            // If relative, make it relative to config_dir
            all_paths.insert(config_dir.join(&pattern_path));
        }
    }
    Ok(all_paths)
}

/// Checks if a path represents a regular file with a `.toml` extension
/// but *not* a `.template.toml` extension.
fn is_valid_toml_file_path(path: &Path) -> bool {

    // Check that the path has a file extension
    let extension = match path.extension() {
        Some(ext) => ext,
        None => return false,
    };

    // Check that the file extension is `.toml`
    if !extension.eq_ignore_ascii_case("toml") {
        return false;
    }

    // Check, file is not .template.toml
    let file_name = path.file_name().unwrap().to_string_lossy();
    if file_name.to_lowercase().ends_with(".template.toml") {
        return false;
    }

    // If we got here, the file is a valid .toml file
    true
}

pub async fn find_toml_files_in_dir(dir_path: &Path) -> ArcellaResult<Option<Vec<PathBuf>>> {
    // Check that the path exists and is a directory
    let metadata = fs::metadata(dir_path).await
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: dir_path.to_path_buf() })?;

    if !metadata.is_dir() {
        return Ok(None);
    }

    let mut dir_entries = fs::read_dir(dir_path).await
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: dir_path.to_path_buf() })?;

    let mut toml_files = Vec::new();

    while let Some(entry) = dir_entries.next_entry().await
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: dir_path.to_path_buf() })?
    {
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        if is_valid_toml_file_path(&path) {
            toml_files.push(path);
        }
    }

    // Sort the files by name without case sensitivity
    toml_files.sort_by_key(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
    });

    Ok(Some(toml_files))
}

/*pub async fn collect_includes_recursive(
    includes: &Vec<String>,
    config_dir: &Path,
    max_depth: usize,
) -> ArcellaResult<(Vec<String>, HashMap<String, KeyValueWithLevel>)> {
    let mut full_includes = Vec::new();
    let mut values = HashMap::new();

    //Collect all paths matching the patterns in includes
    let all_paths = resolve_include_paths(includes, config_dir).await?;


}*/


async fn find_base_dir() -> ArcellaResult<PathBuf> {
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            if parent.file_name() == Some(std::ffi::OsStr::new("bin")) {
                return Ok(parent.parent().unwrap_or(&current_exe).to_path_buf());
            }
        }

        let current_dir = current_exe.parent().unwrap_or(&current_exe);
        let local_config = current_dir.join("config");
        if local_config.exists() && local_config.is_dir() {
            return Ok(current_dir.to_path_buf());
        }
    }

    dirs::home_dir()
        .map(|d| d.join(".arcella"))
        .ok_or_else(|| ArcellaError::Config("Cannot determine home directory".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[tokio::test]
    async fn test_collect_paths_recursive() {
        let depth = 0;
        let max_depth = 10;

        let config_content = r#"
        [database]
            includes = ["*", "test_1.toml"]
        [servers]
        [servers.alpha]
            includes = "test_2.toml"
            test_string = "string"
            test_int = 10
            test_bool = true
        "#;

        let main_doc = config_content.parse::<DocumentMut>().unwrap();

        let mut values: HashMap<String, TValue> = HashMap::new();
        let mut includes: Vec<String> = vec![];

        let result = collect_paths_recursive(
            main_doc.as_item(),
            &["arcella".into()],
            &mut includes,
            &mut values,
            depth,
            max_depth,
        );
        assert!(result.is_ok());

        let expected_includes = vec!["*".to_string(), "test_1.toml".to_string(), "test_2.toml".to_string()];
        assert_eq!(includes, expected_includes);

        let mut expected_values = std::collections::HashMap::new();
        expected_values.insert("arcella.servers.alpha.test_string".to_string(), TValue::String("string".to_string()));
        expected_values.insert("arcella.servers.alpha.test_int".to_string(), TValue::Integer(10));
        expected_values.insert("arcella.servers.alpha.test_bool".to_string(), TValue::Boolean(true));

        assert_eq!(values, expected_values);        

    }

    mod find_toml_tests {
        use super::*;
        use tempfile::TempDir;
        use std::fs;

        #[tokio::test]
        async fn test_find_toml_files_in_dir_valid_directory() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create some test files
            fs::write(dir_path.join("config1.toml"), "# Config 1").unwrap();
            fs::write(dir_path.join("config2.toml"), "# Config 2").unwrap();
            fs::write(dir_path.join("not_a_config.txt"), "Text file").unwrap();
            fs::write(dir_path.join("Config3.TOML"), "# Config 3 (uppercase)").unwrap(); // Проверка case-insensitivity
            fs::write(dir_path.join("template.template.toml"), "# Template").unwrap(); // Должен быть исключен
            fs::write(dir_path.join("normal.template.toml"), "# Normal Template").unwrap(); // Должен быть исключен

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 3); // config1.toml, config2.toml, Config3.TOML

            let expected_names: Vec<String> = vec![
                "config1.toml",
                "config2.toml", 
                "Config3.TOML"
            ].into_iter().map(|s| s.to_string()).collect();

            let actual_names: Vec<String> = files.iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect();

            assert_eq!(actual_names, expected_names);
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_empty_directory() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert!(files.is_empty());
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_nonexistent_path() {
            let nonexistent_path = Path::new("/this/path/definitely/does/not/exist/arcella_test_dir");
            
            let result = find_toml_files_in_dir(nonexistent_path).await;
            
            assert!(result.is_err());
            // Проверяем, что ошибка — это IoWithPath
            match result.unwrap_err() {
                ArcellaError::IoWithPath { .. } => {}, // OK
                _ => panic!("Expected ArcellaError::IoWithPath"),
            }
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_file_instead_of_dir() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("not_a_dir.toml");
            fs::write(&file_path, "# Just a file").unwrap();

            let result = find_toml_files_in_dir(&file_path).await.unwrap();
            
            assert!(result.is_none()); // Путь — файл, а не директория
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_nested_dirs_ignored() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create a subdirectory
            let sub_dir = dir_path.join("subdir");
            fs::create_dir(&sub_dir).unwrap();

            // Create .toml files in both the main dir and sub dir
            fs::write(dir_path.join("main_config.toml"), "# Main").unwrap();
            fs::write(sub_dir.join("nested_config.toml"), "# Nested").unwrap(); // Этот не должен быть найден

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 1); // Только main_config.toml
            assert!(files[0].file_name().unwrap().to_string_lossy().contains("main_config.toml"));
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_sorted_order() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create .toml files in non-lexicographic order
            fs::write(dir_path.join("z.toml"), "# Z").unwrap();
            fs::write(dir_path.join("a.toml"), "# A").unwrap();
            fs::write(dir_path.join("m.toml"), "# M").unwrap();

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 3);
            // Проверяем, что файлы отсортированы
            assert!(files[0].file_name().unwrap().to_string_lossy() == "a.toml");
            assert!(files[1].file_name().unwrap().to_string_lossy() == "m.toml");
            assert!(files[2].file_name().unwrap().to_string_lossy() == "z.toml");
        }
    }    

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

