// arcella/arcella-fs-utils/src/config_loader.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! Recursive configuration loader for Arcella.
//!
//! This module provides functions for loading TOML-based configuration files,
//! including support for recursive inclusion of other files via the `includes` key.
//! It handles circular dependencies, limits recursion depth, and collects warnings
//! during the loading process for later reporting.

//!
//! ## Key Features
//!
//! - **File inclusion**: Files can include others via `includes = "file.toml"` or `includes = ["a.toml", "b.toml"]`.
//! - **Glob and directory support**: `includes` may contain glob patterns (e.g., `"config.d/*.toml"`) or directories.
//! - **Cycle detection**: Each file is loaded at most once across the entire configuration tree.
//! - **Depth limiting**: Prevents infinite recursion due to deep or cyclic includes (`MAX_CONFIG_DEPTH = 5`).
//! - **Warning collection**: Non-fatal issues (e.g., pruned subtrees, duplicate includes) are collected for later inspection.
//! - **Deterministic ordering**: Included files from globs are sorted lexicographically to ensure consistent behavior.

//!
//! ## Path Resolution
//!
//! All paths in `includes` are resolved **relative to `ConfigLoadParams::config_dir`**,  
//! *not* relative to the including file. This ensures predictable and reproducible behavior
//! regardless of the inclusion chain.
//!
//! ## Missing Files
//!
//! If a path in `includes` does not exist or is not a valid TOML file (e.g., a `.template.toml` file),
//! it is **silently skipped** and a `SkippedInvalidFile` warning is recorded.
//! This allows optional configuration files (e.g., `local.toml`) to be absent without causing an error.

use std::path::Path;

use crate::collect_toml_includes;
use crate::ConfigLoadWarning; 
use crate::error::{ArcellaUtilsError, Result as ArcellaUtilsResult};
use crate::toml;
use crate::types::*;

use arcella_types::config::Value as TomlValue;

/// The maximum allowed recursion depth when loading configuration files.
///
/// This prevents stack overflow or excessive resource consumption from deeply nested
/// or circular `includes`. The root file is at depth 0, so up to `MAX_CONFIG_DEPTH + 1`
/// files can be loaded in a single inclusion chain.
///
/// Example: with `MAX_CONFIG_DEPTH = 5`, the following is allowed:
/// `root.toml → a.toml → b.toml → c.toml → d.toml → e.toml` (6 files total).
/// Attempting to include a 7th file will trigger a `MaxDepthReached` warning and skip loading.
const MAX_CONFIG_DEPTH: usize = 5;

/// Recursively loads configuration files starting from `config_file_path`, including files specified in `includes`.
///
/// This function reads the initial configuration file, parses it, extracts `includes`,
/// and then recursively processes those included files up to `MAX_CONFIG_DEPTH`.
/// It collects both configuration data and non-critical warnings during the process.
///
/// Warnings are collected in the provided `state.warnings` vector. This allows reporting issues
/// (like null values, duplicate includes, etc.) that occur before the main logger is initialized.
///
/// Critical errors (like I/O failures, TOML syntax errors) will stop the loading process
/// and return an `Err`.
///
/// # Arguments
///
/// * `params` – Immutable loading parameters (prefix, config directory).
/// * `state` – Mutable state tracking visited files, loaded files, and warnings.
/// * `config_file_path` – The absolute or relative path to the configuration file to load.
/// * `included_from` – The file that included `config_file_path` (for cycle diagnostics).
/// * `current_depth` – Current inclusion depth (0 for the root file).
///
/// # Returns
///
/// A `Result` containing a `Vec<TomlFileData>` representing the loaded configurations,
/// or an `ArcellaUtilsError` if a critical error occurs.
///
/// Note: Each file is loaded at most once globally (not per inclusion path), to avoid redundant work
/// and ensure deterministic behavior.
pub async fn load_config_recursive(
    params: &ConfigLoadParams,
    state: &mut ConfigLoadState,
    config_file_path: &Path,
    included_from: Option<&Path>,
    current_depth: usize,
) -> ArcellaUtilsResult<Vec<TomlFileData>> {
    // Enforce maximum inclusion depth
    if current_depth > MAX_CONFIG_DEPTH {
        state.warnings.push(ConfigLoadWarning::MaxDepthReached {
            path: config_file_path.to_path_buf(),
        });
        return Ok(vec![]); // Reached maximum depth
    }

    // Prevent loading the same file more than once (global deduplication)
    if state.visited_paths.contains(config_file_path) {
        state.warnings.push(ConfigLoadWarning::DuplicateInclude {
            path: config_file_path.to_path_buf(),
            included_from: included_from.map(|p| p.to_path_buf())
                .unwrap_or_else(|| config_file_path.to_path_buf()),
        });
        return Ok(vec![]); // Not an error, just break the recursion
    }

    // Read file content first; only mark as visited after successful read
    // to avoid poisoning the state on transient I/O errors.    

    let content = tokio::fs::read_to_string(config_file_path)
        .await
        .map_err(|e| ArcellaUtilsError::IoWithPath {
            source: e,
            path: config_file_path.to_path_buf(),
        })?;

    // Now it's safe to mark the file as visited
    state.visited_paths.insert(config_file_path.to_path_buf());
    let (file_idx, _) = state.config_files.insert_full(config_file_path.to_path_buf());

    let all_configs = load_config_recursive_from_content(
        params,
        state,
        &content,
        file_idx,
        config_file_path,
        current_depth,
    ).await?;

    // visited_paths.remove(config_file_path); // Optional, if cycles are checked only within one traversal path

    Ok(all_configs)
}

/// Parses configuration content and recursively loads included files.
///
/// This function parses the provided TOML `content`, extracts `includes`,
/// resolves them relative to `params.config_dir`, and recursively loads each included file.
/// Globs and directories in `includes` are expanded and sorted lexicographically.
///
/// # Arguments
///
/// * `params` – Immutable loading parameters.
/// * `state` – Mutable loading state.
/// * `content` – Raw TOML content of the current file.
/// * `file_idx` – Unique index of this file (for value provenance).
/// * `config_file_path` – Path of the current file (used for diagnostics).
/// * `current_depth` – Current inclusion depth.
///
/// # Returns
///
/// A vector of `TomlFileData` for this file and all recursively included files.
pub async fn load_config_recursive_from_content(
    params: &ConfigLoadParams,
    state: &mut ConfigLoadState,
    content: &str,
    file_idx: usize,
    config_file_path: &Path,
    current_depth: usize,
) -> ArcellaUtilsResult<Vec<TomlFileData>> {

    let (config, result) = toml::parse_and_collect(&content, &params.prefix, file_idx)?;
    if result == TraversalResult::Pruned{
        state.warnings.push(ConfigLoadWarning::Pruned {
            path: config_file_path.to_path_buf(),
        });
    }

     // --- Check values for Null or other issues (example) ---
    // This could be extracted into a separate function for checking TomlFileData
    for (key, (value, _)) in &config.values {
        if matches!(value, TomlValue::Null) {
            state.warnings.push(ConfigLoadWarning::NullValueDetected {
                key: key.clone(),
                file: config_file_path.to_path_buf(),
            });
        }
    }

    // Resolve and expand includes (e.g., globs, directories) into concrete file paths.
    // The result is sorted lexicographically to ensure deterministic loading order.
    // Invalid or missing paths are skipped and recorded as warnings.
    let include_paths = collect_toml_includes(
        &config.includes, 
        &params.config_dir, 
        &mut state.warnings,
    ).await?;

    let mut all_configs = vec![config];

    // Recursively load each included file
    for include_path in include_paths {
        // Pin the future returned by the recursive call
        let sub_configs_future = Box::pin(load_config_recursive(
            params,
            state,
            &include_path,
            Some(config_file_path),
            current_depth + 1,
        ));
        // Await the pinned future
        let mut sub_configs = sub_configs_future.await?;
        all_configs.append(&mut sub_configs);
    };

    Ok(all_configs)
}

/// Loads configuration files recursively starting from a single file.
///
/// This is a convenience entry point that initiates recursive loading from a root file.
/// It does not initialize `state`; the caller must provide a fresh or reused `ConfigLoadState`.
///
/// # Returns
///
/// A `Result` containing a `Vec<TomlFileData>` with all loaded configuration data.
/// Warnings are accumulated in `state.warnings` and should be inspected by the caller.
pub async fn load_config_recursive_from_file(
    params: &ConfigLoadParams,
    state: &mut ConfigLoadState,
    config_file_path: &Path,
) -> ArcellaUtilsResult<Vec<TomlFileData>> {

    load_config_recursive(
        params,
        state,
        config_file_path, 
        None,
        0,
    ).await

}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;
    use indexmap::IndexSet;
    use std::collections::HashSet;

    use super::*;

    #[tokio::test]
    async fn test_load_config_recursive_simple() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };


        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await.unwrap();

        assert_eq!(configs.len(), 1); // Main config only
        assert!(state.warnings.is_empty()); // No warnings expected

        // Check if the main config has the expected value
        let main_config = &configs[0];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap().0, TomlValue::Integer(8080));
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_includes() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
            includes = ["db.toml"]
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let db_config_path = config_dir.join("db.toml");
        let db_config_content = r#"
            [database]
            host = "localhost"
            port = 5432
        "#;
        fs::write(&db_config_path, db_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await.unwrap();

        assert_eq!(configs.len(), 2); // Main config and included db.toml
        assert!(state.warnings.is_empty()); // No warnings expected

        // Check values from both configs
        let main_config = &configs[0];
        let db_config = &configs[1];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap().0, TomlValue::Integer(8080));
        assert_eq!(db_config.values.get("arcella.database.host").unwrap().0, TomlValue::String("localhost".to_string()));
        assert_eq!(db_config.values.get("arcella.database.port").unwrap().0, TomlValue::Integer(5432));
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_cycle() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
            includes = ["cycle.toml"]
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let cycle_config_path = config_dir.join("cycle.toml");
        let cycle_config_content = r#"
            [database]
            host = "localhost"
            includes = ["main.toml"] # Creates a cycle
        "#;
        fs::write(&cycle_config_path, cycle_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await.unwrap();

        // Should load main.toml and cycle.toml once, then detect the cycle and stop.
        // The exact behavior might vary depending on the order of processing in collect_toml_includes,
        // but we expect at least one warning about the duplicate/cycle.
        assert!(configs.len() >= 1); // At least main.toml is loaded
        assert!(!state.warnings.is_empty()); // At least one warning for the cycle
        assert!(state.warnings.iter().any(|w| matches!(w, ConfigLoadWarning::DuplicateInclude { .. })));
    }

    #[tokio::test]
    async fn test_load_config_recursive_depth_limit() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        // Create a chain of files that exceeds MAX_CONFIG_DEPTH
        for i in 0..=MAX_CONFIG_DEPTH + 2 { // Create more files than the limit
            let current_file = config_dir.join(format!("level_{}.toml", i));
            let mut content = format!("key_{} = {}", i, i);
            content.push_str(&format!("\nincludes = [\"level_{}.toml\"]", i + 1));
            fs::write(&current_file, content).unwrap();
        }

        let root_file = config_dir.join("level_0.toml");

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &root_file,
        ).await.unwrap();

        // Should stop after MAX_CONFIG_DEPTH
        // The exact number of loaded configs might vary slightly depending on implementation details,
        // but the key point is that it stops and generates a warning.
        assert!(configs.len() <= MAX_CONFIG_DEPTH + 1); // At most MAX_DEPTH + 1 configs (including root)
        assert!(state.warnings.iter().any(|w| matches!(w, ConfigLoadWarning::MaxDepthReached { .. })));
    }

    #[tokio::test]
    async fn test_load_config_recursive_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
            includes = ["nonexistent.toml"] # This file does not exist
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await;

        // Should return an error because nonexistent.toml is listed in includes
        assert!(configs.is_ok());
        assert!(state.warnings.iter().any(|w| matches!(w, ConfigLoadWarning::SkippedInvalidFile { .. })));
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_directory_in_includes() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
            includes = ["subdir/"] # Include a directory
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let subdir = config_dir.join("subdir");
        fs::create_dir(&subdir).unwrap();

        let sub_config_path = subdir.join("sub_config.toml");
        let sub_config_content = r#"
            [logging]
            level = "info"
        "#;
        fs::write(&sub_config_path, sub_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await.unwrap();

        assert_eq!(configs.len(), 2); // Main config and the file in subdir
        assert!(state.warnings.is_empty()); // No warnings expected

        let main_config = &configs[0];
        let sub_config = &configs[1];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap().0, TomlValue::Integer(8080));
        assert_eq!(sub_config.values.get("arcella.logging.level").unwrap().0, TomlValue::String("info".to_string()));
    }

    // Additional test for the convenience function's return tuple structure
    #[tokio::test]
    async fn test_load_config_recursive_from_file_return_type() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();
        let mut state  = ConfigLoadState {
            config_files: IndexSet::new(),
            visited_paths: HashSet::new(),
            warnings: Vec::new(),
        };

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let params = ConfigLoadParams {
            prefix: vec!["arcella".to_string()],
            config_dir: config_dir.to_path_buf(),
        };

        let configs = load_config_recursive_from_file(
            &params,
            &mut state,
            &main_config_path,
        ).await;

        assert!(configs.is_ok());

        let configs = configs.unwrap();
        assert_eq!(configs.len(), 1);
        assert!(state.warnings.is_empty());

        // Check the type of the return value
        let _: (Vec<TomlFileData>, Vec<ConfigLoadWarning>) = (configs, state.warnings);
    }
}
