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
//! This module defines a universal value representation (`Value`) used across
//! different parts of the Arcella system (e.g., configuration, ALME protocol, manifests)
//! to handle structured data in a type-safe manner.
//!
//! The `Value` enum provides a flexible way to represent common data types
//! that can be serialized/deserialized using `serde`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
/// use std::collections::HashMap;
///
/// // Creating a simple value
/// let string_val = Value::String("hello".to_string());
///
/// // Creating a complex nested structure
/// let mut map = HashMap::new();
/// map.insert("key1".to_string(), Value::Integer(42));
/// map.insert("key2".to_string(), Value::Boolean(true));
/// let array_val = Value::Array(vec![Value::String("item1".to_string()), Value::Null]);
/// map.insert("key3".to_string(), array_val);
/// let complex_val = Value::Map(map);
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
    Float(f64),

    /// A boolean value.
    Boolean(bool),

    /// A map of string keys to `Value`s.
    Map(HashMap<String, Value>),

    /// An explicit null value, representing the absence of data.
    Null,

    /// An error state, used to signal failures during data processing or conversion.
    ///
    /// This variant is useful when a value cannot be correctly parsed or constructed,
    /// allowing the error to be propagated alongside other valid data.
    Error(String),
}