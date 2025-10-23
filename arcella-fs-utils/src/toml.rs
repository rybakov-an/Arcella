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
use toml_edit::{Item as TomlEditItem, Value as TomlEditValue};

use arcella_types::{
    value::Value as TomlValue
};

use crate::{ArcellaUtilsError, ArcellaResult};

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
    max_depth: usize,
) -> ArcellaResult<()> {
    if depth > max_depth {
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
                        max_depth,
                    )?;                    
                }
            }
        },
        _ => {}
    }        

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::DocumentMut;

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

        let mut values: HashMap<String, TomlValue> = HashMap::new();
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
        expected_values.insert("arcella.servers.alpha.test_string".to_string(), TomlValue::String("string".to_string()));
        expected_values.insert("arcella.servers.alpha.test_int".to_string(), TomlValue::Integer(10));
        expected_values.insert("arcella.servers.alpha.test_bool".to_string(), TomlValue::Boolean(true));

        assert_eq!(values, expected_values);        

    }

}
