// arcella/arcella-fs-utils/src/warnings.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::path::PathBuf;

/// Represents non-critical warnings that can occur during configuration loading.
///
/// These warnings are collected during the configuration loading process
/// when the main logger might not yet be initialized. They are stored
/// in a buffer and can be processed (e.g., logged) later.
#[derive(Debug, Clone)]
pub enum ConfigLoadWarning {
    /// General-purpose warning for unexpected conditions.
    Internal(String),

    /// A configuration value was found to be `Null`.
    NullValueDetected { key: String, file: PathBuf },

    /// An error occurred while processing a configuration value (e.g., unsupported type).
    ValueError { key: String, error: String, file: PathBuf },

    /// A configuration file was included more than once (cycle or duplicate).
    DuplicateInclude { path: PathBuf, included_from: PathBuf },

    /// A configuration file was retried for processing (e.g., due to internal logic or depth limits).
    RetriedProcessing { path: PathBuf },

    /// A file specified in `includes` was skipped because it did not pass validation
    /// (e.g., not a `.toml` file or is a `.template.toml` file).
    SkippedInvalidFile { path: PathBuf },

    /// An unknown TOML type was encountered that could not be converted.
    UnknownTomlType { key: String, type_name: String, file: PathBuf },

    /// Represents a warning that occurs when the maximum depth of includes is reached.
    MaxDepthReached { path: PathBuf },

    /// A TOML document subtree was skipped because it exceeded the maximum allowed nesting depth
    /// (`MAX_TOML_DEPTH`). This is not an error, but some configuration keys may be missing.
    Pruned { path: PathBuf },
}

impl std::fmt::Display for ConfigLoadWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadWarning::Internal(msg) => {
                write!(f, "{}", msg)
            }
            ConfigLoadWarning::NullValueDetected { key, file } => {
                write!(f, "Null value found for key '{}' in file {:?}", key, file)
            }
            ConfigLoadWarning::ValueError { key, error, file } => {
                write!(f, "Error processing value for key '{}' in file {:?}: {}", key, file, error)
            }
            ConfigLoadWarning::DuplicateInclude { path, included_from } => {
                write!(f, "Duplicate include path '{:?}' found, already included from {:?}", path, included_from)
            }
            ConfigLoadWarning::RetriedProcessing { path } => {
                write!(f, "Retried processing of config file {:?}", path)
            }
            ConfigLoadWarning::SkippedInvalidFile { path } => {
                write!(f, "Skipped invalid file in includes: {:?}", path)
            }
            ConfigLoadWarning::UnknownTomlType { key, type_name, file } => {
                write!(f, "Unknown TOML type '{}' for key '{}' in file {:?}", type_name, key, file)
            }
            ConfigLoadWarning::MaxDepthReached { path } => {
                write!(f, "Maximum include depth reached for file {:?}", path)
            }
            ConfigLoadWarning::Pruned { path } => {
                write!(f, "Pruned file {:?}", path)
            }
        }
    }
}
