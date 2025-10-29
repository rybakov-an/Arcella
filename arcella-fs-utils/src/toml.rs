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
//! This module provides:
//! - Safe conversion from `toml_edit::Value` to Arcella’s internal `Value` type.
//! - Recursive traversal of TOML documents to extract:
//!   - Configuration key-value pairs (stored with dot-separated paths).
//!   - File inclusion directives under keys named `includes`.
//!
//! The traversal respects a maximum depth limit to prevent stack overflow.
//! Unsupported TOML types (e.g., datetimes, inline tables) result in an error.
//!
//! # Entry Points
//!
//! - [`parse_and_collect`] — high-level function for parsing and extracting data.
//! - [`parse`] + [`collect_paths`] — for more granular control.

use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use toml_edit::{DocumentMut, Item as TomlEditItem, Value as TomlEditValue};

use arcella_types::config::{ConfigValues, Value as TomlValue};

use crate::{ArcellaUtilsError, ArcellaResult};
use crate::types::*;

/// Extension trait to convert `toml_edit::Value` into Arcella’s canonical `Value`.
///
/// Only TOML scalar types and arrays of scalars are supported.
/// The following TOML types are **not supported** and will cause an error:
/// - Datetime
/// - Inline tables
///
/// Arrays are supported recursively, but must contain only supported scalar types.
pub trait ValueExt {
    /// Converts a `toml_edit::Value` into Arcella’s `Value`.
    ///
    /// # Errors
    ///
    /// Returns `ArcellaUtilsError::TOML` if the value contains an unsupported type
    /// (e.g., datetime, inline table, or nested unsupported structure).
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
                let inner_values: Vec<TomlValue> = array
                    .iter()
                    .map(|v| Self::from_toml_value(v)) 
                    .collect::<ArcellaResult<_>>()?;
                Self::Array(inner_values)
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

/// Recursively traverses a TOML item to collect configuration values and `includes` directives.
///
/// This function walks the TOML structure starting from `item`, building dot-separated
/// configuration keys from the current path. It handles two special cases:
///
/// - Keys named `"includes"` are treated as file inclusion directives. Their values
///   may be either a string or an array of strings; all valid string values are added
///   to the `includes` output vector.
/// - All other scalar values are converted and stored in `values` with their full path.
///
/// Table nesting deeper than [`MAX_TOML_DEPTH`] is pruned (not traversed further),
/// and the function returns [`TraversalResult::Pruned`].
///
/// # Arguments
///
/// * `item` – The TOML item to traverse (typically a table root).
/// * `current_path` – The hierarchical path to this item (e.g., `["arcella", "server"]`).
/// * `file_idx` – A unique index identifying the source file (used for value provenance).
/// * `includes` – Mutable vector to collect inclusion paths.
/// * `values` – Mutable map to store configuration key-value pairs.
/// * `depth` – Current recursion depth (should start at 0).
///
/// # Returns
///
/// * `Ok(TraversalResult::Full)` if the entire subtree was processed.
/// * `Ok(TraversalResult::Pruned)` if traversal was stopped due to depth limit.
/// * `Err(...)` if a value could not be converted (e.g., unsupported type).
pub fn collect_paths_recursive(
    item: &TomlEditItem,
    current_path: &[String],
    file_idx: usize, 
    includes: &mut Vec<String>,
    values: &mut ConfigValues,
    depth: usize,
) -> ArcellaResult<TraversalResult> {
    if depth > MAX_TOML_DEPTH {
        return Ok(TraversalResult::Pruned);
    }
    let mut result = TraversalResult::Full; 

    match item {
        TomlEditItem::Table(table) => {
            for (key, value) in table {
                let mut key_path = current_path.to_vec();
                key_path.push(key.to_string());

                if key == "includes" {
                // Handle both scalar and array forms of 'includes'
                    match value {
                        TomlEditItem::Value(TomlEditValue::Array(arr)) => {
                            for elem in arr {
                                if let Some(s) = elem.as_str() {
                                    includes.push(s.to_owned());
                                }
                            }
                        }
                        // Also handle a single string value for 'includes'
                        TomlEditItem::Value(single) => {
                            if let Some(s) = single.as_str() {
                                includes.push(s.to_owned());
                            }
                        }
                        _ => {} 
                    };
                } else if let TomlEditItem::Value(subvalue) = value {
                    // Scalar value: convert and store with full path
                    let converted = TomlValue::from_toml_value(subvalue)?;
                    values.insert(key_path.join("."), (converted, file_idx));
                } else {
                    // Nested table or other composite item: recurse
                     let child_result = collect_paths_recursive(
                        value,
                        &key_path,
                        file_idx,
                        includes,
                        values,
                        depth + 1,
                    )?;
                    if child_result == TraversalResult::Pruned {
                        result = TraversalResult::Pruned;
                    }
                }
            }
        },
        _ => {}
    }        

    Ok(result)
}

/// Parses a TOML string into a mutable `toml_edit::DocumentMut`.
///
/// Preserves formatting and comments, which is useful for tooling.
///
/// # Errors
///
/// Returns `ArcellaUtilsError::TOML` if the input is not valid TOML.
pub fn parse(content: &str) -> ArcellaResult<DocumentMut> {
    content
        .parse::<DocumentMut>()
        .map_err(|e| ArcellaUtilsError::TOML(format!("{}", e)))
}

/// Extracts configuration data from a parsed TOML document.
///
/// Traverses the document root and collects:
/// - All scalar values (with dot-separated keys prefixed by `prefix`).
/// - All `includes` directives (as raw strings).
///
/// # Arguments
///
/// * `doc` – Parsed TOML document.
/// * `prefix` – Path prefix for all collected keys (e.g., `["arcella"]`).
/// * `file_idx` – Unique identifier for the source file (for provenance tracking).
///
/// # Returns
///
/// A tuple of:
/// - [`TomlFileData`] containing `includes` and `values`.
/// - [`TraversalResult`] indicating whether traversal was complete or pruned.
pub fn collect_paths(
    doc: &DocumentMut, 
    prefix: &[String],
    file_idx: usize,
) -> ArcellaResult<(TomlFileData, TraversalResult)> {
    let mut values: ConfigValues = IndexMap::new();
    let mut includes: Vec<String> = Vec::new();
    let result = collect_paths_recursive(
        doc.as_item(),
        prefix,
        file_idx,
        &mut includes,
        &mut values,
        0,
    )?;

    Ok((TomlFileData{includes, values}, result))
}

/// Parses TOML content and extracts configuration data in one step.
///
/// This is the primary entry point for loading a single configuration file.
///
/// # Arguments
///
/// * `content` – Raw TOML content as a string.
/// * `prefix` – Key prefix for namespacing (e.g., to avoid collisions between files).
/// * `file_idx` – Unique index of the file in the loading sequence.
///
/// # Returns
///
/// Same as [`collect_paths`]: a `TomlFileData` and traversal result.
pub fn parse_and_collect(
    content: &str,
    prefix: &[String],
    file_idx: usize,
) -> ArcellaResult<(TomlFileData, TraversalResult)> {
    let doc = parse(content)?;
    collect_paths(&doc, prefix, file_idx)
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

        let mut values: ConfigValues = IndexMap::new();
        let mut includes: Vec<String> = vec![];

        let result = collect_paths_recursive(
            main_doc.as_item(),
            &vec!["arcella".to_string()],
            0,
            &mut includes,
            &mut values,
            depth,
        );
        assert!(result.is_ok());

        let expected_includes = vec!["*".to_string(), "test_1.toml".to_string(), "test_2.toml".to_string()];
        assert_eq!(includes, expected_includes);

        let mut expected_values: ConfigValues = IndexMap::new();
        expected_values.insert("arcella.servers.alpha.test_string".to_string(), (TomlValue::String("string".to_string()), 0));
        expected_values.insert("arcella.servers.alpha.test_int".to_string(), (TomlValue::Integer(10), 0));
        expected_values.insert("arcella.servers.alpha.test_bool".to_string(), (TomlValue::Boolean(true), 0));

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
                &vec!["root".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec!["config.d/*.toml".to_string()];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("root.server.port".to_string(), (TomlValue::Integer(8080), 0));
            expected_values.insert("root.server.host".to_string(), (TomlValue::String("localhost".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
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
                &vec![],
                0,
            ).unwrap();

            let expected_includes = Vec::new();

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("database.host".to_string(), (TomlValue::String("db.example.com".to_string()), 0));
            expected_values.insert("database.port".to_string(), (TomlValue::Integer(5432), 0));
            expected_values.insert("database.pool.max_connections".to_string(), (TomlValue::Integer(10), 0));
            expected_values.insert("database.pool.timeout".to_string(), (TomlValue::Float(OrderedFloat(30.5)), 0));
            expected_values.insert("logging.level".to_string(), (TomlValue::String("info".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
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
                &vec!["config".to_string()],
                0
            ).unwrap();

            let expected_includes = vec!["overrides.toml".to_string()];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("config.app.name".to_string(), (TomlValue::String("my_app".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
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
                &vec!["app".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec![
                "config.d/*.toml".to_string(),
                "local.toml".to_string(),
                "secrets.toml".to_string(),
            ];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("app.app.version".to_string(), (TomlValue::String("1.0.0".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
       }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_empty_content() {
            let config_content = "";

            let config = parse_and_collect(
                config_content,
                &vec![],
                0,
            ).unwrap();

            let expected_includes = Vec::new();
            let expected_values = IndexMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_only_includes() {
            let config_content = r#"
            includes = ["a.toml", "b.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &vec!["top".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec!["a.toml".to_string(), "b.toml".to_string()];
            let expected_values = IndexMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[tokio::test]
        async fn test_parse_config_and_collect_includes_invalid_toml() {
            let config_content = r#"
            [app
            name = "broken"
            "#; // Invalid TOML syntax

            let result = parse_and_collect(
                config_content,
                &vec![],
                0,
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
                &vec![],
                0,
            ).unwrap();

            let expected_includes = Vec::new();

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("features.enabled".to_string(), (TomlValue::Boolean(true), 0));
            expected_values.insert("features.disabled".to_string(), (TomlValue::Boolean(false), 0));
            expected_values.insert("features.flags.list".to_string(), (TomlValue::Array(vec![
                TomlValue::String("flag1".to_string()),
                TomlValue::String("flag2".to_string()),
                TomlValue::String("flag3".to_string()),
            ]), 0));
            expected_values.insert("server.ports".to_string(), (TomlValue::Array(vec![
                TomlValue::Integer(80),
                TomlValue::Integer(443),
                TomlValue::Integer(8080),
            ]), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }        

    }    

}
