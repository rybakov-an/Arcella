// arcella/arcella-fs-utils/src/types.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use indexmap::IndexSet;
use std::collections::HashSet;
use std::path::PathBuf;

use arcella_types::config::ConfigValues;
use crate::ConfigLoadWarning; 

/// Maximum allowed recursion depth when traversing nested TOML tables.
///
/// Prevents stack overflow due to deeply nested or maliciously crafted TOML.
/// This limit applies only to table nesting, not array depth or file inclusion depth
/// (which is controlled by `MAX_CONFIG_DEPTH` in `config_loader.rs`).
pub const MAX_TOML_DEPTH: usize = 10;

/// Template file suffix
pub const TEMPLATE_TOML_SUFFIX: &str = ".template.toml";

// Immutable parameters — can be freely cloned
#[derive(Debug, Clone)]
pub struct ConfigLoadParams {
    pub prefix: Vec<String>,
    pub config_dir: PathBuf,
}

// Mutable state — passed by &mut reference
pub struct ConfigLoadState {
    /// All configuration files that have been successfully loaded, in order of inclusion.
    pub config_files: IndexSet<PathBuf>,

    /// Tracks files currently in the inclusion stack to detect cyclic includes.
    pub visited_paths: HashSet<PathBuf>,

    /// Non-fatal warnings collected during loading.
    pub warnings: Vec<ConfigLoadWarning>,
}

impl Default for ConfigLoadState {
    fn default() -> Self {
        Self {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TomlFileData {
    pub includes: Vec<String>,
    pub values: ConfigValues,
}

/// Indicates the outcome of a recursive traversal of a TOML document.
///
/// - `Full`: The entire subtree was processed without hitting depth limits.
/// - `Pruned`: Traversal was stopped early because `MAX_TOML_DEPTH` was exceeded.
///   This is a non-fatal condition; a warning is issued, but loading continues.
#[derive(Debug, Clone, PartialEq)]
pub enum TraversalResult { Full, Pruned }
