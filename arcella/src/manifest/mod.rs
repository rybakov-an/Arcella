// arcella/arcella/src/manifest/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

//! Module manifest parsing and validation.
//!
//! This module implements a two-layer manifest system:
//!
//! 1. **Component Manifest** (`component.toml`): Describes *what* the module is —
//!    its identity, interfaces (WIT exports/imports), and metadata.
//!    This file is **optional** for Component Model modules (interfaces can be read from .wasm),
//!    but **required** for WASI modules.
//!
//! 2. **Deployment Profile** (`deployment.toml`): Describes *how* to run the module —
//!    isolation mode, trust level, worker group, lifecycle hooks.
//!    This file is **never bundled** with the module; it is provided at install time.
//!
//! Both files are TOML-based and reside alongside the `.wasm` file during installation.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::{ArcellaError, Result as ArcellaResult};

// ================================
// 1. COMPONENT MANIFEST (portable)
// ================================

/// Describes the intrinsic properties of a WebAssembly module.
///
/// This manifest is **environment-agnostic** and focuses on identity and interface contracts.
/// For Component Model modules, much of this can be inferred from the binary.
/// For WASI modules, it must be provided externally via `component.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ComponentManifest {
    /// Human-readable name of the module (e.g., `"http-logger"`).
    ///
    /// Must be non-empty and unique within a versioned context.
    pub name: String,

    /// Semantic version of the module (e.g., `"0.1.0"`).
    ///
    /// Used for dependency resolution and hot updates.
    pub version: String,

    /// Short description (optional).
    #[serde(default)]
    pub description: Option<String>,

    /// List of WIT interfaces this module **exports** to other components.
    ///
    /// Each entry must follow the format: `namespace:interface@version`
    /// (e.g., `"logger:log@1.0"`, `"wasi:http/incoming-handler@0.2.0"`).
    #[serde(default)]
    pub exports: Vec<String>,

    /// List of WIT interfaces this module **imports** from the environment.
    ///
    /// Format is identical to `exports`. These interfaces must be provided
    /// by the runtime or other modules at link time.
    #[serde(default)]
    pub imports: Vec<String>,
    // ... other metadata fields
}

impl ComponentManifest {
    /// Attempts to load a `component.toml` file next to the given `.wasm` path.
    ///
    /// Returns `Ok(None)` if the file does not exist (allowed for Component Model modules).
    /// Returns an error only on I/O or parse failure.
    pub fn from_component_toml(wasm_path: &PathBuf) -> ArcellaResult<Option<Self>> {
        let manifest_path = wasm_path.with_file_name("component.toml");
        if !manifest_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| ArcellaError::IoWithPath(e, manifest_path))?;

        let wrapper: ComponentManifestWrapper =
            toml::from_str(&content).map_err(|e| ArcellaError::Manifest(e.to_string()))?;

        Ok(Some(wrapper.component))
    }

    /// Validates semantic correctness of the component manifest.
    pub fn validate(&self) -> ArcellaResult<()> {
        if self.name.is_empty() {
            return Err(ArcellaError::Manifest("Component name must not be empty".into()));
        }
        if self.version.is_empty() {
            return Err(ArcellaError::Manifest("Component version must not be empty".into()));
        }

        for export in &self.exports {
            if !Self::validate_interface_format(export) {
                return Err(ArcellaError::Manifest(format!(
                    "Invalid export format (expected 'interface@version'): {}",
                    export
                )));
            }
        }

        for import in &self.imports {
            if !Self::validate_interface_format(import) {
                return Err(ArcellaError::Manifest(format!(
                    "Invalid import format (expected 'interface@version'): {}",
                    import
                )));
            }
        }

        Ok(())
    }

    /// Validates that a string matches the expected WIT interface format.
    fn validate_interface_format(s: &str) -> bool {
        static RE: OnceLock<regex::Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(r"^[a-zA-Z0-9_-]+:[a-zA-Z0-9_/-]+@[a-zA-Z0-9.-]+$").unwrap()
        });
        re.is_match(s)
    }

    /// Returns the canonical module identifier: `name@version`.
    ///
    /// This ID is used internally by the runtime to uniquely reference a module.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// Wrapper to match TOML structure: `[component]`
#[derive(Deserialize)]
struct ComponentManifestWrapper {
    component: ComponentManifest,
}

// ==================================
// 2. DEPLOYMENT PROFILE (non-portable)
// ==================================

/// Describes how a module should be executed in a specific Arcella instance.
///
/// This profile is **not part of the module distribution** — it is provided
/// by the operator at install time (e.g., via `--profile deployment.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentProfile {
    /// Isolation strategy for execution.
    ///
    /// Determines whether the module runs in the main tokio thread (`Main`)
    /// or in a separate `arcella-worker` process (`Worker`).
    pub isolation: IsolationMode,

    /// Whether the module is trusted to run in the main process.
    ///
    /// Only allowed when `isolation = "main"`. Trusted modules have direct
    /// access to other trusted modules and internal runtime services.
    pub trusted: bool,

    /// Whether the module uses async WebAssembly (Component Model).
    ///
    /// Required to be `true` if `isolation = "main"`. Sync modules (e.g., WASI)
    /// must use `isolation = "worker"`.
    #[serde(rename = "async")]
    pub r#async: bool,

    /// Optional worker group name for resource sharing.
    ///
    /// Modules with the same `group` value are co-located in the same
    /// `arcella-worker` process to reduce overhead. Only applicable when
    /// `isolation = "worker"`.
    #[serde(default)]
    pub group: Option<String>,

    /// Optional configuration for lifecycle entry points.
    #[serde(default)]
    pub startup: StartupConfig,
}


impl Default for DeploymentProfile {
    /// Sensible default: untrusted worker (safe for any module).
    fn default() -> Self {
        Self {
            isolation: IsolationMode::Worker,
            trusted: false,
            r#async: false,
            group: None,
            startup: StartupConfig::default(),
        }
    }
}


impl DeploymentProfile {
    /// Loads a deployment profile from a TOML file.
    pub fn from_file(path: &PathBuf) -> ArcellaResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ArcellaError::IoWithPath(e, path.clone()))?;
        let wrapper: DeploymentProfileWrapper =
            toml::from_str(&content).map_err(|e| ArcellaError::Manifest(e.to_string()))?;
        Ok(wrapper.deployment)
    }

    /// Validates deployment constraints.
    pub fn validate(&self) -> ArcellaResult<()> {
        if self.trusted && self.isolation != IsolationMode::Main {
            return Err(ArcellaError::Manifest(
                "Only 'main' isolation can be trusted".into(),
            ));
        }

        if self.isolation == IsolationMode::Main && !self.r#async {
            return Err(ArcellaError::Manifest(
                "'main' isolation requires async = true".into(),
            ));
        }

        Ok(())
    }
}

/// Wrapper to match TOML structure: `[deployment]`
#[derive(Deserialize)]
struct DeploymentProfileWrapper {
    deployment: DeploymentProfile,
}

// ========================
// 3. SHARED TYPES
// ========================

/// Execution isolation mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationMode {
    /// Run the module directly in the main Arcella tokio thread.
    ///
    /// Requires `trusted = true` and `async = true`. Provides zero-cost
    /// interoperation with other trusted modules.
    #[serde(rename = "main")]
    Main,

    /// Run the module in a separate `arcella-worker` process.
    ///
    /// Used for untrusted or synchronous (WASI) modules. Communication
    /// occurs via IPC with serialization overhead.
    #[serde(rename = "worker")]
    Worker,
}

/// Configuration for module lifecycle entry points.
///
/// These hooks will be used in future versions (v0.3+) to invoke specific
/// WIT functions when a module is started or stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StartupConfig {
    /// Optional name of the WIT function to call on module start.
    ///
    /// If omitted, the runtime may attempt to call a default entry point
    /// (e.g., the component's default export).
    #[serde(default)]
    pub entrypoint: Option<String>,

    /// Optional name of the WIT function to call on graceful shutdown.
    ///
    /// Allows the module to release resources before termination.
    #[serde(default)]
    pub shutdown: Option<String>,
}


// ========================
// 4. VALIDATION HELPERS
// ========================

/// Validates compatibility between a component and its deployment profile.
pub fn validate_compatibility(
    component: &ComponentManifest,
    deployment: &DeploymentProfile,
) -> ArcellaResult<()> {
    // For now, no hard incompatibilities.
    // In the future: e.g., "async component cannot run in sync worker"
    Ok(())
}

// ========================
// 5. TESTS
// ========================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_component_manifest_valid() {
        let toml = r#"
            [component]
            name = "test"
            version = "0.1.0"
            exports = ["foo:bar@1.0"]
            imports = ["wasi:cli@0.2.0"]
        "#;
        let wrapper: ComponentManifestWrapper = toml::from_str(toml).unwrap();
        assert!(wrapper.component.validate().is_ok());
    }

    #[test]
    fn test_deployment_profile_valid() {
        let toml = r#"
            [deployment]
            isolation = "main"
            trusted = true
            async = true
        "#;
        let wrapper: DeploymentProfileWrapper = toml::from_str(toml).unwrap();
        assert!(wrapper.deployment.validate().is_ok());
    }

    #[test]
    fn test_deployment_profile_invalid_trusted_worker() {
        let toml = r#"
            [deployment]
            isolation = "worker"
            trusted = true
            async = false
        "#;
        let wrapper: DeploymentProfileWrapper = toml::from_str(toml).unwrap();
        assert!(wrapper.deployment.validate().is_err());
    }

    #[test]
    fn test_load_component_toml_missing() {
        let temp_dir = TempDir::new().unwrap();
        let wasm_path = temp_dir.path().join("module.wasm");
        fs::write(&wasm_path, b"dummy").unwrap();

        let manifest = ComponentManifest::from_component_toml(&wasm_path).unwrap();
        assert!(manifest.is_none());
    }

    #[test]
    fn test_load_component_toml_exists() {
        let temp_dir = TempDir::new().unwrap();
        let wasm_path = temp_dir.path().join("module.wasm");
        let toml_path = temp_dir.path().join("component.toml");
        fs::write(&wasm_path, b"dummy").unwrap();
        fs::write(
            &toml_path,
            r#"
                [component]
                name = "test"
                version = "0.1.0"
            "#,
        )
        .unwrap();

        let manifest = ComponentManifest::from_component_toml(&wasm_path).unwrap();
        assert!(manifest.is_some());
        assert_eq!(manifest.unwrap().name, "test");
    }

    #[test]
    fn test_interface_format() {
        assert!(ComponentManifest::validate_interface_format("ns:iface@1.0"));
        assert!(ComponentManifest::validate_interface_format("wasi:http/incoming@0.2.0"));
        assert!(!ComponentManifest::validate_interface_format("bad"));
        assert!(!ComponentManifest::validate_interface_format("ns:iface"));
    }
}