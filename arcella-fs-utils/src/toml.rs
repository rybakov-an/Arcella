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
//! - Safe conversion from `toml_edit::Value` and `toml_edit::Table` to Arcella’s internal `Value` type.
//! - Recursive traversal of TOML documents to extract:
//!   - Configuration key-value pairs (stored with dot-separated paths).
//!   - File inclusion directives under keys named `includes`.
//!
//! The traversal respects a maximum depth limit (`MAX_TOML_DEPTH`) to prevent stack overflow.
//! Unsupported TOML types (e.g., datetimes) result in an error.
//!
//! # Entry Points
//!
//! - [`parse_and_collect`] — high-level function for parsing and extracting data.
//! - [`parse`] + [`collect_paths`] — for more granular control.
//!
//! # Special Semantics
//!
//! - **`includes` key**: If a table contains a key named `"includes"`, its value is interpreted as
//!   a list of configuration files to include. Both a single string and an array of strings are accepted.
//! - **`[[array-of-tables]]`**: These are converted into a `Value::Array` of `Value::Map`. Keys inside
//!   each table are stored relative to that table (i.e., they do *not* inherit the outer path prefix).
//!   For example:
//!   ```toml
//!   [[servers]]
//!   name = "a"
//!   ```
//!   becomes:
//!   ```text
//!   key: "servers", value: Array([Map{"name": "a"}])
//!   ```

use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item as TomlEditItem, Table, Value as TomlEditValue};

use arcella_types::config::{ConfigValues, Value as TomlValue};

use crate::{ArcellaUtilsError, ArcellaResult};
use crate::types::*;

/// Key name used to identify file inclusion directives in TOML.
const INCLUDES_KEY: &str = "includes";

/// Extension trait to convert `toml_edit::Value` into Arcella’s canonical `Value`.
///
/// Only TOML scalar types and arrays of scalars are supported.
/// The following TOML types are **not supported** and will cause an error:
/// - Datetime
/// - Inline tables (handled separately via `Table`)
///
/// Arrays are supported recursively, but must contain only supported scalar types.
pub trait ValueExt {
    /// Converts a `toml_edit::Value` into Arcella’s `Value`.
    ///
    /// # Errors
    ///
    /// Returns `ArcellaUtilsError::TOML` if the value contains an unsupported type
    /// (e.g., datetime or nested unsupported structure).
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

/// Converts an inline table into a regular `Table`.
///
/// Note: This conversion discards formatting and comments, which is acceptable
/// because Arcella uses `toml_edit` only for parsing, not for round-trip editing.
fn inline_table_to_table(inline: &InlineTable) -> Table {
    let mut table = Table::new();
    for (key, item) in inline.iter() {
        table.insert(key, item.into());
    }
    table
}

/// Converts an `ArrayOfTables` into a `TomlValue::Array` of `TomlValue::Map`,
/// respecting depth limits and collecting includes.
///
/// Each table in the array is processed independently with an empty path prefix,
/// meaning keys inside the table are stored relative to the table itself.
/// This matches TOML's semantic model for `[[array-of-tables]]`.
fn convert_array_of_tables_to_value(
    arr: &ArrayOfTables,
    depth: usize,
    file_idx: usize,
    includes: &mut Vec<String>,
) -> ArcellaResult<(TomlValue, TraversalResult)> {
    if depth > MAX_TOML_DEPTH {
        return Ok((TomlValue::Array(Vec::new()), TraversalResult::Pruned));
    }

    let mut result_vec = Vec::with_capacity(arr.len());
    let mut overall_result = TraversalResult::Full;

    for table in arr {
        let mut temp_values = IndexMap::new();
        let mut temp_includes = Vec::new();
        let child_result = table_to_value_map_recursive(
            table,
            &[],
            file_idx,
            &mut temp_includes,
            &mut temp_values,
            depth + 1,
        )?;

        includes.extend(temp_includes);

        // Convert collected values into a HashMap (relative to this table)
        let map: HashMap<String, TomlValue> = temp_values
            .into_iter()
            .map(|(k, (v, _))| (k, v))
            .collect();

        result_vec.push(TomlValue::Map(map));

        if child_result == TraversalResult::Pruned {
            overall_result = TraversalResult::Pruned;
        }
    }

    Ok((TomlValue::Array(result_vec), overall_result))
}


/// Recursively processes a TOML table, collecting configuration values and `includes` directives.
///
/// Keys are built using `current_path`. The special key `"includes"` is handled separately.
/// If its value is a string or array of strings, those paths are added to `includes`.
/// Other types under `"includes"` are ignored (no error is raised, but traversal continues).
///
/// Depth is checked against `MAX_TOML_DEPTH`; exceeding it results in pruning.
fn table_to_value_map_recursive(
    table: &Table,
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

    for (key, item) in table {
        let mut key_path = current_path.to_vec();
        key_path.push(key.to_string());

        if key == INCLUDES_KEY {
            // We accept both string and array forms of 'includes' for user convenience.
            match item {
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
                // Non-string/array values under 'includes' are silently ignored.
                // In the future, this could emit a ConfigLoadWarning.
                _ => {
                    // Do nothing — not an error, but also not actionable.
                }
            }

            continue;

        }

        let child_result = collect_paths_recursive(
            item,
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

    Ok(result)
}

/// Recursively traverses a TOML item to collect configuration values and `includes` directives.
///
/// This function walks the TOML structure starting from `item`, building dot-separated
/// configuration keys from the current path. It handles two special cases:
///
/// - Keys named [`INCLUDES_KEY`] are treated as file inclusion directives. Their values
///   may be either a string or an array of strings; all valid string values are added
///   to the `includes` output vector.
/// - All other scalar values are converted and stored in `values` with their full path.
///
/// Table nesting deeper than [`MAX_TOML_DEPTH`] is pruned (not traversed further),
/// and the function returns [`TraversalResult::Pruned`].
///
/// **Note**: `[[array-of-tables]]` are **not traversed as part of the key hierarchy**.
/// Instead, they are converted into `Value::Array(Value::Map(...))` and stored under their key.
/// For example:
/// ```toml
/// [[servers]]
/// name = "a"
/// [[servers]]
/// name = "b"
/// ```
/// becomes:
/// ```text
/// key: "servers", value: Array([Map{"name": "a"}, Map{"name": "b"}])
/// ```

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

    match item {
        TomlEditItem::Value(TomlEditValue::InlineTable(inline)) => {
            let table = inline_table_to_table(inline);
            table_to_value_map_recursive(
                &table,
                current_path,
                file_idx, 
                includes,
                values,
                depth,
            )
        }
        TomlEditItem::Table(table) => {
            table_to_value_map_recursive(
                table,
                current_path,
                file_idx, 
                includes,
                values,
                depth,
            )
        }
        TomlEditItem::ArrayOfTables(arr) => {
            let (array_val, child_result) = convert_array_of_tables_to_value(
                arr,
                depth,
                file_idx,
                includes,
            )?;
            values.insert(current_path.join("."), (array_val, file_idx));
            Ok(child_result)
        }
        TomlEditItem::Value(subvalue) => {
            let converted = TomlValue::from_toml_value(subvalue)?;
            values.insert(current_path.join("."), (converted, file_idx));
            Ok(TraversalResult::Full)
        }
        TomlEditItem::None => {
            // TOML has no null literal, but `toml_edit` may produce None programmatically.
            values.insert(current_path.join("."), (TomlValue::Null, file_idx));
            Ok(TraversalResult::Full)
        }
    }
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
/// * `file_idx` – Unique index of the file in the loading sequence (used for provenance).
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

    mod parse_config_and_collect_includes_tests {
       use super::*;

            #[test]
        fn test_max_toml_depth_pruned() {
            const MAX_DEPTH: usize = crate::types::MAX_TOML_DEPTH; // 10

            let mut path = "l0".to_string();
            for i in 1..=MAX_DEPTH + 1 {
                path.push_str(&format!(".l{}", i));
            }
            let content = format!("[{}]\nvalue = \"deep\"", path);

            let (data, result) = parse_and_collect(&content, &[], 0).unwrap();

            assert_eq!(result, TraversalResult::Pruned);

            assert!(!data.values.contains_key(&format!("{}.value", path)));
        }

        #[test]
        fn test_parse_config_and_collect_includes_simple() {
            let config_content = r#"
            [server]
            port = 8080
            host = "localhost"

            includes = ["config.d/*.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &["root".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec!["config.d/*.toml".to_string()];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("root.server.port".to_string(), (TomlValue::Integer(8080), 0));
            expected_values.insert("root.server.host".to_string(), (TomlValue::String("localhost".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_parse_config_and_collect_includes_nested() {
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
                &[],
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
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_parse_config_and_collect_includes_with_single_string_includes() {
            let config_content = r#"
            [app]
            name = "my_app"

            includes = "overrides.toml"
            "#;

            let config = parse_and_collect(
                config_content,
                &["config".to_string()],
                0
            ).unwrap();

            let expected_includes = vec!["overrides.toml".to_string()];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("config.app.name".to_string(), (TomlValue::String("my_app".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_parse_config_and_collect_includes_with_array_includes() {
            let config_content = r#"
            [app]
            version = "1.0.0"

            includes = ["config.d/*.toml", "local.toml", "secrets.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &vec!["config".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec![
                "config.d/*.toml".to_string(),
                "local.toml".to_string(),
                "secrets.toml".to_string(),
            ];

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert("config.app.version".to_string(), (TomlValue::String("1.0.0".to_string()), 0));

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
       }

        #[test]
        fn test_parse_config_and_collect_includes_empty_content() {
            let config_content = "";

            let config = parse_and_collect(
                config_content,
                &[],
                0,
            ).unwrap();

            let expected_includes = Vec::new();
            let expected_values = IndexMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_parse_config_and_collect_includes_only_includes() {
            let config_content = r#"
            includes = ["a.toml", "b.toml"]
            "#;

            let config = parse_and_collect(
                config_content,
                &["top".to_string()],
                0,
            ).unwrap();

            let expected_includes = vec!["a.toml".to_string(), "b.toml".to_string()];
            let expected_values = IndexMap::new();

            let expected_config = TomlFileData{
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_parse_config_and_collect_includes_invalid_toml() {
            let config_content = r#"
            [app
            name = "broken"
            "#; // Invalid TOML syntax

            let result = parse_and_collect(
                config_content,
                &[],
                0,
            );

            assert!(result.is_err());
            match result.unwrap_err() {
                ArcellaUtilsError::TOML(_) => {} // OK
                _ => panic!("Expected ArcellaUtilsError::TOML"),
            }
        }

        #[test]
        fn test_parse_config_and_collect_includes_with_boolean_and_array_values() {
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
                &[],
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
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }        

        #[test]
        fn test_array_of_tables_support() {
            let config_content = r#"
            [[servers]]
            name = "alpha"
            port = 8080

            [[servers]]
            name = "beta"
            port = 8081
            "#;

            let config = parse_and_collect(config_content, &[], 0).unwrap();

            let expected_includes = Vec::new();

            let mut expected_values: ConfigValues = IndexMap::new();
            expected_values.insert(
                "servers".to_string(),
                (
                    TomlValue::Array(vec![
                        TomlValue::Map({
                            let mut m = HashMap::new();
                            m.insert("name".to_string(), TomlValue::String("alpha".to_string()));
                            m.insert("port".to_string(), TomlValue::Integer(8080));
                            m
                        }),
                        TomlValue::Map({
                            let mut m = HashMap::new();
                            m.insert("name".to_string(), TomlValue::String("beta".to_string()));
                            m.insert("port".to_string(), TomlValue::Integer(8081));
                            m
                        }),
                    ]),
                    0,
                ),
            );

            let expected_config = TomlFileData {
                includes: expected_includes,
                values: expected_values,
            };

            assert_eq!(config, (expected_config, TraversalResult::Full));
        }

        #[test]
        fn test_nested_array_of_tables() {
            let config_content = r#"
            [[clusters]]
            name = "prod"
            [[clusters.nodes]]
            host = "node1"
            [[clusters.nodes]]
            host = "node2"

            [[clusters]]
            name = "dev"
            [[clusters.nodes]]
            host = "dev1"
            "#;

            let (config, traversal_result) = parse_and_collect(config_content, &[], 0).unwrap();

            // Should produce:
            // clusters = [
            //   { name: "prod", nodes: [ {host: "node1"}, {host: "node2"} ] },
            //   { name: "dev", nodes: [ {host: "dev1"} ] }
            // ]

            assert!(config.values.contains_key("clusters"));
            let val = &config.values["clusters"].0;
            match val {
                TomlValue::Array(arr) => {
                    assert_eq!(arr.len(), 2);
                    // Check first cluster
                    if let TomlValue::Map(map) = &arr[0] {
                        assert_eq!(map.get("name"), Some(&TomlValue::String("prod".to_string())));
                        if let Some(TomlValue::Array(nodes)) = map.get("nodes") {
                            assert_eq!(nodes.len(), 2);
                        } else {
                            panic!("Expected nodes array");
                        }
                    } else {
                        panic!("Expected map");
                    }
                }
                _ => panic!("Expected array"),
            }
        }

        #[test]
        fn test_max_toml_depth_pruned_inline_table() {
            const MAX_DEPTH: usize = crate::types::MAX_TOML_DEPTH; // 10
            const START_IDX: usize = 40;

            // Создаём inline-таблицу на глубине MAX_DEPTH + 1
            // Create inline-table with depth up to MAX_DEPTH + 1
            let mut inner = format!("inner = {{ x = {} }}", START_IDX);
            for i in 1..MAX_DEPTH + 1 {
                inner = format!("inner = {{ x = {}, {} }}", START_IDX + i, inner);
            }
            let content = format!("[top]\n{}", inner);

            let (config, traversal_result) = parse_and_collect(&content, &[], 12).unwrap();

            // Result must checked as Pruned
            assert_eq!(traversal_result, TraversalResult::Pruned);

            assert_eq!(config.values.len(), 8);

            for (num,  (_, (value, idx))) in (&config.values).iter().enumerate() {
                match value {
                    TomlValue::Integer(val) => {
                        assert_eq!(*val, (START_IDX + MAX_DEPTH - num) as i64);
                    }
                    _ => {
                        panic!("Error values!")
                    }
                }
                assert_eq!(*idx, 12);
            }

        }

    }

}
