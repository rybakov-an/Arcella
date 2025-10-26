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
//!
//! The primary entry point for parsing a TOML string and extracting its content is
//! [`parse_and_collect`]. For more granular control, you can use [`parse`] to get a
//! `toml_edit::DocumentMut` and then use [`collect_paths`] to extract the data.

use ordered_float::OrderedFloat;
use std::collections::{HashMap};
use toml_edit::{DocumentMut, Item as TomlEditItem, Value as TomlEditValue};

use arcella_types::value::{
    TypedError,
    Value as TomlValue
};

use crate::{ArcellaUtilsError, ArcellaResult};

/// The maximum allowed recursion depth when traversing TOML structures.
/// This prevents potential stack overflow errors from malformed or deeply nested documents.
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
            TomlEditValue::Float(f) => Self::Float(OrderedFloat(*f.value())),
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

#[derive(Debug, Clone, PartialEq)]
pub struct TomlFileData {
    pub includes: Vec<String>,
    pub values: HashMap<String, TomlValue>,
}

/// Recursively traverses a TOML item and collects configuration values and `includes` paths.
///
/// This function walks the TOML structure starting from `item`, accumulating:
/// - Configuration values (non-`includes`) into the `values` map, keyed by their dot-separated path.
/// - Paths specified under keys named `includes` into the `includes` vector.
///
/// The traversal respects a maximum depth (`MAX_TOML_DEPTH`) to prevent infinite recursion in malformed documents.
/// 
/// # Arguments
///
/// * `item` - The TOML item to start traversal from (e.g., a table, array, or value).
/// * `current_path` - The path to the current item in the TOML document, as a vector of strings.
/// * `includes` - A mutable reference to a vector where `includes` paths are collected.
/// * `values` - A mutable reference to a map where configuration key-value pairs are stored.
///              The key is the dot-separated path (e.g., "arcella.servers.alpha.test_int").
/// * `depth` - The current recursion depth.
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

/// Parses a TOML string into a `toml_edit::DocumentMut`.
///
/// This function uses `toml_edit` to parse the input string. If parsing fails,
/// an `ArcellaUtilsError::TOML` error is returned.
///
/// # Arguments
///
/// * `content` - The string content of the TOML file to parse.
///
/// # Returns
///
/// A `Result` containing the parsed `toml_edit::DocumentMut` or an error.
pub fn parse(content: &str) -> ArcellaResult<DocumentMut> {
    content.parse::<DocumentMut>()
        .map_err(|e| ArcellaUtilsError::TOML(format!("{}", e)))
}

/// Collects configuration values and `includes` paths from a parsed `toml_edit::DocumentMut`.
///
/// This function traverses the document starting from the root item and extracts:
/// - Configuration key-value pairs into a `HashMap` with dot-separated keys.
/// - File paths specified under `includes` keys into a `Vec<String>`.
///
/// It uses `collect_paths_recursive` internally.
///
/// # Arguments
///
/// * `doc` - A reference to the parsed `toml_edit::DocumentMut`.
/// * `prefix` - A slice of strings representing the prefix to use for the keys in the returned `values` map.
///
/// # Returns
///
/// A `Result` containing a `TomlFileData` struct with the collected `includes` and `values`.
pub fn collect_paths(
    doc: &DocumentMut, 
    prefix: &[String]
) -> ArcellaResult<TomlFileData> {
    let mut values: HashMap<String, TomlValue> = HashMap::new();
    let mut includes: Vec<String> = vec![];
    let depth = 0;

    collect_paths_recursive(
        doc.as_item(),
        prefix,
        &mut includes,
        &mut values,
        depth,
    )?;

    Ok(TomlFileData{includes, values})
}

/// Parses a TOML file content and collects configuration values and `includes` paths.
/// 
/// This is a convenience function that combines [`parse`] and [`collect_paths`].
/// It is used for parsing the main `arcella.toml` as well as any included files.
/// 
/// # Arguments
/// 
/// * `content` - The string content of the TOML file.
/// * `prefix` - The prefix to use for the keys in the returned `values` map.
/// 
/// # Returns
/// 
/// A `Result` containing a `TomlFileData` struct with:
/// - `includes`: A `Vec<String>` of paths found under `includes` keys.
/// - `values`: A `HashMap<String, TomlValue>` of configuration key-value pairs.
pub fn parse_and_collect(
    content: &str,
    prefix: &[String],
) -> ArcellaResult<TomlFileData> {
    let doc = parse(content)?;
    collect_paths(&doc, prefix)
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

            let config = parse_and_collect(
                config_content,
                &["root".into()]
            ).unwrap();

            let expected_includes = vec!["config.d/*.toml".to_string()];

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("root.server.port".to_string(), TomlValue::Integer(8080));
            expected_values.insert("root.server.host".to_string(), TomlValue::String("localhost".to_string()));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
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

            let config = parse_and_collect(
                config_content,
                &[]
            ).unwrap();

            let expected_includes = Vec::new();

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("database.host".to_string(), TomlValue::String("db.example.com".to_string()));
            expected_values.insert("database.port".to_string(), TomlValue::Integer(5432));
            expected_values.insert("database.pool.max_connections".to_string(), TomlValue::Integer(10));
            expected_values.insert("database.pool.timeout".to_string(), TomlValue::Float(OrderedFloat(30.5)));
            expected_values.insert("logging.level".to_string(), TomlValue::String("info".to_string()));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_with_single_string_includes() {
            let config_content = r#"
            [app]
            name = "my_app"

            includes = "overrides.toml"
            "#;

            let config = parse_and_collect(
                config_content,
                &["config".into()]
            ).unwrap();

            let expected_includes = vec!["overrides.toml".to_string()];

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("config.app.name".to_string(), TomlValue::String("my_app".to_string()));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_with_array_includes() {
            let config_content = r#"
            [app]
            version = "1.0.0"

            includes = ["config.d/*.toml", "local.toml", "secrets.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &["app".into()]
            ).unwrap();

            let expected_includes = vec![
                "config.d/*.toml".to_string(),
                "local.toml".to_string(),
                "secrets.toml".to_string(),
            ];

            let mut expected_values = std::collections::HashMap::new();
            expected_values.insert("app.app.version".to_string(), TomlValue::String("1.0.0".to_string()));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
       }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_empty_content() {
            let config_content = "";

            let config = parse_and_collect(
                config_content,
                &[]
            ).unwrap();

            let expected_includes = Vec::new();
            let expected_values = HashMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_only_includes() {
            let config_content = r#"
            includes = ["a.toml", "b.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &["top".into()]
            ).unwrap();

            let expected_includes = vec!["a.toml".to_string(), "b.toml".to_string()];
            let expected_values = HashMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_invalid_toml() {
            let config_content = r#"
            [app
            name = "broken"
            "#; // Invalid TOML syntax

            let result = parse_and_collect(
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

            let config = parse_and_collect(
                config_content,
                &[]
            ).unwrap();

            let expected_includes = Vec::new();

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

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, expected_config);
        }        

    }    

}
