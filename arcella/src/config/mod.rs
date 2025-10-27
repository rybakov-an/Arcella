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
use std::collections::HashMap;
use indexmap::{map::Entry, IndexMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;

use arcella_types::{
    config::Value as TomlValue
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

struct ResolvedValue {
    value: TomlValue,
    source_layer: usize,
    granted_by: usize,
}

pub async fn load() -> ArcellaResult<(ArcellaConfig, Vec<fs_utils::ConfigLoadWarning>)> {
    
    // 1. Find base_dir
    let base_dir = fs_utils::find_base_dir().await?;

    // 2. Set config_dir
    let config_dir = base_dir.join("config");    

    // 3. Ensure config_dir exists
    let (main_config_path, mut warnings) = ensure_config_template(&config_dir).await?;

    let default_config = fs_utils::toml::parse_and_collect(
        DEFAULT_CONFIG_CONTENT,
        &["arcella".to_string()],
    )?;

    // 4. Load arcella.toml
    let configs = fs_utils::load_config_recursive_from_file(
        &["arcella".to_string()],
        &main_config_path,
        &config_dir,
        &mut warnings,
    ).await?;

    let mut final_values = merge_config(
        default_config,
        configs,
        &mut warnings,
    )?;
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

fn merge_config(
    default_config: fs_utils::TomlFileData,
    configs: Vec<fs_utils::TomlFileData>,
    warnings: &mut Vec<fs_utils::ConfigLoadWarning>
) -> Result< IndexMap<String, (TomlValue, usize)>, ArcellaError> {
    
    let mut preliminary_values: IndexMap<String, ResolvedValue> = IndexMap::new();

    // Обрабатываем от низшего приоритета к высшему (но по индексу — от высокого к низкому)
    for layer_idx in (0..configs.len()).rev() {
        let config = &configs[layer_idx];
        for (key, value) in &config.values {
            // Check if the key ends with #redef
            let (actual_key, is_redef) = if key.ends_with(REDEF_SUFFIX) {
                // Extract the original key without the #redef suffix
                let original_key = key[..key.len() - REDEF_SUFFIX.len()].to_string();
                (original_key, true)
            } else {
                (key.clone(), false)
            };

            match preliminary_values.entry(actual_key.clone()) {
                Entry::Occupied(mut e) => {
                    // Текущий слой имеет БОЛЕЕ ВЫСОКИЙ приоритет (меньший idx), чем e.get().source_layer
                    if !is_redef { 
                        // Более приоритетный слой задаёт значение — перезаписываем
                        warnings.push(fs_utils::ConfigLoadWarning::ValueError {
                            key: actual_key.clone(),
                            error: format!(
                                "Value from layer {} ignored due to no #redef flag in layer {}",
                                e.get().granted_by,
                                layer_idx,
                            ),
                            file: PathBuf::from(format!("layer_{}.toml", layer_idx)),
                        });
                        // Заменяем значение текущим
                        e.get_mut().value = value.clone();
                        e.get_mut().source_layer = layer_idx;
                    }
                    e.get_mut().granted_by = layer_idx;
                }
                Entry::Vacant(_) => {
                    // Место с этим ключом вакантно
                    preliminary_values.insert(
                        actual_key, 
                        ResolvedValue {
                            value: value.clone(),
                            source_layer: layer_idx,
                            granted_by: layer_idx,   
                        }
                    );
                }
            }

        }
    }

    let mut final_values: IndexMap<String, (TomlValue, usize)> = IndexMap::new();

    // Create final config from default config
    // Выполняем первисное заполнение из конфигурации по умолчанию
    for (key, value) in &default_config.values {
        final_values.insert(
            key.clone(), 
            (value.clone(), 0)
        );  
    }

    // Merge preliminary values
    for (key, preliminary_value) in &preliminary_values {
        // Флаг говорит о том, что раздел конфигурации допускает 
        // доопределение параметров отсутствующих в конфигурации по умолчанию
        let is_newable = key.starts_with("arcella.custom") 
            || key.starts_with("arcella.modules");
        let new_value = &preliminary_value.value;
        let insert_index = preliminary_value.source_layer;

        match final_values.entry(key.clone()) {
            Entry::Occupied(mut entry) => {
                // Значение с данным ключем есть в конфигурации по умолчанию
                if preliminary_value.granted_by == 0 {
                    // Это значение было в основной конфигурации поэтому
                    // его можно использовать для замены значения по умолчанию
                    entry.insert(
                        (new_value.clone(), insert_index)
                    );
                } else {
                    // Для замены значения по умолчанию в основной конфигурации
                    // ключ параметра должен иметь суффикс #redef
                    warnings.push(fs_utils::ConfigLoadWarning::ValueError {
                        key: key.clone(),
                        error: format!(
                            "Value from layer {} ignored due to #redef missing in arcella.toml",
                            insert_index,
                        ),
                        file: PathBuf::from(format!("layer_{}.toml", insert_index)),
                    })

                }
            }
            Entry::Vacant(_) => {
                // В конфигурации по умолчанию отсутствует данный ключ поэтому проверяем
                // что новый параметр вставляется в разделы arcella.custom или arcella.modules
                if is_newable {
                    final_values.insert(
                        key.clone(), 
                        (new_value.clone(), insert_index)
                    );
                } else {
                    // В этот раздел добавлять новые параметры нельзя
                    warnings.push(fs_utils::ConfigLoadWarning::ValueError {
                        key: key.clone(),
                        error: format!(
                            "Value from layer {} ignored due to missing in default config",
                            insert_index
                        ),
                        file: PathBuf::from(format!("layer_{}.toml", insert_index)),
                    });
                }
            }
        }
    };

    Ok(final_values)

}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_toml_value(s: &str) -> TomlValue {
        TomlValue::String(s.to_string())
    }

    #[test]
    fn test_merge_config_example_from_docs() {
        // Встроенный конфиг по умолчанию (layer 0)
        let mut default_values = IndexMap::new();
        default_values.insert("arcella.log.level".to_string(), make_toml_value("info"));
        default_values.insert("arcella.log.file".to_string(), make_toml_value("arcella_default.log"));
        default_values.insert("arcella.server.port".to_string(), make_toml_value("8080"));
        default_values.insert("arcella.server.host".to_string(), make_toml_value("0.0.0.0"));

        let default_config = fs_utils::TomlFileData {
            includes: vec![],
            values: default_values,
        };

        // arcella.toml (layer 1)
        let mut main_config_values = IndexMap::new();
        // level#redef позволяет переопределение
        main_config_values.insert("arcella.log.level#redef".to_string(), make_toml_value("warn"));
        main_config_values.insert("arcella.log.file".to_string(), make_toml_value("arcella_main.log"));
        main_config_values.insert("arcella.server.port".to_string(), make_toml_value("9000"));

        let main_config = fs_utils::TomlFileData {
            includes: vec![],
            values: main_config_values,
        };

        // level_1.toml (layer 2, предполагаем, что он загружен через includes)
        let mut level_1_values = IndexMap::new();
        level_1_values.insert("arcella.log.level".to_string(), make_toml_value("debug"));
        level_1_values.insert("arcella.server.host".to_string(), make_toml_value("127.0.0.1")); // Этот ключ не помечен как #redef в arcella.toml -> игнорируется
        level_1_values.insert("arcella.server.name".to_string(), make_toml_value("www.server.net")); // Новый ключ в arcella.server -> игнорируется
        level_1_values.insert("arcella.custom.message".to_string(), make_toml_value("Это дополнительный параметр")); // Новый ключ в arcella.custom -> разрешено

        let level_1_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_1_values,
        };

        let configs = vec![main_config, level_1_config];

        let mut warnings = vec![];
        let result = merge_config(default_config, configs, &mut warnings).expect("merge_config should succeed");

        // Проверяем итоговую конфигурацию
        assert_eq!(result.get("arcella.log.level"), Some(&(make_toml_value("debug"), 1))); // Переопределено из level_1.toml
        assert_eq!(result.get("arcella.log.file"), Some(&(make_toml_value("arcella_main.log"), 0))); // Из arcella.toml
        assert_eq!(result.get("arcella.server.port"), Some(&(make_toml_value("9000"), 0))); // Из arcella.toml
        assert_eq!(result.get("arcella.server.host"), Some(&(make_toml_value("0.0.0.0"), 0))); // Осталось из default_config.toml
        assert_eq!(result.get("arcella.custom.message"), Some(&(make_toml_value("Это дополнительный параметр"), 1))); // Из level_1.toml

        // Проверяем предупреждения
        assert_eq!(warnings.len(), 2);

        let warning1 = &warnings[0];
        match warning1 {
            fs_utils::ConfigLoadWarning::ValueError { key, error, .. } => {
                assert_eq!(key, "arcella.server.host");
                assert!(error.contains("ignored due to #redef missing in arcella.toml"));
            }
            _ => panic!("Expected ValueError for arcella.server.host"),
        }

        let warning2 = &warnings[1];
        match warning2 {
            fs_utils::ConfigLoadWarning::ValueError { key, error, .. } => {
                assert_eq!(key, "arcella.server.name");
                assert!(error.contains("ignored due to missing in default config"));
            }
            _ => panic!("Expected ValueError for arcella.server.name"),
        }
    }

    #[test]
    fn test_merge_config_no_redef_prevents_override() {
        // default_config (layer 0)
        let mut default_values = IndexMap::new();
        default_values.insert("arcella.server.host".to_string(), make_toml_value("0.0.0.0"));
        default_values.insert("arcella.server.port".to_string(), make_toml_value("8090"));
        let default_config = fs_utils::TomlFileData {
            includes: vec![],
            values: default_values,
        };

        // arcella.toml (layer 1) - не помечает host как #redef
        let mut main_config_values = IndexMap::new();
        main_config_values.insert("arcella.server.host".to_string(), make_toml_value("192.168.1.1"));
        let main_config = fs_utils::TomlFileData {
            includes: vec![],
            values: main_config_values,
        };

        // level_1.toml (layer 2)
        let mut level_1_values = IndexMap::new();
        level_1_values.insert("arcella.server.port".to_string(), make_toml_value("9000"));
        let level_1_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_1_values,
        };

        // level_2.toml (layer 3) - пытается изменить host
        let mut level_2_values = IndexMap::new();
        level_2_values.insert("arcella.server.host".to_string(), make_toml_value("127.0.0.1"));
        let level_2_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_2_values,
        };

        let configs = vec![main_config, level_1_config, level_2_config];

        let mut warnings = vec![];
        let result = merge_config(default_config, configs, &mut warnings).expect("merge_config should succeed");

        assert_eq!(result.get("arcella.server.host"), Some(&(make_toml_value("192.168.1.1"), 0))); // Остается значение из arcella.toml

        assert_eq!(warnings.len(), 2);
        let warning_1 = &warnings[0];
        match warning_1 {
            fs_utils::ConfigLoadWarning::ValueError { key, error, .. } => {
                assert_eq!(key, "arcella.server.host");
                assert!(error.contains("Value from layer 2 ignored due to no #redef flag in layer 0"));
            }
            _ => panic!("Expected ValueError for arcella.server.host due to missing #redef in arcella.toml when layer 2 tried to set it"),
        }
        let warning_2 = &warnings[1];
        match warning_2 {
            fs_utils::ConfigLoadWarning::ValueError { key, error, .. } => {
                assert_eq!(key, "arcella.server.port");
                assert!(error.contains("Value from layer 1 ignored due to #redef missing in arcella.toml"));
            }
            _ => panic!("Expexted ValueError for arcella.server.port due to missing #redef in arcella.toml when layer 1 tried to set it"),
        }
    }

    #[test]
    fn test_merge_config_redef_allows_override() {
        // default_config (layer 0)
        let mut default_values = IndexMap::new();
        default_values.insert("arcella.log.level".to_string(), make_toml_value("info"));
        let default_config = fs_utils::TomlFileData {
            includes: vec![],
            values: default_values,
        };

        // arcella.toml (layer 1) - помечает level как #redef
        let mut main_config_values = IndexMap::new();
        main_config_values.insert("arcella.log.level#redef".to_string(), make_toml_value("warn"));
        let main_config = fs_utils::TomlFileData {
            includes: vec![],
            values: main_config_values,
        };

        // level_1.toml (layer 2) - может изменить level, так как arcella.toml пометила его как #redef
        let mut level_1_values = IndexMap::new();
        level_1_values.insert("arcella.log.level#redef".to_string(), make_toml_value("debug"));
        let level_1_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_1_values,
        };

        // level_2.toml (layer 3) - может изменить level, так как level_1.toml пометил его как #redef
        let mut level_2_values = IndexMap::new();
        level_2_values.insert("arcella.log.level".to_string(), make_toml_value("trace"));
        let level_2_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_2_values,
        };

        let configs = vec![main_config, level_1_config, level_2_config];

        let mut warnings = vec![];
        let result = merge_config(default_config, configs, &mut warnings).expect("merge_config should succeed");

        // Значение level должно быть переопределено из level_1.toml, так как #redef разрешил это в arcella.toml
        assert_eq!(result.get("arcella.log.level"), Some(&(make_toml_value("trace"), 2)));
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_merge_config_new_key_in_custom_allowed() {
        // default_config (layer 0)
        let mut default_values = IndexMap::new();
        default_values.insert("arcella.log.level".to_string(), make_toml_value("info"));
        let default_config = fs_utils::TomlFileData {
            includes: vec![],
            values: default_values,
        };

        // arcella.toml (layer 1)
        let main_config_values = IndexMap::new(); // Пустой
        let main_config = fs_utils::TomlFileData {
            includes: vec![],
            values: main_config_values,
        };

        // level_1.toml (layer 2) - добавляет новый ключ в arcella.custom
        let mut level_1_values = IndexMap::new();
        level_1_values.insert("arcella.custom.new_key".to_string(), make_toml_value("new_value"));
        let level_1_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_1_values,
        };

        let configs = vec![main_config, level_1_config];

        let mut warnings = vec![];
        let result = merge_config(default_config, configs, &mut warnings).expect("merge_config should succeed");

        assert_eq!(result.get("arcella.custom.new_key"), Some(&(make_toml_value("new_value"), 1)));
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_merge_config_new_key_in_server_ignored() {
        // default_config (layer 0)
        let mut default_values = IndexMap::new();
        default_values.insert("arcella.log.level".to_string(), make_toml_value("info"));
        let default_config = fs_utils::TomlFileData {
            includes: vec![],
            values: default_values,
        };

        // arcella.toml (layer 1)
        let main_config_values = IndexMap::new(); // Пустой
        let main_config = fs_utils::TomlFileData {
            includes: vec![],
            values: main_config_values,
        };

        // level_1.toml (layer 2) - пытается добавить новый ключ в arcella.server
        let mut level_1_values = IndexMap::new();
        level_1_values.insert("arcella.server.new_option".to_string(), make_toml_value("some_value"));
        let level_1_config = fs_utils::TomlFileData {
            includes: vec![],
            values: level_1_values,
        };

        let configs = vec![main_config, level_1_config];

        let mut warnings = vec![];
        let result = merge_config(default_config, configs, &mut warnings).expect("merge_config should succeed");

        // Новый ключ не должен появиться
        assert!(!result.contains_key("arcella.server.new_option"));
        // Должно быть предупреждение
        assert_eq!(warnings.len(), 1);
        let warning = &warnings[0];
        match warning {
            fs_utils::ConfigLoadWarning::ValueError { key, error, .. } => {
                assert_eq!(key, "arcella.server.new_option");
                assert!(error.contains("ignored due to missing in default config"));
            }
            _ => panic!("Expected ValueError for new key in arcella.server"),
        }
    }    

}

