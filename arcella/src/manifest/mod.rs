// arcella/arcella/src/manifest/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use serde::{Deserialize, Serialize};
use std::path::{Path};

use crate::error::{ArcellaError, Result as ArcellaResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub module: ModuleMetadata,
    #[serde(default)]
    pub startup: StartupConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleMetadata {
    pub name: String,
    pub version: String,
    pub isolation: IsolationMode,
    pub trusted: bool,
    pub r#async: bool,
    #[serde(default)]
    pub group: Option<String>,

    /// Export interface list in format "namespace:interface@version"
    #[serde(default)]
    pub exports: Vec<String>,
    /// Required import list
    #[serde(default)]
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationMode {
    #[serde(rename = "main")]
    Main,
    #[serde(rename = "worker")]
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StartupConfig {
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub shutdown: Option<String>,
}

impl ModuleManifest {
    pub fn from_wasm_path(wasm_path: &Path) -> Result<Self, ArcellaError> {

        let manifest_path = wasm_path.with_file_name("arcella.toml");
        if !manifest_path.exists() {
            return Err(ArcellaError::Manifest(
                format!("arcella.toml not found at {:?}", manifest_path),
            ));
        }

        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| ArcellaError::IoWithPath(e, manifest_path))?;

        toml::from_str(&content)
            .map_err(|e| ArcellaError::Manifest(e.to_string()))

    }

    pub fn validate(&self) -> Result<(), ArcellaError> {

        if self.module.name.is_empty() {
            return Err(ArcellaError::Manifest("Module name must not be empty".into()));
        }
        if self.module.version.is_empty() {
            return Err(ArcellaError::Manifest("Module version must not be empty".into()));
        }

        // trusted = true only for isolation = "main"
        if self.module.trusted && self.module.isolation != IsolationMode::Main {
            return Err(ArcellaError::Manifest(
                "Only 'main' isolation can be trusted".into(),
            ));
        }

        // async = true required for isolation = "main"
        if self.module.isolation == IsolationMode::Main && !self.module.r#async {
            return Err(ArcellaError::Manifest(
                "'main' isolation requires async = true".into(),
            ));
        }

        for export in &self.module.exports {
            if !export.contains('@') {
                return Err(ArcellaError::Manifest(
                    format!("Invalid export format (expected 'interface@version'): {}", export)
                ));
            }
        }

        for import in &self.module.imports {
            if !import.contains('@') {
                return Err(ArcellaError::Manifest(
                    format!("Invalid export format (expected 'interface@version'): {}", import)
                ));
            }
        }

        Ok(())

    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_manifest() {
        let test_toml = r#"
            [module]
            name = "test"
            version = "0.1.0"
            isolation = "main"
            trusted = true
            async = true
            exports = ["foo:bar@1.0"]
            imports = ["wasi:cli@0.2.0"]
        "#;
        let manifest: ModuleManifest = toml::from_str(test_toml).unwrap();
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_invalid_trusted_worker() {
        let test_toml = r#"
            [module]
            name = "test"
            version = "0.1.0"
            isolation = "worker"
            trusted = true
            async = false
        "#;
        let manifest: ModuleManifest = toml::from_str(test_toml).unwrap();
        assert!(manifest.validate().is_err());
    }
}