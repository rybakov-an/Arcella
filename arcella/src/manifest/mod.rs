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
//! This module implements a three-layer manifest system:
//!
//! 1. **Component Manifest** (`component.toml`): Describes *what* the module is —
//!    its identity, interfaces (WIT exports/imports), and metadata.
//!    This file is **optional** for Component Model modules (interfaces can be read from .wasm),
//!    but **required** for WASI modules.
//!
//! 2. **Deployment Template** (`deployment-template.toml`): Describes *how* to run the module —
//!    isolation mode, trust level, async mode, and lifecycle hooks.
//!    This file is **optional** and provides default deployment recommendations.
//!
//! 3. **Deployment Specification** (`*.deployment.toml`): Describes *where* and *how many*
//!    instances to run — target worker group, replica count, and runtime overrides.
//!    This file is **created by administrators** for specific deployment scenarios.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use regex::Regex;
use std::sync::OnceLock;
use wasmtime::{
    Engine,
    component::{
        Component, 
        ComponentType,
        types::{
            ComponentItem,
        },
    }
};

use arcella_types::spec::{self, ComponentItemSpec};
use arcella_wasmtime::ComponentItemSpecExt;

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

    /// Tree of WIT interfaces this module **exports** to other components.
    ///
    /// Each entry must follow the format: `namespace:interface@version`
    /// (e.g., `"logger:log@1.0"`, `"wasi:http/incoming-handler@0.2.0"`).
    #[serde(default)]
    pub exports: HashMap<String, ComponentItemSpec>,

    /// Tree of WIT interfaces this module **imports** from the environment.
    ///
    /// Format is identical to `exports`. These interfaces must be provided
    /// by the runtime or other modules at link time.
    #[serde(default)]
    pub imports: HashMap<String, ComponentItemSpec>,

    /// Component capabilities and requirements
    #[serde(default)]
    pub capabilities: ComponentCapabilities,

    // ... other metadata fields
}

impl ComponentManifest {
    /// Attempts to load a `component.toml` file next to the given `.wasm` path.
    ///
    /// Returns `Ok(None)` if the file does not exist (allowed for Component Model modules).
    /// Returns an error only on I/O or parse failure.
    pub fn from_component_toml(wasm_path: &Path) -> ArcellaResult<Option<Self>> {
        let manifest_path = wasm_path.with_file_name("component.toml");
        if !manifest_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| ArcellaError::IoWithPath(e, manifest_path))?;

        let wrapper: ComponentManifestWrapper =
            toml::from_str(&content).map_err(|e| ArcellaError::Manifest(e.to_string()))?;

        let manifest = wrapper.component;
        manifest.validate()?;
        
        Ok(Some(manifest))
    }

    /// Extracts component metadata directly from a WebAssembly Component binary.
    ///
    /// This function:
    /// - Only works with **WebAssembly Components** (not core Wasm or WASI preview1 modules).
    /// - Extracts imports and exports in the format `namespace:interface`.
    /// - Does **not** include version (`@x.y`) — this must be provided via `component.toml`
    ///   or inferred from file naming convention if needed later.
    /// - Requires a valid `name` and `version` — since they are not stored in Wasm,
    ///   this function returns an error. In practice, you should derive them from
    ///   the filename (e.g., `http-logger@0.1.0.wasm`) or require `component.toml`.
    ///
    /// For MVP v0.2.3, we assume that if `component.toml` is missing,
    /// the filename encodes `name@version`.
    pub fn from_wasm(engine: &Engine, wasm_path: &Path) -> ArcellaResult<Self> {

        if !wasm_path.exists() {
            return Err(ArcellaError::IoWithPath(
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {:?}", wasm_path)
                ),
                wasm_path.into(),
            ));
        }

        let file_stem = wasm_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| ArcellaError::Manifest("Invalid .wasm filename".into()))?;        

        if !validate_module_id(file_stem) {
            return Err(ArcellaError::Manifest(
                "Filename must be 'name@version.wasm' for components without component.toml".into()
            ));
        }

        let (name, version) = file_stem
            .split_once('@')
            .ok_or_else(|| ArcellaError::Manifest("Expected 'name@version' format".into()))?;

        let component = Component::from_file(engine, &wasm_path)
            .map_err(ArcellaError::Wasmtime)?;
        
        let component_type = component.component_type();

        let exports: HashMap<String, ComponentItemSpec> = component_type
            .exports(engine)
            .map(|(name, item)| {
                let spec = item.to_spec(engine).unwrap_or_else(|e| {
                    ComponentItemSpec::Unknown {
                        debug: Some(format!("Export '{}': {:?}", name, e)),
                    }
                });
                (name.into(), spec)
            })
            .collect();

        let imports: HashMap<String, ComponentItemSpec> = component_type
            .imports(engine)
            .map(|(name, item)| {
                let spec = item.to_spec(engine).unwrap_or_else(|e| {
                    ComponentItemSpec::Unknown {
                        debug: Some(format!("Import '{}': {:?}", name, e)),
                    }
                });
                (name.into(), spec)
            })
            .collect();

        let manifest = Self {
            name: name.into(),
            version: version.into(),
            description: None,
            exports: exports,
            imports: imports,
            capabilities: ComponentCapabilities::default(),
        };

        manifest.validate()?;
        Ok(manifest)

    }

    /// Validates that a string is a valid WIT interface identifier (without version).
    fn validate_interface_name_for_wit(s: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+(:[a-zA-Z0-9_/-]+)?$").unwrap()
        });
        re.is_match(s)
    }

    /// Validates semantic correctness of the component manifest.
    pub fn validate(&self) -> ArcellaResult<()> {
        if self.name.is_empty() {
            return Err(ArcellaError::Manifest("Component name must not be empty".into()));
        }
        if self.version.is_empty() {
            return Err(ArcellaError::Manifest("Component version must not be empty".into()));
        }

        // Validate name format (alphanumeric, hyphens, underscores)
        if !Self::validate_name_format(&self.name) {
            return Err(ArcellaError::Manifest(
                "Component name must contain only alphanumeric characters, hyphens, and underscores".into()
            ));
        }

        // Validate version format (semver-like)
        if !Self::validate_version_format(&self.version) {
            return Err(ArcellaError::Manifest(
                "Component version must follow semantic versioning format (e.g., 0.1.0)".into()
                        ));
        }

        Ok(())
    }

    /// Validates component name format
    fn validate_name_format(name: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap()
        });
        re.is_match(name)
    }

    /// Validates version format (simplified semver)
    fn validate_version_format(version: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^\d+\.\d+\.\d+([+-][a-zA-Z0-9.-]+)?$").unwrap()
        });
        re.is_match(version)
    }

    /// Validates that a string matches the expected WIT interface format.
    fn validate_interface_format(s: &str) -> bool {
        static RE_WITH_VERSION: OnceLock<Regex> = OnceLock::new();
        static RE_WITHOUT_VERSION: OnceLock<Regex> = OnceLock::new();
        
        let re1 = RE_WITH_VERSION.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+:[a-zA-Z0-9_/-]+@[a-zA-Z0-9.+_-]+$").unwrap()
        });
        let re2 = RE_WITHOUT_VERSION.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+:[a-zA-Z0-9_/-]+$").unwrap()
        });
        
        re1.is_match(s) || re2.is_match(s)
    }

    /// Returns the canonical module identifier: `name@version`.
    ///
    /// This ID is used internally by the runtime to uniquely reference a module.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// Component capabilities and requirements
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ComponentCapabilities {
    /// Required WASI preview2 capabilities
    #[serde(default)]
    pub wasi: Vec<String>,
    
    /// Required filesystem access paths
    #[serde(default)]
    pub filesystem: Vec<String>,
    
    /// Required network access
    #[serde(default)]
    pub network: Vec<String>,
    
    /// Environment variables needed
    #[serde(default)]
    pub environment: Vec<String>,
}

/// Wrapper to match TOML structure: `[component]`
#[derive(Deserialize)]
struct ComponentManifestWrapper {
    component: ComponentManifest,
}

// ==================================
// 2. DEPLOYMENT TEMPLATE (optional recommendations)
// ==================================

/// Recommended deployment configuration that ships with the component.
///
/// This template provides sensible defaults but can be overridden by
/// deployment specifications. It does NOT specify group or replica count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentTemplate {
    /// Recommended isolation strategy
    /// 
    /// Determines whether the module runs in the main tokio thread (`Main`)
    /// or in a separate `arcella-worker` process (`Worker`).
    pub isolation: IsolationMode,

    /// Recommended trust level
    ///
    /// Only allowed when `isolation = "main"`. Trusted modules have direct
    /// access to other trusted modules and internal runtime services.
    pub trusted: bool,

    /// Whether the module uses async WebAssembly
    ///
    /// Required to be `true` if `isolation = "main"`. Sync modules (e.g., WASI)
    /// must use `isolation = "worker"`.
    #[serde(rename = "async")]
    pub r#async: bool,

    /// Recommended worker group name (if using worker isolation)
    ///
    /// Modules with the same `group` value are co-located in the same
    /// `arcella-worker` process to reduce overhead. Only applicable when
    /// `isolation = "worker"`.
    #[serde(default)]
    pub group: Option<String>,

    /// Lifecycle hook configuration
    #[serde(default)]
    pub startup: StartupConfig,

    /// Resource limits and requirements
    #[serde(default)]
    pub resources: ResourceRequirements,
}

impl Default for DeploymentTemplate {
    /// Sensible default: untrusted worker (safe for any module).
    fn default() -> Self {
        Self {
            isolation: IsolationMode::Worker,
            trusted: false,
            r#async: false,
            group: None,
            startup: StartupConfig::default(),
            resources: ResourceRequirements::default(),
        }
    }
}

impl DeploymentTemplate {
    /// Loads a deployment template from a TOML file.
    pub fn from_file(path: &Path) -> ArcellaResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ArcellaError::IoWithPath(e, path.into()))?;
        let wrapper: DeploymentTemplateWrapper =
            toml::from_str(&content).map_err(|e| ArcellaError::Manifest(e.to_string()))?;
        
        let template = wrapper.deployment;
        template.validate()?;
        
        Ok(template)
    }

    /// Attempts to load a deployment template next to the given `.wasm` path.
    pub fn from_template_toml(wasm_path: &Path) -> ArcellaResult<Option<Self>> {
        let template_path = wasm_path.with_file_name("deployment-template.toml");
        if !template_path.exists() {
            return Ok(None);
        }

        Self::from_file(&template_path).map(Some)
    }

    /// Validates template constraints.
    pub fn validate(&self) -> ArcellaResult<()> {
        validate_isolation_constraints(
            &self.isolation,
            self.trusted,
            self.r#async,
        )?;

        if self.group.is_some() && self.isolation != IsolationMode::Worker {
            return Err(ArcellaError::Manifest(
                "Group can only be specified for worker isolation".into(),
            ));
        }

        if let Some(ref group) = self.group {
            if !ComponentManifest::validate_name_format(group) {
                return Err(ArcellaError::Manifest("Invalid group name format".into()));
            }
        }

        Ok(())
    }
}

/// Wrapper to match TOML structure: `[deployment]`
#[derive(Deserialize)]
struct DeploymentTemplateWrapper {
    deployment: DeploymentTemplate,
}

// ==================================
// 3. DEPLOYMENT SPECIFICATION (runtime-specific)
// ==================================

/// Runtime-specific deployment configuration created by administrators.
///
/// This specifies exactly how and where to run the component in a specific
/// Arcella instance, including target group and replica count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentSpec {
    /// Component ID to deploy (e.g., "http-logger@0.1.0")
    pub module_id: String,

    /// Target worker group for this deployment
    pub group: String,

    /// Number of replicas to run
    pub replicas: u32,

    /// Optional overrides for deployment template parameters
    #[serde(default)]
    pub overrides: DeploymentOverrides,
}

impl DeploymentSpec {
    /// Loads a deployment specification from a TOML file.
    pub fn from_file(path: &Path) -> ArcellaResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ArcellaError::IoWithPath(e, path.into()))?;
        
        let wrapper: DeploymentSpecWrapper =
            toml::from_str(&content).map_err(|e| ArcellaError::Manifest(e.to_string()))?;
        
        let spec = wrapper.deployment;
        spec.validate()?;
        
        Ok(spec)
    }

    /// Validates deployment specification.
    pub fn validate(&self) -> ArcellaResult<()> {
        if self.module_id.is_empty() {
            return Err(ArcellaError::Manifest("Module ID must not be empty".into()));
        }

        if self.group.is_empty() {
            return Err(ArcellaError::Manifest("Group must not be empty".into()));
        }

        if self.replicas == 0 {
            return Err(ArcellaError::Manifest("Replicas must be at least 1".into()));
        }

        /// Validates that module_id follows `name@version` format.
        /// Does NOT check if the component actually exists in storage.
        if !validate_module_id(&self.module_id) {
            return Err(ArcellaError::Manifest(
                "Module ID must follow name@version format".into()
            ));
        }

        Ok(())
    }

     /// Creates a full deployment by combining template and overrides.
    ///
    /// The `group` always comes from the deployment spec (not the template).
    /// If no template is provided, safe defaults are used.
    pub fn create_deployment(
        &self,
        template: Option<&DeploymentTemplate>,
    ) -> ArcellaResult<FullDeployment> {
        // Use provided template or fall back to safe defaults
        let base = template.cloned().unwrap_or_default();

        // Apply overrides
        let isolation = self.overrides.isolation.clone().unwrap_or(base.isolation);
        let trusted = self.overrides.trusted.unwrap_or(base.trusted);
        let r#async = self.overrides.r#async.unwrap_or(base.r#async);
        let startup = self.overrides.startup.clone().unwrap_or(base.startup);
        let resources = self.overrides.resources.clone().unwrap_or(base.resources);

        let deployment = FullDeployment {
            module_id: self.module_id.clone(),
            group: self.group.clone(), // Always from spec, never from template
            replicas: self.replicas,
            isolation,
            trusted,
            r#async,
            startup,
            resources,
        };

        deployment.validate()?;
        Ok(deployment)
    }
}

/// Optional overrides for deployment template parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeploymentOverrides {
    /// Override isolation mode
    pub isolation: Option<IsolationMode>,
    
    /// Override trust level
    pub trusted: Option<bool>,
    
    /// Override async mode
    #[serde(rename = "async")]
    pub r#async: Option<bool>,
    
    /// Override startup configuration
    pub startup: Option<StartupConfig>,
    
    /// Override resource requirements
    pub resources: Option<ResourceRequirements>,
}

/// Complete deployment configuration ready for execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullDeployment {
    pub module_id: String,
    pub group: String,
    pub replicas: u32,
    pub isolation: IsolationMode,
    pub trusted: bool,
    pub r#async: bool,
    pub startup: StartupConfig,
    pub resources: ResourceRequirements,
}

impl FullDeployment {
    pub fn validate(&self) -> ArcellaResult<()> {
        validate_isolation_constraints(
            &self.isolation,
            self.trusted,
            self.r#async
        )?;

        if self.isolation == IsolationMode::Main && self.replicas != 1 {
            return Err(ArcellaError::Manifest("Main isolation supports only 1 replica".into()));
        }

        Ok(())
    }
}

/// Wrapper to match TOML structure: `[deployment]`
#[derive(Deserialize)]
struct DeploymentSpecWrapper {
    deployment: DeploymentSpec,
}

// ========================
// 4. SHARED TYPES
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

    /// Timeout in seconds for startup (0 = no timeout)
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

/// Resource requirements and limits
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceRequirements {
    /// Maximum memory in MB
    #[serde(default)]
    pub memory_mb: Option<u32>,
    
    /// Maximum fuel units
    #[serde(default)]
    pub fuel: Option<u64>,
    
    /// CPU shares (relative weight)
    #[serde(default)]
    pub cpu_shares: Option<u32>,
}

impl ResourceRequirements {
    pub fn validate(&self) -> ArcellaResult<()> {
        if let Some(mem) = self.memory_mb {
            if mem == 0 {
                return Err(ArcellaError::Manifest("Memory must be at least 1 MB".into()));
            }
        }
        if let Some(fuel) = self.fuel {
            if fuel == 0 {
                return Err(ArcellaError::Manifest("Fuel must be at least 1".into()));
            }
        }
        Ok(())
    }
}

// ========================
// 5. BUNDLE MANAGEMENT
// ========================

/// Represents a complete component bundle with all manifests
#[derive(Debug, Clone)]
pub struct ComponentBundle {
    pub component: ComponentManifest,
    pub template: Option<DeploymentTemplate>,
    pub wasm_path: PathBuf,
}

impl ComponentBundle {
    /// Loads a complete component bundle from a directory
    pub fn from_wasm_path(engine: &Engine, wasm_path: &Path) -> ArcellaResult<Self> {

        let component = if let Some(manifest) = ComponentManifest::from_component_toml(wasm_path)? {
            manifest
        } else {
            // TODO: в будущем — извлечение из .wasm
            ComponentManifest::from_wasm(engine, wasm_path)?
        };
                
        let template = DeploymentTemplate::from_template_toml(wasm_path)?;

        Ok(Self {
            component,
            template,
            wasm_path: wasm_path.to_path_buf(),
        })
    }

    /// Validates the entire bundle for consistency
    pub fn validate(&self) -> ArcellaResult<()> {
        self.component.validate()?;
        
        if let Some(template) = &self.template {
            template.validate()?;
            validate_compatibility(&self.component, template)?;
        }

        Ok(())
    }
}

// ========================
// 6. VALIDATION HELPERS
// ========================

/// Validates compatibility between a component and its deployment template
pub fn validate_compatibility(
    component: &ComponentManifest,
    deployment: &DeploymentTemplate,
) -> ArcellaResult<()> {

    // Check if async component is deployed in sync mode
    if !component.exports.is_empty() && !deployment.r#async {
        return Err(ArcellaError::Manifest("Component exports but deployment is sync".into()));
    }

    Ok(())
}

pub fn validate_module_id(id: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^[a-zA-Z0-9_-]+@\d+\.\d+\.\d+([+-][a-zA-Z0-9.-]+)?$").unwrap()
    });
    re.is_match(id)
}

fn validate_isolation_constraints(
    isolation: &IsolationMode,
    trusted: bool,
    r#async: bool,
) -> ArcellaResult<()> {
    if trusted && *isolation != IsolationMode::Main {
        return Err(ArcellaError::Manifest("Only 'main' isolation can be trusted".into()));
    }
    if *isolation == IsolationMode::Main && !r#async {
        return Err(ArcellaError::Manifest("'main' isolation requires async = true".into()));
    }
    Ok(())
}

// ========================
// 7. TESTS
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
            name = "test-component"
            version = "0.1.0"
            exports = ["foo:bar@1.0", "wasi:http/incoming-handler@0.2.0"]
            imports = ["wasi:cli@0.2.0"]
        "#;
        let wrapper: ComponentManifestWrapper = toml::from_str(toml).unwrap();
        assert!(wrapper.component.validate().is_ok());
        assert_eq!(wrapper.component.id(), "test-component@0.1.0");
    }

    #[test]
    fn test_deployment_spec_validation() {
        let spec = DeploymentSpec {
            module_id: "test@1.0.0".to_string(),
            group: "web".to_string(),
            replicas: 3,
            overrides: DeploymentOverrides::default(),
        };
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn test_full_deployment_creation() {
        let template = DeploymentTemplate {
            isolation: IsolationMode::Worker,
            trusted: false,
            r#async: true,
            group: Some("default".to_string()),
            startup: StartupConfig::default(),
            resources: ResourceRequirements::default(),
        };

        let spec = DeploymentSpec {
            module_id: "test@1.0.0".to_string(),
            group: "web".to_string(),
            replicas: 5,
            overrides: DeploymentOverrides::default(),
        };

        let deployment = spec.create_deployment(Some(&template)).unwrap();
        assert_eq!(deployment.group, "web");
        assert_eq!(deployment.replicas, 5);
        assert_eq!(deployment.isolation, IsolationMode::Worker);
    }

    #[test]
    fn test_main_isolation_constraints() {
        let deployment = FullDeployment {
            module_id: "test@1.0.0".to_string(),
            group: "main".to_string(),
            replicas: 2, // This should fail validation
            isolation: IsolationMode::Main,
            trusted: true,
            r#async: true,
            startup: StartupConfig::default(),
            resources: ResourceRequirements::default(),
        };

        assert!(deployment.validate().is_err());
    }

    #[test]
    fn test_invalid_component_name() {
        let toml = r#"
            [component]
            name = "invalid name!"
            version = "0.1.0"
        "#;
        let wrapper: Result<ComponentManifestWrapper, _> = toml::from_str(toml);
        assert!(wrapper.is_ok());
        assert!(wrapper.unwrap().component.validate().is_err());
    }    
    
}