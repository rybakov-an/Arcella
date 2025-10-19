// arcella/arcella-wasmtime/src/manifest.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path};
use std::sync::OnceLock;
use wasmtime::{
    Engine,
    component::{
        Component, 
    }
};

use arcella_types::{
    manifest::{ComponentManifest, ComponentCapabilities},
    spec::ComponentItemSpec,
};
use crate::ArcellaWasmtimeError;
use crate::Result;
use crate::from_wasmtime::{ComponentItemSpecExt, ComponentTypeExt};

pub trait ComponentManifestExt {

    fn validate(&self) -> Result<()>;

}

impl ComponentManifestExt for ComponentManifest {
    
    /// Validates semantic correctness of the component manifest.
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(ArcellaWasmtimeError::Manifest("Component name must not be empty".into()));
        }
        if self.version.is_empty() {
            return Err(ArcellaWasmtimeError::Manifest("Component version must not be empty".into()));
        }

        // Validate name format (alphanumeric, hyphens, underscores)
        if !ComponentManifest::validate_name_format(&self.name) {
            return Err(ArcellaWasmtimeError::Manifest(
                "Component name must contain only alphanumeric characters, hyphens, and underscores".into()
            ));
        }

        // Validate version format (semver-like)
        if !ComponentManifest::validate_version_format(&self.version) {
            return Err(ArcellaWasmtimeError::Manifest(
                "Component version must follow semantic versioning format (e.g., 0.1.0)".into()
                        ));
        }

        Ok(())
    }

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
pub fn component_manifest_from_wasm(engine: &Engine, wasm_path: &Path) -> Result<ComponentManifest> {

    if !wasm_path.exists() {
        return Err(ArcellaWasmtimeError::IoWithPath{
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {:?}", wasm_path)
            ),
            path: wasm_path.into(),
        });
    }

    let file_stem = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ArcellaWasmtimeError::Manifest("Invalid .wasm filename".into()))?;        

    if !ComponentManifest::validate_module_id(file_stem) {
        return Err(ArcellaWasmtimeError::Manifest(
            "Filename must be 'name@version.wasm' for components without component.toml".into()
        ));
    }

    let (name, version) = file_stem
        .split_once('@')
        .ok_or_else(|| ArcellaWasmtimeError::Manifest("Expected 'name@version' format".into()))?;

    let component = Component::from_file(engine, &wasm_path)
        .map_err(ArcellaWasmtimeError::Wasmtime)?;
    
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

    let manifest = ComponentManifest {
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
