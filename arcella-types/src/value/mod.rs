// arcella/arcella-types/src/value/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! Generic value types for Arcella.
//!
//! This module defines a universal value representation ([`Value`]) used across
//! different parts of the Arcella system (e.g., configuration, ALME protocol, manifests)
//! to handle structured data in a type-safe manner.
//!
//! The [`Value`] enum provides a flexible way to represent common data types
//! that can be serialized/deserialized using `serde`.
//!
//! It also includes [`ConfigData`], a utility for managing hierarchical configurations
//! where keys like `arcella.log.level` can be grouped into logical sections.

use indexmap::IndexMap;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a specific error that occurred during data processing.
///
/// This struct is used inside the [`Value::TypedError`] variant to carry
/// structured error information instead of just a string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedError {
    /// Human-readable error message.
    pub message: String,
    /// A string identifying the type or category of the error.
    pub error_type: String,
}

/// A generic value type that can represent data from configuration, ALME protocol,
/// WIT interfaces, or other structured sources within the Arcella ecosystem.
///
/// This enum serves as a common interchange format for dynamic data, similar to
/// `serde_json::Value` but tailored for Arcella's specific needs.
///
/// It supports:
/// - Primitive types: `String`, `Integer`, `Float`, `Boolean`, `Null`
/// - Compound types: `Array` (list of `Value`), `Map` (key-value pairs of `String` to `Value`)
/// - Error signaling: `Error` (for representing failures during data processing)
///
/// # Examples
///
/// ```
/// use arcella_types::value::Value;
/// use ordered_float::OrderedFloat;
///
/// // Creating a simple value
/// let string_val = Value::String("hello".to_string());
/// let int_val = Value::Integer(42);
/// let float_val = Value::Float(OrderedFloat(3.14));
/// let bool_val = Value::Boolean(true);
/// let null_val = Value::Null;
///
/// // Creating an array of values
/// let array_val = Value::Array(vec![
///     Value::String("item1".to_string()),
///     Value::Integer(2),
///     Value::Null,
/// ]);
///
/// // Creating a map of values
/// use std::collections::HashMap;
/// let mut map = HashMap::new();
/// map.insert("key1".to_string(), Value::Integer(42));
/// map.insert("key2".to_string(), Value::Boolean(true));
/// let map_val = Value::Map(map);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// A sequence of `Value`s.
    Array(Vec<Value>),

    /// A UTF-8 string.
    String(String),

    /// A signed 64-bit integer.
    Integer(i64),

    /// A 64-bit floating-point number.
    /// Uses `OrderedFloat` to ensure total ordering for use in collections.
    Float(OrderedFloat<f64>),

    /// A boolean value.
    Boolean(bool),

    /// A map of string keys to `Value`s.
    /// Uses `HashMap` for fast lookups.
    Map(HashMap<String, Value>),

    /// An explicit null value, representing the absence of data.
    Null,

    /// A typed error value, useful for signaling errors within data structures.
    TypedError(TypedError),
}

/// Represents an entry within a section of the configuration.
/// It can be either a reference to a value key or a name of a subsection.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionEntry {
    /// A reference to a value key by its index in the `values` map.
    ValueKey(usize),

    /// The name of a subsection.
    SubSection(String),
}

/// Represents hierarchical configuration data with support for logical sections.
///
/// Keys in the configuration are expected to be in a dotted format (e.g., `arcella.log.level`).
/// This struct allows grouping related keys into sections for easier access.
///
/// The underlying storage uses `IndexMap` to preserve the order of insertion for keys.
#[derive(Debug, Clone)]
pub struct ConfigData {
    /// Original flat map of all parameters, sorted by key.
    pub values: IndexMap<String, Value>,

    /// Map of sections (e.g., "arcella", "arcella.log", "arcella.modules").
    /// The value is a vector of `SectionEntry` items, representing
    /// the next level keys that belong to this section.
    ///
    /// For example, if the key is "arcella.log.level", then only "arcella.log" sections will contain its index
    pub sections: IndexMap<String, Vec<SectionEntry>>,
}

impl ConfigData {
    /// Creates a new `ConfigData` instance from a flat map of key-value pairs.
    /// It organizes the keys into hierarchical sections based on dot-separated prefixes.
    ///
    /// # Arguments
    ///
    /// * `values` - An `IndexMap` containing the configuration keys and their values.
    ///
    /// # Returns
    ///
    /// A new `ConfigData` instance with organized sections.
    pub fn new(values: IndexMap<String, Value>) -> Self {
        let mut sorted_values = values;
        sorted_values.sort_keys();

        let mut sections: IndexMap<String, Vec<SectionEntry>> = IndexMap::new();

        for (i, key) in sorted_values.keys().enumerate() {
            let parts: Vec<&str> = key.split('.').collect();

            // Update all intermediate sections
            let mut current_path = String::new();
            for (j, &part) in parts.iter().enumerate() {
                let old_path = current_path.clone();
                if !current_path.is_empty() {
                    current_path.push('.');
                }
                current_path.push_str(part);

                let parent_section = sections
                    .entry(old_path.clone())
                        .or_default();

                if j == parts.len() - 1 {
                    parent_section.push(SectionEntry::ValueKey(i));
                } else {
                    let current_entry = SectionEntry::SubSection(current_path.clone());
                    if !parent_section.contains(&current_entry) {
                        parent_section.push(current_entry);
                    }
                    let _ = sections
                        .entry(current_path.clone())
                            .or_default();
                }

            }

        }

        sections.sort_keys();

        ConfigData {
            values: sorted_values,
            sections,
        }
    }

    /// Retrieves a reference to the value associated with the given key.
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to look up.
    ///
    /// # Returns
    ///
    /// `Some(&Value)` if the key exists, otherwise `None`.
    ///
    /// # Example
    ///
    /// ```
    /// use arcella_types::value::{ConfigData, Value};
    /// use indexmap::IndexMap;
    ///
    /// let mut input = IndexMap::new();
    /// input.insert("key1".to_string(), Value::Integer(42));
    /// let config = ConfigData::new(input);
    ///
    /// assert_eq!(config.get("key1"), Some(&Value::Integer(42)));
    /// assert_eq!(config.get("nonexistent"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Retrieves the indices of value keys belonging to the specified section.
    ///
    /// # Arguments
    ///
    /// * `section` - The name of the configuration section (e.g., "arcella.log").
    ///
    /// # Returns
    ///
    /// `Some(Vec<usize>)` containing the indices if the section exists, otherwise `None`.
    pub fn get_section_keys(&self, section: &str) -> Option<Vec<usize>> {
        self.sections.get(section).map(|entries| {
            entries.iter()
                .filter_map(|entry| match entry {
                    SectionEntry::ValueKey(i) => Some(*i),
                    SectionEntry::SubSection(_) => None,
                })
                .collect()
        })
    }

    /// Retrieves the names of sub-sections belonging to the specified section.
    ///
    /// # Arguments
    ///
    /// * `section` - The name of the configuration section (e.g., "arcella.log").
    ///
    /// # Returns
    ///
    /// `Some(Vec<String>)` containing the names of sub-sections if the section exists, otherwise `None`.
    pub fn get_subsection_names(&self, section: &str) -> Option<Vec<String>> {
        self.sections.get(section).map(|entries| {
            entries.iter()
                .filter_map(|entry| match entry {
                    SectionEntry::ValueKey(_) => None,
                    SectionEntry::SubSection(name) => Some(name.clone()),
                })
                .collect()
        })
    }    

    /// Retrieves the key-value pairs belonging to the specified section.
    ///
    /// This method returns an `IndexMap` where keys are the full configuration keys
    /// (e.g., "arcella.log.level") and values are references to the corresponding `Value`s.
    /// The order of the returned map reflects the sorted order of the original keys.
    ///
    /// # Arguments
    ///
    /// * `section` - The name of the configuration section (e.g., "arcella.log").
    ///
    /// # Returns
    ///
    /// `Some(IndexMap<String, &Value>)` if the section exists, otherwise `None`.
    ///
    /// # Example
    ///
    /// ```
    /// use arcella_types::value::{ConfigData, Value};
    /// use indexmap::IndexMap;
    ///
    /// let mut input = IndexMap::new();
    /// input.insert("arcella.log.level".to_string(), Value::String("info".to_string()));
    /// input.insert("arcella.log.file".to_string(), Value::String("log.txt".to_string()));
    /// input.insert("arcella.modules.path".to_string(), Value::String("/mods".to_string()));
    /// let config = ConfigData::new(input);
    ///
    /// let log_section = config.get_section_data("arcella.log").unwrap();
    /// assert_eq!(log_section.len(), 2);
    /// assert_eq!(log_section.get("arcella.log.level"), Some(&&Value::String("info".to_string())));
    /// assert_eq!(log_section.get("arcella.log.file"), Some(&&Value::String("log.txt".to_string())));
    /// ```
    pub fn get_section_data(&self, section: &str) -> Option<IndexMap<String, &Value>> {
        let indices = self.get_section_keys(section)?;
        let mut section_data = IndexMap::new();
        for idx in indices {
            if let Some((key, value)) = self.values.get_index(idx) {
                section_data.insert(key.clone(), value);
            }
        }
        Some(section_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_data_new() {
        let mut input = IndexMap::new();
        input.insert("arcella.modules.path".to_string(), Value::String("/mods".to_string()));
        input.insert("arcella.log.level".to_string(), Value::String("info".to_string()));
        input.insert("arcella.log.file".to_string(), Value::String("log.txt".to_string()));
        input.insert("server.port".to_string(), Value::Integer(8080));
        input.insert("server.host".to_string(), Value::String("localhost".to_string()));

        let config = ConfigData::new(input);

        assert_eq!(config.values.len(), 5);
        assert!(config.values.get_index(0).unwrap().0 == "arcella.log.file");
        assert!(config.values.get_index(1).unwrap().0 == "arcella.log.level");
        assert!(config.values.get_index(2).unwrap().0 == "arcella.modules.path");
        assert!(config.values.get_index(3).unwrap().0 == "server.host");
        assert!(config.values.get_index(4).unwrap().0 == "server.port");

        assert!(config.sections.contains_key(""));
        assert!(config.sections.contains_key("arcella"));
        assert!(config.sections.contains_key("arcella.log"));
        assert!(config.sections.contains_key("arcella.modules"));
        assert!(config.sections.contains_key("server"));
    }

    #[test]
    fn test_config_data_get() {
        let mut input = IndexMap::new();
        input.insert("key1".to_string(), Value::Integer(42));
        let config = ConfigData::new(input);

        assert_eq!(config.get("key1"), Some(&Value::Integer(42)));
        assert_eq!(config.get("nonexistent"), None);
    }

    #[test]
    fn test_config_data_get_section_data() {
        let mut input = IndexMap::new();
        input.insert("arcella.modules.path".to_string(), Value::String("/mods".to_string()));
        input.insert("arcella.log.level".to_string(), Value::String("info".to_string()));
        input.insert("arcella.log.file".to_string(), Value::String("log.txt".to_string()));
        input.insert("server.port".to_string(), Value::Integer(8080));
        input.insert("server.host".to_string(), Value::String("localhost".to_string()));

        let config = ConfigData::new(input);

        let log_section = config.get_section_data("arcella.log").unwrap();
        assert_eq!(log_section.len(), 2);
        assert_eq!(log_section.get("arcella.log.level"), Some(&&Value::String("info".to_string())));
        assert_eq!(log_section.get("arcella.log.file"), Some(&&Value::String("log.txt".to_string())));

        let arcella_section = config.get_section_data("arcella").unwrap();
        assert_eq!(arcella_section.len(), 0); // Includes log.file, log.level, modules.path
    }

    #[test]
    fn test_config_data_get_subsection_names() {
        let mut input = IndexMap::new();
        input.insert("arcella.modules.path".to_string(), Value::String("/mods".to_string()));
        input.insert("arcella.log.level".to_string(), Value::String("info".to_string()));
        input.insert("arcella.log.file".to_string(), Value::String("log.txt".to_string()));
        input.insert("server.port".to_string(), Value::Integer(8080));
        input.insert("server.host".to_string(), Value::String("localhost".to_string()));

        let config = ConfigData::new(input);

        // Check subsections for "arcella"
        let arcella_subsections = config.get_subsection_names("arcella").unwrap();
        assert_eq!(arcella_subsections.len(), 2);
        assert!(arcella_subsections.contains(&"arcella.log".to_string()));
        assert!(arcella_subsections.contains(&"arcella.modules".to_string()));
        // Check that the order corresponds to the insertion order
        assert_eq!(arcella_subsections[0], "arcella.log");
        assert_eq!(arcella_subsections[1], "arcella.modules");

        // Check subsections for "server" (there are none)
        let server_subsections = config.get_subsection_names("server").unwrap();
        assert_eq!(server_subsections.len(), 0);

        // Check subsections for "nonexistent" (section does not exist)
        let nonexistent_subsections = config.get_subsection_names("nonexistent");
        assert_eq!(nonexistent_subsections, None);

        // Check subsections for "arcella.log" (also no sub-sections)
        let log_subsections = config.get_subsection_names("arcella.log").unwrap();
        assert_eq!(log_subsections.len(), 0);
    }    
}
