// arcella/arcella-types/src/manifest/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::spec::ComponentItemSpec;

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
    #[serde(default, deserialize_with = "deserialize_interface_list")]
    pub exports: HashMap<String, ComponentItemSpec>,

    /// Tree of WIT interfaces this module **imports** from the environment.
    ///
    /// Format is identical to `exports`. These interfaces must be provided
    /// by the runtime or other modules at link time.
    #[serde(default, deserialize_with = "deserialize_interface_list")]
    pub imports: HashMap<String, ComponentItemSpec>,

    /// Component capabilities and requirements
    #[serde(default)]
    pub capabilities: ComponentCapabilities,

    // ... other metadata fields
}

impl ComponentManifest {

    /// Returns the canonical module identifier: `name@version`.
    ///
    /// This ID is used internally by the runtime to uniquely reference a module.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

    pub fn validate_module_id(id: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+@\d+\.\d+\.\d+([+-][a-zA-Z0-9.-]+)?$").unwrap()
        });
        re.is_match(id)
    }

    /// Validates component name format
    pub fn validate_name_format(name: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap()
        });
        re.is_match(name)
    }

    /// Validates version format (simplified semver)
    pub fn validate_version_format(version: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^\d+\.\d+\.\d+([+-][a-zA-Z0-9.-]+)?$").unwrap()
        });
        re.is_match(version)
    }

    /// Validates that a string matches the expected WIT interface format.
    pub fn validate_interface_format(s: &str) -> bool {
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

}

fn deserialize_interface_list<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ComponentItemSpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let interfaces: Vec<String> = Vec::deserialize(deserializer)?;
    let mut map = HashMap::new();
    for iface in interfaces {
        // For MVP: save as Unknown, because no WIT-parser
        // On future: deser on namespace/interface@version and create struct
        map.insert(iface.clone(), ComponentItemSpec::Unknown { debug: None });
    }
    Ok(map)
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

    /// CPU/memory limits (for resource control)
    #[serde(default)]
    pub resources: ComponentResources,

    /// Trusted execution environment (TEE) requirements
    #[serde(default)]
    pub security: ComponentSecurity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ComponentResources {
    pub memory_max: Option<u64>, // bytes
    pub cpu_shares: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ComponentSecurity {
    pub requires_tee: bool,
    pub allowed_syscalls: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_module_id() {
        assert!(ComponentManifest::validate_module_id("my-comp@1.0.0"));
        assert!(!ComponentManifest::validate_module_id("my comp@1.0.0")); // пробел
    }

}