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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::collect_toml_includes;
use crate::toml::TomlFileData;

use crate::ConfigLoadWarning; 
use crate::error::{ArcellaUtilsError, Result as ArcellaUtilsResult};
use crate::toml;

/// The maximum allowed recursion depth when loading configuration files.
/// This prevents potential stack overflow errors from circular `includes` or deeply nested structures.
const MAX_CONFIG_DEPTH: usize = 5;

/// Recursively loads configuration files starting from `config_file_path`, including files specified in `includes`.
///
/// This function reads the initial configuration file, parses it, extracts `includes`,
/// and then recursively processes those included files up to `MAX_CONFIG_DEPTH`.
/// It collects both configuration data and non-critical warnings during the process.
///
/// Warnings are collected in the provided `warnings` vector. This allows reporting issues
/// (like null values, duplicate includes, etc.) that occur before the main logger is initialized.
///
/// Critical errors (like I/O failures, TOML syntax errors) will stop the loading process
/// and return an `Err`.
///
/// # Arguments
///
/// * `prefix` - The prefix to prepend to all keys in the configuration data.
/// * `config_file_path` - The path to the initial configuration file (e.g., `arcella.toml`).
/// * `included_from` - The path of the file that included the current file, used for warning context.
/// * `config_dir` - The base directory used to resolve relative paths in `includes`.
/// * `current_depth` - The current recursion depth (for internal use).
/// * `visited_paths` - A set of paths already visited to prevent circular includes.
/// * `warnings` - A mutable reference to a vector where `ConfigLoadWarning`s are collected.
///
/// # Returns
///
/// A `Result` containing a `Vec<TomlFileData>` representing the loaded configurations,
/// or an `ArcellaUtilsError` if a critical error occurs.
pub async fn load_config_recursive(
    prefix: &[String],
    config_file_path: &Path,
    included_from: Option<&Path>,
    config_dir: &Path,
    current_depth: usize,
    visited_paths: &mut HashSet<PathBuf>,
    warnings: &mut Vec<ConfigLoadWarning>,
) -> ArcellaUtilsResult<Vec<TomlFileData>> {
    // Check recursion depth
    if current_depth > MAX_CONFIG_DEPTH {
        warnings.push(ConfigLoadWarning::MaxDepthReached {
            path: config_file_path.to_path_buf(),
        });
        return Ok(vec![]); // Reached maximum depth
    }

    // Check for circular dependencies
    if visited_paths.contains(config_file_path) {
        warnings.push(ConfigLoadWarning::DuplicateInclude {
            path: config_file_path.to_path_buf(),
            included_from: included_from.map(|p| p.to_path_buf())
                .unwrap_or_else(|| config_file_path.to_path_buf()),
        });
        return Ok(vec![]); // Not an error, just break the recursion
    }

    visited_paths.insert(config_file_path.to_path_buf());

    let content = tokio::fs::read_to_string(config_file_path)
        .await
        .map_err(|e| ArcellaUtilsError::IoWithPath {
            source: e,
            path: config_file_path.to_path_buf(),
        })?;

    let all_configs = load_config_recursive_from_content(
        prefix,
        &content,
        config_file_path,
        config_dir,
        current_depth,
        visited_paths,
        warnings,
    ).await?;

    // visited_paths.remove(config_file_path); // Optional, if cycles are checked only within one traversal path

    Ok(all_configs)
}

/// Parses configuration content and recursively loads included files.
///
/// This function parses the provided TOML `content`, extracts `includes`,
/// and then recursively processes those included files up to `MAX_CONFIG_DEPTH`.
/// It collects both configuration data and non-critical warnings during the process.
///
/// # Arguments
///
/// * `prefix` - The prefix to prepend to all keys in the configuration data.
/// * `content` - The string content of the TOML configuration to parse.
/// * `config_file_path` - The path to the current configuration file being processed, used for warning context.
/// * `config_dir` - The base directory used to resolve relative paths in `includes`.
/// * `current_depth` - The current recursion depth (for internal use).
/// * `visited_paths` - A set of paths already visited to prevent circular includes.
/// * `warnings` - A mutable reference to a vector where `ConfigLoadWarning`s are collected.
///
/// # Returns
///
/// A `Result` containing a `Vec<TomlFileData>` representing the loaded configurations,
/// or an `ArcellaUtilsError` if a critical error occurs.
pub async fn load_config_recursive_from_content(
    prefix: &[String],
    content: &str,
    config_file_path: &Path,
    config_dir: &Path,
    current_depth: usize,
    visited_paths: &mut HashSet<PathBuf>,
    warnings: &mut Vec<ConfigLoadWarning>,
) -> ArcellaUtilsResult<Vec<TomlFileData>> {

    let config = toml::parse_and_collect(&content, prefix)?;

     // --- Check values for Null or other issues (example) ---
    // This could be extracted into a separate function for checking TomlFileData
    for (key, value) in &config.values {
        if matches!(value, arcella_types::value::Value::Null) {
            warnings.push(ConfigLoadWarning::NullValueDetected {
                key: key.clone(),
                file: config_file_path.to_path_buf(),
            });
        }
        // For other checks if Value can be Error or another problematic type
        // if matches!(value, arcella_types::value::Value::Error(_)) { ... }
    }

    let mut all_configs = vec![config.clone()];

    let include_paths = collect_toml_includes(&config.includes, config_dir).await?;

    // --- Handle the recursive calls with Box::pin ---
    for include_path in include_paths {
        // Pin the future returned by the recursive call
        let sub_configs_future = Box::pin(load_config_recursive(
            prefix,
            &include_path,
            Some(config_file_path),
            config_dir,
            current_depth + 1,
            visited_paths,
            warnings,
        ));
        // Await the pinned future
        let mut sub_configs = sub_configs_future.await?;
        all_configs.append(&mut sub_configs);
    };

    Ok(all_configs)
}

/// Loads configuration files recursively starting from a single file, collecting data and warnings.
///
/// This is a convenience function that initializes the visited paths set and the warnings vector,
/// then calls the core `load_config_recursive` function.
///
/// # Arguments
///
/// * `prefix` - The prefix to prepend to all keys in the configuration data.
/// * `config_file_path` - The path to the initial configuration file (e.g., `arcella.toml`).
/// * `config_dir` - The base directory used to resolve relative paths in `includes`.
///
/// # Returns
///
/// A `Result` containing a tuple `(Vec<TomlFileData>, Vec<ConfigLoadWarning>)`.
/// The first element is the vector of loaded configuration data.
/// The second element is the vector of collected non-critical warnings.
pub async fn load_config_recursive_from_file(
    prefix: &[String],
    config_file_path: &Path,
    config_dir: &Path,
) -> ArcellaUtilsResult<(Vec<TomlFileData>, Vec<ConfigLoadWarning>)> {
    let mut visited = HashSet::new();
    let mut warnings = Vec::new(); // Create the warnings vector

    let configs = load_config_recursive(
        prefix,
        config_file_path, 
        None,
        config_dir,
        0,
        &mut visited,
        &mut warnings).await?;
    // Return both the configs and the accumulated warnings
    Ok((configs, warnings))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_load_config_recursive_simple() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let (configs, warnings) = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await.unwrap();

        assert_eq!(configs.len(), 1); // Main config only
        assert!(warnings.is_empty()); // No warnings expected

        // Check if the main config has the expected value
        let main_config = &configs[0];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap(), &arcella_types::value::Value::Integer(8080));
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_includes() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

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

        let (configs, warnings) = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await.unwrap();

        assert_eq!(configs.len(), 2); // Main config and included db.toml
        assert!(warnings.is_empty()); // No warnings expected

        // Check values from both configs
        let main_config = &configs[0];
        let db_config = &configs[1];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap(), &arcella_types::value::Value::Integer(8080));
        assert_eq!(db_config.values.get("arcella.database.host").unwrap(), &arcella_types::value::Value::String("localhost".to_string()));
        assert_eq!(db_config.values.get("arcella.database.port").unwrap(), &arcella_types::value::Value::Integer(5432));
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_cycle() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

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

        let (configs, warnings) = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await.unwrap();

        // Should load main.toml and cycle.toml once, then detect the cycle and stop.
        // The exact behavior might vary depending on the order of processing in collect_toml_includes,
        // but we expect at least one warning about the duplicate/cycle.
        assert!(configs.len() >= 1); // At least main.toml is loaded
        assert!(!warnings.is_empty()); // At least one warning for the cycle
        assert!(warnings.iter().any(|w| matches!(w, ConfigLoadWarning::DuplicateInclude { .. })));
    }

    #[tokio::test]
    async fn test_load_config_recursive_depth_limit() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

        // Create a chain of files that exceeds MAX_CONFIG_DEPTH
        for i in 0..=MAX_CONFIG_DEPTH + 2 { // Create more files than the limit
            let current_file = config_dir.join(format!("level_{}.toml", i));
            let mut content = format!("key_{} = {}", i, i);
            content.push_str(&format!("\nincludes = [\"level_{}.toml\"]", i + 1));
            fs::write(&current_file, content).unwrap();
        }

        let root_file = config_dir.join("level_0.toml");
        let (configs, warnings) = load_config_recursive_from_file(
            &["arcella".to_string()],
            &root_file,
            config_dir,
        ).await.unwrap();

        // Should stop after MAX_CONFIG_DEPTH
        // The exact number of loaded configs might vary slightly depending on implementation details,
        // but the key point is that it stops and generates a warning.
        assert!(configs.len() <= MAX_CONFIG_DEPTH + 1); // At most MAX_DEPTH + 1 configs (including root)
        assert!(warnings.iter().any(|w| matches!(w, ConfigLoadWarning::MaxDepthReached { .. })));
    }

    #[tokio::test]
    async fn test_load_config_recursive_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
            includes = ["nonexistent.toml"] # This file does not exist
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let result = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await;

        // Should return an error because nonexistent.toml is listed in includes
        assert!(result.is_err());
        match result.unwrap_err() {
            ArcellaUtilsError::PathNotFound { .. } => {} // Expected error type
            _ => panic!("Expected ArcellaUtilsError::PathNotFound"),
        }
    }

    #[tokio::test]
    async fn test_load_config_recursive_with_directory_in_includes() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

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

        let (configs, warnings) = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await.unwrap();

        assert_eq!(configs.len(), 2); // Main config and the file in subdir
        assert!(warnings.is_empty()); // No warnings expected

        let main_config = &configs[0];
        let sub_config = &configs[1];
        assert_eq!(main_config.values.get("arcella.server.port").unwrap(), &arcella_types::value::Value::Integer(8080));
        assert_eq!(sub_config.values.get("arcella.logging.level").unwrap(), &arcella_types::value::Value::String("info".to_string()));
    }

    // Additional test for the convenience function's return tuple structure
    #[tokio::test]
    async fn test_load_config_recursive_from_file_return_type() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path();

        let main_config_path = config_dir.join("main.toml");
        let main_config_content = r#"
            [server]
            port = 8080
        "#;
        fs::write(&main_config_path, main_config_content).unwrap();

        let result = load_config_recursive_from_file(
            &["arcella".to_string()],
            &main_config_path,
            config_dir,
        ).await;

        assert!(result.is_ok());

        let (configs, warnings) = result.unwrap();
        assert_eq!(configs.len(), 1);
        assert!(warnings.is_empty());

        // Check the type of the return value
        let _: (Vec<TomlFileData>, Vec<ConfigLoadWarning>) = (configs, warnings);
    }
}
