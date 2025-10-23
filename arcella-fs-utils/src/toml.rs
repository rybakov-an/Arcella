// arcella/arcella-fs-utils/src/toml.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! TOML parsing and value conversion utilities for Arcella.
//!
//! This module provides functions for:
//! - Converting `toml_edit` values into `arcella_types::Value` for consistent representation.
//! - Recursively traversing TOML documents to extract configuration values and `includes` paths.

use std::collections::{HashMap};
use toml_edit::{DocumentMut, Item as TomlEditItem, Value as TomlEditValue};

use arcella_types::{
    value::Value as TomlValue
};

use crate::{ArcellaUtilsError, ArcellaResult};

pub const MAX_TOML_DEPTH: usize = 10;

/// Extension trait for converting `toml_edit::Value` into `arcella_types::Value`.
///
/// This allows for a consistent representation of TOML values across Arcella components.
pub trait ValueExt {
    /// Converts a `toml_edit::Value` into a `arcella_types::Value`.
    ///
    /// # Arguments
    ///
    /// * `value` - The `toml_edit::Value` to convert.
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted `arcella_types::Value` or an error
    /// if the TOML type is unsupported.
    fn from_toml_value(value: &TomlEditValue) -> ArcellaResult<TomlValue>;
}

impl ValueExt for TomlValue {
    fn from_toml_value(value: &TomlEditValue) -> ArcellaResult<TomlValue> {
        let result = match value {
            TomlEditValue::String(s) => Self::String(s.value().into()),
            TomlEditValue::Integer(i) => Self::Integer(*i.value()),
            TomlEditValue::Float(f) => Self::Float(*f.value()),
            TomlEditValue::Boolean(b) => Self::Boolean(*b.value()),
            TomlEditValue::Array(array) => {
                let inner_values: ArcellaResult<Vec<TomlValue>> = array
                    .iter()
                    .map(|v| Self::from_toml_value(v)) 
                    .collect();
                Self::Array(inner_values?)
            },
            _ => { 
                return Err(ArcellaUtilsError::TOML(
                    format!("Unsupported TOML value type: {:?}", value)
                ));
            },
        };

        Ok(result)
    }
}

/// Recursively traverses a TOML item and collects configuration values and `includes` paths.
///
/// This function walks the TOML structure starting from `item`, accumulating:
/// - Configuration values (non-`includes`) into the `values` map, keyed by their dot-separated path.
/// - Paths specified under keys named `includes` into the `includes` vector.
///
/// The traversal respects a maximum depth to prevent infinite recursion in malformed documents.
///
/// # Arguments
///
/// * `item` - The TOML item to start traversal from (e.g., a table, array, or value).
/// * `current_path` - The path to the current item in the TOML document, as a vector of strings.
/// * `includes` - A mutable reference to a vector where `includes` paths are collected.
/// * `values` - A mutable reference to a map where configuration key-value pairs are stored.
///              The key is the dot-separated path (e.g., "arcella.servers.alpha.test_int").
/// * `depth` - The current recursion depth.
/// * `max_depth` - The maximum allowed recursion depth.
///
/// # Returns
///
/// A `Result` indicating success or an error if a TOML value type cannot be converted.
pub fn collect_paths_recursive(
    item: &TomlEditItem,
    current_path: &[String],
    includes: &mut Vec<String>,
    values: &mut HashMap<String, TomlValue>,
    depth: usize,
) -> ArcellaResult<()> {
    if depth > MAX_TOML_DEPTH {
        return Ok(());
    }

    match item {
        TomlEditItem::Table(table) => {
            for (key, value) in table {
                let mut key_path = current_path.to_vec();
                key_path.push(key.into());

                if key == "includes" {
                    match value {
                        TomlEditItem::Value(TomlEditValue::Array(includes_array)) => {
                            for include in includes_array {
                                if let Some(str_val) = include.as_str() {
                                    includes.push(str_val.to_owned());
                                }
                            }
                        },
                        // Also handle a single string value for 'includes'
                        TomlEditItem::Value(include) => {
                            if let Some(str_val) = include.as_str() {
                                includes.push(str_val.to_owned());
                            }
                        },
                        _ => {} 
                    };
                } else if let TomlEditItem::Value(subvalue) = value {
                    values.insert(
                        key_path.join("."), 
                        TomlValue::from_toml_value(subvalue)?
                    );
                } else {
                    collect_paths_recursive(
                        value,
                        &key_path,
                        includes,
                        values,
                        depth + 1,
                    )?;                    
                }
            }
        },
        _ => {}
    }        

    Ok(())
}

/// Parses a TOML file content and collects configuration values and `includes` paths.
/// 
/// This function is used for parsing the main `arcella.toml` as well as any included files.
/// 
/// # Arguments
/// 
/// * `content` - The string content of the TOML file.
/// * `current_path_prefix` - The prefix to use for the keys in the returned `values` map.
/// 
/// # Returns
/// 
/// A tuple containing:
/// - A `Vec<String>` of paths found under `includes` keys.
/// - A `HashMap<String, TomlValue>` of configuration key-value pairs.
pub fn parse_config_and_collect_includes(
    content: &str,
    current_path_prefix: &[String],
) -> ArcellaResult<(Vec<String>, HashMap<String, TomlValue>)> {
    let doc = content.parse::<DocumentMut>()
        .map_err(|e| ArcellaUtilsError::TOML(format!("{}", e)))?;

    let mut values: HashMap<String, TomlValue> = HashMap::new();
    let mut includes: Vec<String> = vec![];

    let depth = 0;

    collect_paths_recursive(
        doc.as_item(),
        current_path_prefix,
        &mut includes,
        &mut values,
        depth,
    )?;

    Ok((includes, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_collect_paths_recursive() {
        let depth = 0;

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

        let mut values: HashMap<String, TomlValue> = HashMap::new();
        let mut includes: Vec<String> = vec![];

        let result = collect_paths_recursive(
            main_doc.as_item(),
            &["arcella".into()],
            &mut includes,
            &mut values,
            depth,
        );
        assert!(result.is_ok());

        let expected_includes = vec!["*".to_string(), "test_1.toml".to_string(), "test_2.toml".to_string()];
        assert_eq!(includes, expected_includes);

        let mut expected_values = std::collections::HashMap::new();
        expected_values.insert("arcella.servers.alpha.test_string".to_string(), TomlValue::String("string".to_string()));
        expected_values.insert("arcella.servers.alpha.test_int".to_string(), TomlValue::Integer(10));
        expected_values.insert("arcella.servers.alpha.test_bool".to_string(), TomlValue::Boolean(true));

        assert_eq!(values, expected_values);        

    }

    mod parse_config_and_collect_includes_tests {
       use super::*;

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_simple() {
            let config_content = r#"
            [server]
            port = 8080
            host = "localhost"

            includes = ["config.d/*.toml"]
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &["root".into()]
            ).unwrap();

            let expected_includes = vec!["config.d/*.toml".to_string()];
            assert_eq!(includes, expected_includes);

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("root.server.port".to_string(), TomlValue::Integer(8080));
            expected_values.insert("root.server.host".to_string(), TomlValue::String("localhost".to_string()));

            assert_eq!(values, expected_values);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_nested() {
            let config_content = r#"
            [database]
            host = "db.example.com"
            port = 5432

            [database.pool]
            max_connections = 10
            timeout = 30.5

            [logging]
            level = "info"
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &[]
            ).unwrap();

            assert!(includes.is_empty());

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("database.host".to_string(), TomlValue::String("db.example.com".to_string()));
            expected_values.insert("database.port".to_string(), TomlValue::Integer(5432));
            expected_values.insert("database.pool.max_connections".to_string(), TomlValue::Integer(10));
            expected_values.insert("database.pool.timeout".to_string(), TomlValue::Float(30.5));
            expected_values.insert("logging.level".to_string(), TomlValue::String("info".to_string()));

            assert_eq!(values, expected_values);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_with_single_string_includes() {
            let config_content = r#"
            [app]
            name = "my_app"

            includes = "overrides.toml"
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &["config".into()]
            ).unwrap();

            let expected_includes = vec!["overrides.toml".to_string()];
            assert_eq!(includes, expected_includes);

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("config.app.name".to_string(), TomlValue::String("my_app".to_string()));

            assert_eq!(values, expected_values);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_with_array_includes() {
            let config_content = r#"
            [app]
            version = "1.0.0"

            includes = ["config.d/*.toml", "local.toml", "secrets.toml"]
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &["app".into()]
            ).unwrap();

            let expected_includes = vec![
                "config.d/*.toml".to_string(),
                "local.toml".to_string(),
                "secrets.toml".to_string(),
            ];
            assert_eq!(includes, expected_includes);

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("app.app.version".to_string(), TomlValue::String("1.0.0".to_string()));

            assert_eq!(values, expected_values);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_empty_content() {
            let config_content = "";

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &[]
            ).unwrap();

            assert!(includes.is_empty());
            assert!(values.is_empty());
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_only_includes() {
            let config_content = r#"
            includes = ["a.toml", "b.toml"]
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &["top".into()]
            ).unwrap();

            let expected_includes = vec!["a.toml".to_string(), "b.toml".to_string()];
            assert_eq!(includes, expected_includes);

            assert!(values.is_empty());
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_invalid_toml() {
            let config_content = r#"
            [app
            name = "broken"
            "#; // Invalid TOML syntax

            let result = parse_config_and_collect_includes(
                config_content,
                &[]
            );

            assert!(result.is_err());
            match result.unwrap_err() {
                ArcellaUtilsError::TOML(_) => {} // OK
                _ => panic!("Expected ArcellaUtilsError::TOML"),
            }
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_with_boolean_and_array_values() {
            let config_content = r#"
            [features]
            enabled = true
            disabled = false

            [features.flags]
            list = ["flag1", "flag2", "flag3"]

            [server]
            ports = [80, 443, 8080]
            "#;

            let (includes, values) = parse_config_and_collect_includes(
                config_content,
                &[]
            ).unwrap();

            assert!(includes.is_empty());

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("features.enabled".to_string(), TomlValue::Boolean(true));
            expected_values.insert("features.disabled".to_string(), TomlValue::Boolean(false));
            expected_values.insert("features.flags.list".to_string(), TomlValue::Array(vec![
                TomlValue::String("flag1".to_string()),
                TomlValue::String("flag2".to_string()),
                TomlValue::String("flag3".to_string()),
            ]));
            expected_values.insert("server.ports".to_string(), TomlValue::Array(vec![
                TomlValue::Integer(80),
                TomlValue::Integer(443),
                TomlValue::Integer(8080),
            ]));

            assert_eq!(values, expected_values);
        }        

    }    

}
