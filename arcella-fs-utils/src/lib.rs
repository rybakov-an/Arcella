// arcella/arcella-fs-utils/src/lib.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! File system and TOML configuration utilities for Arcella.
//!
//! This crate provides common functions for:
//! - Resolving base directories based on executable location or environment.
//! - Finding and validating `.toml` configuration files.
//! - Collecting files specified by `includes` patterns.
//! - Converting TOML values into a serializable format used by Arcella.
//! - Loading configurations recursively with warnings.
//!
//! It is designed to be used by the Arcella runtime and other tools that need
//! to process TOML-based configurations in a consistent way.

use futures::future;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

pub mod config_loader;
pub use config_loader::*;

pub mod error;
use crate::error::{ArcellaUtilsError, Result as ArcellaResult};

pub mod toml;

pub mod types;
pub use types::*;

pub mod warnings;
pub use warnings::*;

/// Determines the base directory for Arcella based on the executable location or environment.
///
/// The function follows this priority order:
/// 1. If the executable is located in a `bin` subdirectory, the parent of `bin` is used.
/// 2. If the current directory (where the executable is run from) contains a `config` subdirectory,
///    the current directory is used.
/// 3. Otherwise, the user's home directory joined with `.arcella` is used.
///
/// # Returns
///
/// A `Result` containing the determined `PathBuf` or an error if the home directory
/// cannot be determined.
pub async fn find_base_dir() -> ArcellaResult<PathBuf> {
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            // Case 1: executable is in a `bin` directory
            if parent.file_name() == Some(std::ffi::OsStr::new("bin")) {
                if let Some(grandparent) = parent.parent() {
                    return Ok(grandparent.to_path_buf());
                }
                // If `/bin/app`, grandparent is root — still valid
                // But if somehow `bin` is root (shouldn't happen), fall through
            }

            // Case 2: check if current_exe's parent has a `config` dir
            let local_config = parent.join("config");
            if local_config.is_dir() {
                return Ok(parent.to_path_buf());
            }
        }
    }

    // Case 3: fallback to ~/.arcella
    dirs::home_dir()
        .map(|d| d.join(".arcella"))
        .ok_or_else(|| ArcellaUtilsError::Internal("Cannot determine home directory".into()))
}

/// Checks if a path represents a regular file with a `.toml` extension
/// but *not* a `.template.toml` extension.
///
/// This function performs the following checks:
/// - The path must have a file name (i.e., not be `"."`, `".."`, or root).
/// - The file extension must be `.toml` (case-insensitive).
/// - The full file name must *not* end with `.template.toml` (case-insensitive).
///
/// Note: This function does **not** check whether the path exists or is a file on disk.
/// It only inspects the path components.
///
/// # Arguments
///
/// * `path` - The path to check.
///
/// # Returns
///
/// `true` if the path is a valid TOML file according to the criteria, `false` otherwise.
pub fn is_valid_toml_file_path(path: &Path) -> bool {

    // 1. Get the file name as a string (return false if missing)
    let file_name = match path.file_name() {
        Some(name) => name.to_string_lossy(),
        None => return false,
    };

    let file_name_lower = file_name.to_lowercase();

    // 2. Must end with ".toml"
    if !file_name_lower.ends_with(".toml") {
        return false;
    }

    // 3. Must NOT end with ".template.toml"
    if file_name_lower.ends_with(TEMPLATE_TOML_SUFFIX) {
        return false;
    }

    // If all checks pass, it's a valid TOML config file
    true
}

/// Finds all `.toml` files in a given directory, excluding `.template.toml` files.
///
/// This function scans the specified directory for regular files with the `.toml`
/// extension (case-insensitive), ignoring any files ending in `.template.toml`.
/// The resulting file paths are sorted lexicographically (case-insensitive).
///
/// # Arguments
///
/// * `dir_path` - The path to the directory to scan.
///
/// # Returns
///
/// A `Result` containing:
/// - `Ok(Some(Vec<PathBuf>))` with a sorted list of valid `.toml` file paths if the path exists and is a directory.
/// - `Ok(None)` if the path exists but is not a directory.
/// - `Err(ArcellaUtilsError)` if an I/O error occurs while accessing the path.
pub async fn find_toml_files_in_dir(dir_path: &Path) -> ArcellaResult<Option<Vec<PathBuf>>> {
    // Check that the path exists and is a directory
    let metadata = fs::metadata(dir_path).await
        .map_err(|e| ArcellaUtilsError::IoWithPath { source: e, path: dir_path.to_path_buf() })?;

    if !metadata.is_dir() {
        return Ok(None);
    }

    let mut dir_entries = fs::read_dir(dir_path).await
        .map_err(|e| ArcellaUtilsError::IoWithPath { source: e, path: dir_path.to_path_buf() })?;

    let mut toml_files = Vec::new();

    while let Some(entry) = dir_entries.next_entry().await
        .map_err(|e| ArcellaUtilsError::IoWithPath { source: e, path: dir_path.to_path_buf() })?
    {
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        if is_valid_toml_file_path(&path) {
            toml_files.push(path);
        }
    }

    // Sort the files by name without case sensitivity
    toml_files.sort_by_key(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
    });

    Ok(Some(toml_files))
}

/// Collects all `.toml` files specified by `includes` patterns relative to a base directory.
///
/// This function:
/// 1. Resolves all patterns in `includes` to absolute paths based on `config_dir`.
/// 2. Concurrently checks the existence and type (file/directory) of all resolved paths.
/// 3. Separates resolved paths into files and directories.
/// 4. For each resolved file, checks if it's a valid `.toml` file (not `.template.toml`) and includes it.
/// 5. For each resolved directory, finds all valid `.toml` files directly within it (non-recursive).
/// 6. Returns a sorted vector of unique file paths.
///
/// If a resolved path in `includes` does not exist (neither file nor directory), an error is returned.
/// Duplicate paths (e.g., from overlapping patterns) are removed.
///
/// # Arguments
///
/// * `includes` - A vector of string patterns representing file or directory paths to include.
/// * `config_dir` - The base directory to resolve relative paths against.
///
/// # Returns
///
/// A `Result` containing a sorted vector of unique `PathBuf`s pointing to valid `.toml` files,
/// or an error if an I/O issue occurs during directory scanning or if a path in `includes` does not exist.
pub async fn collect_toml_includes(
    includes: &[String],
    config_dir: &Path,
    warnings: &mut Vec<ConfigLoadWarning>,
) -> ArcellaResult<Vec<PathBuf>> {
    let all_paths = resolve_include_paths(includes, config_dir)?;

    // Concurrently check the metadata for all resolved paths
    let metadata_futures: Vec<_> = all_paths
        .iter()
        .map(|path| async move {
            let metadata_res = fs::metadata(&path).await;
            (path.clone(), metadata_res)
        })
        .collect();

    let metadata_results = future::join_all(metadata_futures).await;

    let mut include_files = Vec::new();
    let mut include_dirs = Vec::new();

    for (path, metadata_res) in metadata_results {
        match metadata_res {
            Ok(metadata) => {
                if metadata.is_file() {
                    include_files.push(path);
                } else if metadata.is_dir() {
                    include_dirs.push(path);
                } else {
                    // Path exists but is not a regular file or directory (e.g., socket, device)
                    warnings.push(ConfigLoadWarning::SkippedInvalidFile {
                        path: path.clone(),
                    });
                }
            }
            Err(_) => {
                // Path does not exist → silently skip and warn
                warnings.push(ConfigLoadWarning::SkippedInvalidFile {
                    path: path.clone(),
                });
            }
        }
    }

    // Process files and directories concurrently
    let file_check_futures = include_files.into_iter().map(|file_path| async move {
        if is_valid_toml_file_path(&file_path) {
            Ok(Some(file_path))
        } else {
            // Log or ignore invalid files if needed, returning None
            Ok::<Option<PathBuf>, ArcellaUtilsError>(None)
        }
    });

    let dir_scan_futures = include_dirs.into_iter().map(|dir_path| async move {
        // find_toml_files_in_dir returns Option<Vec<PathBuf>>, we map it to Vec<PathBuf>
        find_toml_files_in_dir(&dir_path).await.map(|opt| opt.unwrap_or_default())
    });

    // Execute all file checks and directory scans in parallel
    let file_results = future::join_all(file_check_futures).await;
    let dir_results = future::join_all(dir_scan_futures).await;

    // Collect results from file checks (filtering out None)
    let mut collected_files = Vec::new();
    for result in file_results {
        if let Some(file_path) = result? {
            collected_files.push(file_path);
        }
    }

    // Collect results from directory scans
    for dir_result in dir_results {
        let toml_files = dir_result?; // This is Vec<PathBuf> from find_toml_files_in_dir
        collected_files.extend(toml_files);
    }

    // Use a HashSet to ensure uniqueness
    let unique_files: HashSet<PathBuf> = collected_files.into_iter().collect();

    // Convert back to Vec and sort
    let mut final_list: Vec<PathBuf> = unique_files.into_iter().collect();
    final_list.sort_by_key(|p| p.to_string_lossy().to_lowercase());

    Ok(final_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    mod find_toml_tests {
        use super::*;

        #[tokio::test]
        async fn test_find_toml_files_in_dir_valid_directory() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create some test files
            fs::write(dir_path.join("config1.toml"), "# Config 1").unwrap();
            fs::write(dir_path.join("config2.toml"), "# Config 2").unwrap();
            fs::write(dir_path.join("not_a_config.txt"), "Text file").unwrap();
            fs::write(dir_path.join("Config3.TOML"), "# Config 3 (uppercase)").unwrap(); // Check case-insensitivity
            fs::write(dir_path.join("normal.template.toml"), "# Normal Template").unwrap(); // Should be excluded

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 3); // config1.toml, config2.toml, Config3.TOML

            let expected_names: Vec<String> = vec![
                "config1.toml",
                "config2.toml", 
                "Config3.TOML"
            ].into_iter().map(|s| s.to_string()).collect();

            let actual_names: Vec<String> = files.iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect();

            assert_eq!(actual_names, expected_names);
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_empty_directory() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert!(files.is_empty());
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_nonexistent_path() {
            let nonexistent_path = Path::new("/this/path/definitely/does/not/exist/arcella_test_dir");
            
            let result = find_toml_files_in_dir(nonexistent_path).await;
            
            assert!(result.is_err());
            // Check that the error is IoWithPath
            match result.unwrap_err() {
                ArcellaUtilsError::IoWithPath { .. } => {}, // OK
                _ => panic!("Expected ArcellaError::IoWithPath"),
            }
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_file_instead_of_dir() {
            let temp_dir = TempDir::new().unwrap();
            let file_path = temp_dir.path().join("not_a_dir.toml");
            fs::write(&file_path, "# Just a file").unwrap();

            let result = find_toml_files_in_dir(&file_path).await.unwrap();
            
            assert!(result.is_none()); // Path is a file, not a directory
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_nested_dirs_ignored() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create a subdirectory
            let sub_dir = dir_path.join("subdir");
            fs::create_dir(&sub_dir).unwrap();

            // Create .toml files in both the main dir and sub dir
            fs::write(dir_path.join("main_config.toml"), "# Main").unwrap();
            fs::write(sub_dir.join("nested_config.toml"), "# Nested").unwrap(); // This should not be found

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 1); // Only main_config.toml
            assert!(files[0].file_name().unwrap().to_string_lossy().contains("main_config.toml"));
        }

        #[tokio::test]
        async fn test_find_toml_files_in_dir_sorted_order() {
            let temp_dir = TempDir::new().unwrap();
            let dir_path = temp_dir.path();

            // Create .toml files in non-lexicographic order
            fs::write(dir_path.join("z.toml"), "# Z").unwrap();
            fs::write(dir_path.join("a.toml"), "# A").unwrap();
            fs::write(dir_path.join("m.toml"), "# M").unwrap();

            let result = find_toml_files_in_dir(dir_path).await.unwrap();
            let files = result.expect("Should return Some");

            assert_eq!(files.len(), 3);
            // Check that files are sorted
            assert!(files[0].file_name().unwrap().to_string_lossy() == "a.toml");
            assert!(files[1].file_name().unwrap().to_string_lossy() == "m.toml");
            assert!(files[2].file_name().unwrap().to_string_lossy() == "z.toml");
        }
    }    

    mod collect_toml_includes_tests {
        use super::*;

        #[tokio::test]
        async fn test_collect_toml_includes_mixed() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            // Create a subdirectory in config_dir
            let sub_dir = config_dir.join("sub");
            fs::create_dir(&sub_dir).unwrap();

            // Create files in config_dir
            let config1_path = config_dir.join("config1.toml");
            let config2_path = config_dir.join("config2.toml");
            let not_toml_path = config_dir.join("not_config.txt");
            let template_path = config_dir.join("template.template.toml");

            fs::write(&config1_path, "# Config 1").unwrap();
            fs::write(&config2_path, "# Config 2").unwrap();
            fs::write(&not_toml_path, "Not a toml file").unwrap();
            fs::write(&template_path, "# Template file").unwrap();

            // Create files in the subdirectory
            let sub_config1_path = sub_dir.join("sub_config1.toml");
            let sub_config2_path = sub_dir.join("sub_config2.toml");
            let sub_not_toml_path = sub_dir.join("sub_not_config.txt");
            let sub_template_path = sub_dir.join("sub_template.template.toml");

            fs::write(&sub_config1_path, "# Sub Config 1").unwrap();
            fs::write(&sub_config2_path, "# Sub Config 2").unwrap();
            fs::write(&sub_not_toml_path, "Not a toml file").unwrap();
            fs::write(&sub_template_path, "# Sub Template file").unwrap();

            let includes = vec![
                "config1.toml".to_string(),           // file in config_dir - OK
                "sub/".to_string(),                   // directory - OK
                "config2.toml".to_string(),           // another file in config_dir - OK
                "sub/sub_config2.toml".to_string(),   // file in subdirectory - OK
            ];

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await;

            // This should now succeed as all paths exist.
            assert!(result.is_ok(), "collect_toml_includes should succeed when all paths in includes exist");
            let collected = result.unwrap();

            let mut expected_paths = vec![
                config1_path,
                config2_path,
                sub_config1_path, // from find_toml_files_in_dir(sub_dir)
                sub_config2_path, // from includes list
            ];
            expected_paths.sort_by_key(|p| p.to_string_lossy().to_lowercase());

            assert_eq!(collected, expected_paths);
        }

        #[tokio::test]
        async fn test_collect_toml_includes_empty_includes() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let includes = vec![];

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await.unwrap();

            assert!(result.is_empty());
        }

        #[tokio::test]
        async fn test_collect_toml_includes_only_dirs() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let sub_dir1 = config_dir.join("sub1");
            let sub_dir2 = config_dir.join("sub2");
            fs::create_dir(&sub_dir1).unwrap();
            fs::create_dir(&sub_dir2).unwrap();

            // Create .toml files in subdirectories
            fs::write(sub_dir1.join("a.toml"), "# A").unwrap();
            fs::write(sub_dir1.join("b.toml"), "# B").unwrap();
            fs::write(sub_dir2.join("c.toml"), "# C").unwrap();
            // Add files that should not be included
            fs::write(sub_dir1.join("d.txt"), "# D").unwrap();

            let includes = vec![
                "sub1/".to_string(),
                "sub2/".to_string(),
            ];

            let mut expected_paths = vec![
                sub_dir1.join("a.toml"),
                sub_dir1.join("b.toml"),
                sub_dir2.join("c.toml"),
            ];
            expected_paths.sort_by_key(|p| p.to_string_lossy().to_lowercase());

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await.unwrap();
            assert_eq!(result, expected_paths);
        }

        #[tokio::test]
        async fn test_collect_toml_includes_only_files() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            // Create .toml files in config_dir
            let file1_path = config_dir.join("file1.toml");
            let file2_path = config_dir.join("file2.toml");
            fs::write(&file1_path, "# File 1").unwrap();
            fs::write(&file2_path, "# File 2").unwrap();

            // Create files that should not be included
            fs::write(config_dir.join("file3.txt"), "# File 3").unwrap();

            let includes = vec![
                "file1.toml".to_string(),
                "file2.toml".to_string(),
                "file3.txt".to_string(), // Not .toml
            ];

            let mut expected_paths = vec![file1_path, file2_path];
            expected_paths.sort_by_key(|p| p.to_string_lossy().to_lowercase());

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await.unwrap();
            assert_eq!(result, expected_paths);
        }

        #[tokio::test]
        async fn test_collect_toml_includes_nonexistent_dir_in_includes() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let includes = vec![
                "nonexistent_dir/".to_string(),
            ];

            let mut warnings = Vec::new();

            // resolve_include_paths will just create the path config_dir.join("nonexistent_dir/"), it does not check its existence.
            // Then in collect_toml_includes, fs::metadata(path) will be called and will fail.
            // Therefore, it should return an error.
            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await;
            assert!(result.is_ok());
            assert!(warnings.len() == 1, "Should have one warning about nonexistent dir");
        }

        #[tokio::test]
        async fn test_collect_toml_includes_nonexistent_file_in_includes() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let includes = vec![
                "nonexistent_file.toml".to_string(),
            ];

            let mut warnings = Vec::new();

            // resolve_include_paths will create the path config_dir.join("nonexistent_file.toml").
            // Then in collect_toml_includes, fs::metadata(path) will be called and will fail.
            // Therefore, it should return an error.
            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await;
            assert!(result.is_ok());
            assert!(warnings.len() == 1, "Should have one warning about nonexistent file");
        }

        #[tokio::test]
        async fn test_collect_toml_includes_duplicate_paths() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let file1_path = config_dir.join("file1.toml");
            fs::write(&file1_path, "# File 1").unwrap();

            // Create a subdirectory and put the same-named file there (a symlink or copy, but for the test just a different path)
            let sub_dir = config_dir.join("sub");
            fs::create_dir(&sub_dir).unwrap();
            let file1_in_sub_path = sub_dir.join("file1.toml");
            let _ = fs::write(&file1_in_sub_path, "# File 1 in sub"); // Physically a different file, but name matches an include path

            // But in includes we specify one file twice and a directory containing another file
            let includes = vec![
                "file1.toml".to_string(), // points to config_dir/file1.toml
                "file1.toml".to_string(), // duplicate
                "sub/".to_string(),       // contains config_dir/sub/file1.toml
            ];

            // find_toml_files_in_dir looks only in sub_dir, i.e., config_dir/sub/file1.toml
            // includes points to config_dir/file1.toml twice.
            // The result should contain config_dir/file1.toml (once due to deduplication) and config_dir/sub/file1.toml.
            let mut expected_paths = vec![
                file1_path, // from includes
                file1_in_sub_path, // from find_toml_files_in_dir(sub_dir)
            ];
            expected_paths.sort_by_key(|p| p.to_string_lossy().to_lowercase());

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await.unwrap();
            assert_eq!(result, expected_paths);
        }

        #[tokio::test]
        async fn test_collect_toml_includes_case_insensitive_extension() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path();

            let file_upper_path = config_dir.join("FILE_UPPER.TOML");
            let file_lower_path = config_dir.join("file_lower.toml");
            fs::write(&file_upper_path, "# FILE UPPER").unwrap();
            fs::write(&file_lower_path, "# file lower").unwrap();

            let sub_dir = config_dir.join("sub");
            fs::create_dir(&sub_dir).unwrap();
            fs::write(sub_dir.join("SUB_FILE.TOML"), "# SUB FILE").unwrap();

            let includes = vec![
                "FILE_UPPER.TOML".to_string(),
                "sub/".to_string(),
            ];

            let mut expected_paths = vec![
                file_upper_path, // from includes
                sub_dir.join("SUB_FILE.TOML"), // from find_toml_files_in_dir
            ];
            expected_paths.sort_by_key(|p| p.to_string_lossy().to_lowercase());

            let mut warnings = Vec::new();

            let result = collect_toml_includes(&includes, config_dir, &mut warnings).await.unwrap();
            assert_eq!(result, expected_paths);
        }
    }   

    #[test]
    fn test_is_valid_toml_file_path_edge_cases() {
        use std::path::Path;

        // Valid cases
        assert!(is_valid_toml_file_path(Path::new("config.toml")));
        assert!(is_valid_toml_file_path(Path::new("Config.TOML")));
        assert!(is_valid_toml_file_path(Path::new("my-config_v2.toml")));

        // Invalid: .template.toml
        assert!(!is_valid_toml_file_path(Path::new("template.template.toml")));
        assert!(!is_valid_toml_file_path(Path::new("foo.TEMPLATE.TOML")));

        // Invalid: wrong extension
        assert!(!is_valid_toml_file_path(Path::new("config.json")));
        assert!(!is_valid_toml_file_path(Path::new("config.toml.bak")));

        // Edge cases
        assert!(!is_valid_toml_file_path(Path::new(".")));
        assert!(!is_valid_toml_file_path(Path::new("..")));
        assert!(!is_valid_toml_file_path(Path::new("/")));
        assert!(!is_valid_toml_file_path(Path::new("")));
    }

}
