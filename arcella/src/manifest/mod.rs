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
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use wasmtime::{
    Engine,
};

use arcella_types::{
    manifest::ComponentManifest
};
use arcella_wasmtime::{
    ComponentManifestExt,
    error::ArcellaWasmtimeError,
    manifest,
};

use crate::error::{ArcellaError, Result as ArcellaResult};

// ================================
// 1. COMPONENT MANIFEST (portable)
// ================================

/// Wrapper to match TOML structure: `[component]`
#[derive(Deserialize)]
struct ComponentManifestWrapper {
    component: ComponentManifest,
}

pub fn load_component_manifest_from_toml(path: &Path) -> ArcellaResult<Option<ComponentManifest>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| ArcellaError::IoWithPath { source: e, path: path.into() })?; 

    let wrapper: ComponentManifestWrapper = toml::from_str(&content)
        .map_err(|e| ArcellaWasmtimeError::Manifest(e.to_string()))?;

    let manifest = wrapper.component;
    manifest.validate()?;

    Ok(Some(manifest))                      
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
            .map_err(|e| ArcellaError::IoWithPath{source: e, path: path.into()})?;
        let wrapper: DeploymentTemplateWrapper =
            toml::from_str(&content).map_err(|e| ArcellaWasmtimeError::Manifest(e.to_string()))?;
        
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
            return Err(ArcellaWasmtimeError::Manifest(
                "Group can only be specified for worker isolation".into(),
            ).into());
        }

        if let Some(ref group) = self.group {
            if !ComponentManifest::validate_name_format(group) {
                return Err(ArcellaWasmtimeError::Manifest(
                    "Invalid group name format".into()
                ).into());
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
            .map_err(|e| ArcellaError::IoWithPath{source: e, path: path.into()})?;
        
        let wrapper: DeploymentSpecWrapper =
            toml::from_str(&content).map_err(|e| ArcellaWasmtimeError::Manifest(e.to_string()))?;
        
        let spec = wrapper.deployment;
        spec.validate()?;
        
        Ok(spec)
    }

    /// Validates deployment specification.
    pub fn validate(&self) -> ArcellaResult<()> {
        if self.module_id.is_empty() {
            return Err(ArcellaWasmtimeError::Manifest(
                "Module ID must not be empty".into()
            ).into());
        }

        if self.group.is_empty() {
            return Err(ArcellaWasmtimeError::Manifest(
                "Group must not be empty".into()
            ).into());
        }

        if self.replicas == 0 {
            return Err(ArcellaWasmtimeError::Manifest(
                "Replicas must be at least 1".into()
            ).into());
        }

        if !validate_module_id(&self.module_id) {
            return Err(ArcellaWasmtimeError::Manifest(
                "Module ID must follow name@version format".into()
            ).into());
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
            return Err(ArcellaWasmtimeError::Manifest(
                "Main isolation supports only 1 replica".into()
            ).into());
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
                return Err(ArcellaWasmtimeError::Manifest(
                    "Memory must be at least 1 MB".into()
                ).into());
            }
        }
        if let Some(fuel) = self.fuel {
            if fuel == 0 {
                return Err(ArcellaWasmtimeError::Manifest(
                    "Fuel must be at least 1".into()
                ).into());
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

        let component = if let Some(manifest) = load_component_manifest_from_toml(
            &wasm_path.with_file_name("component.toml")
        )? {
            manifest
        } else {
            // 2. If component.toml is missing, try to extract from .wasm
            // (Requires arcella_wasmtime crate)
            manifest::component_manifest_from_wasm(engine, wasm_path)?
        };
                
        let template = DeploymentTemplate::from_template_toml(wasm_path)?;

        let bundle = Self {
            component,
            template,
            wasm_path: wasm_path.to_path_buf(),
        };

        bundle.validate()?;

        Ok(bundle)

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
        return Err(ArcellaWasmtimeError::Manifest(
            "Component exports but deployment is sync".into()
        ).into());
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
        return Err(ArcellaWasmtimeError::Manifest(
            "Only 'main' isolation can be trusted".into()
        ).into());
    }
    if *isolation == IsolationMode::Main && !r#async {
        return Err(ArcellaWasmtimeError::Manifest(
            "'main' isolation requires async = true".into()
        ).into());
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

    #[test]
    fn test_load_component_manifest_from_toml() {
        let temp_dir = TempDir::new().unwrap();
        let toml_path = temp_dir.path().join("component.toml");

        let toml_content = r#"
            [component]
            name = "test-component"
            version = "0.1.0"
            description = "A test component"
            exports = ["foo:bar@1.0"]
            imports = ["wasi:cli@0.2.0"]
        "#;

        fs::write(&toml_path, toml_content).unwrap();

        let result = load_component_manifest_from_toml(&toml_path).unwrap();
        assert!(result.is_some());

        let manifest = result.unwrap();
        assert_eq!(manifest.name, "test-component");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.description, Some("A test component".to_string()));
    }

    #[test]
    fn test_load_component_manifest_from_toml_not_found() {
        let fake_path = Path::new("/nonexistent/component.toml");
        let result = load_component_manifest_from_toml(fake_path).unwrap();
        assert!(result.is_none());
    }       
    
}