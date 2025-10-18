// arcella/arcella-lib/src/spec/manifest/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasmtime::{
    Engine,
    component::{
        types::{
            self,
            ComponentItem,
        },
    }
};

use crate::error::Result as ArcellaResult;

/// A serializable and inspectable representation of a WebAssembly Component Model item.
///
/// This enum captures the structure of component imports and exports in a way that can be
/// serialized to TOML/JSON, displayed in CLI output, or used for interface validation.
/// It abstracts over low-level `wasmtime::component::types::ComponentItem` to provide
/// a stable, human-readable format.
///
/// Note: This representation is intentionally lossy for MVP. Full WIT type fidelity
/// will be added in later versions using `wit-parser`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentItemSpec {
    /// A WebAssembly component function with named parameters and result types.
    #[serde(rename = "func")]
    ComponentFunc {
        /// List of `(parameter_name, type_name)` pairs.
        #[serde(default)]
        params: Vec<(String, String)>,

        /// List of result type names (empty for void functions).
        #[serde(default)]
        results: Vec<String>,
    },

    /// A core WebAssembly function (not part of the Component Model).
    ///
    /// Should generally not appear in valid components, but included for completeness.
    #[serde(rename = "core_func")]
    CoreFunc(String), // TODO: Registered type

    /// A core WebAssembly module embedded within a component.
    ///
    /// Represented as a placeholder string in MVP.
    #[serde(rename = "module")]
    Module(String), // TODO: Extern type

    /// A nested WebAssembly component.
    ///
    /// Contains its own imports and exports, forming a hierarchical structure.
    #[serde(rename = "component")]
    Component{
        /// Imports declared by the nested component.
        #[serde(default)]
        imports: HashMap<String, ComponentItemSpec>,

        /// Exports provided by the nested component.
        #[serde(default)]
        exports: HashMap<String, ComponentItemSpec>,
    },

    /// An instantiated component (e.g., a resolved instance like `wasi:cli/stdio`).
    ///
    /// Only contains exports, as instances are the result of linking.
    #[serde(rename = "instance")]
    ComponentInstance {
        /// The exported items of this instance.
        #[serde(default)]
        exports: HashMap<String, ComponentItemSpec>,
    },

    /// A user-defined type (record, variant, enum, flags, etc.).
    ///
    /// Represented as a placeholder string in MVP.
    #[serde(rename = "type_def")]
    Type (String),
    
    /// A resource handle (e.g., file descriptor, socket).
    ///
    /// Represented as a placeholder string in MVP.
    #[serde(rename = "resource")]
    Resource(String),

    /// A fallback for unrecognized or unrepresentable component items.
    ///
    /// Used to prevent parsing failures when encountering new or malformed items.
    #[serde(rename = "unknown")]
    Unknown{
        /// Optional debug information about the unrecognized item.
        #[serde(skip_serializing_if = "Option::is_none")]
        debug: Option<String>,
    },
}

impl ComponentItemSpec {
    /// Maximum recursion depth allowed when traversing nested component structures.
    ///
    /// Prevents stack overflows or infinite loops when processing malicious or deeply nested components.
    const MAX_DEPTH: usize = 32;

    /// Converts a `wasmtime::component::types::ComponentItem` into a serializable `ComponentItemSpec`.
    ///
    /// This is a best-effort, lossy conversion suitable for introspection and manifest generation.
    /// Full type fidelity requires integration with `wit-parser` (planned for v0.4+).
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine used to inspect component types.
    /// * `item` - The component item to convert.
    ///
    /// # Returns
    ///
    /// A `ComponentItemSpec` representing the item, or an `Unknown` variant on error.
    pub fn from_component_item(engine: &Engine, item: &ComponentItem) -> ArcellaResult<Self> {
        Self::from_component_item_with_depth(engine, item, 0)
    }

    /// Recursive helper for `from_component_item` that tracks nesting depth.
    ///
    /// Internal use only.
    pub fn from_component_item_with_depth(
        engine: &Engine, 
        item: &ComponentItem,
        depth: usize,
    ) -> ArcellaResult<Self> {
        if depth > Self::MAX_DEPTH {
            return Ok(Self::Unknown {
                debug: Some("Over MAX_DEPTH".into()),
            });
        }

        match item {
            ComponentItem::ComponentFunc(func_ty) => {
                let params = func_ty
                    .params()
                    .map(|(name, ty)| (name.into(), Self::type_to_string(&ty)) )
                    .collect();
                let results = func_ty
                    .results()
                    .map(|ty| Self::type_to_string(&ty) )
                    .collect();
                Ok(Self::ComponentFunc { params, results })
            },

            ComponentItem::CoreFunc(ty) => Ok(Self::CoreFunc(format!("{}", ty))),

            ComponentItem::Module(ty ) => Ok(Self::Module(format!("{:?}", ty))),

            ComponentItem::Component(comp_ty ) => {
                let imports = comp_ty
                    .imports(&engine)
                    .map(|(name, nested_item)| {
                        (
                            name.into(),
                            match Self::from_component_item_with_depth(engine, &nested_item, depth + 1)  {
                                Ok(item) => item,
                                // Best-effort parsing: skip malformed nested items
                                Err(e) => Self::Unknown {
                                    debug: Some(format!("Error: {:?}", e)),
                                },
                            },
                        )
                    })
                    .collect();
                let exports = comp_ty
                    .exports(&engine)
                    .map(|(name, nested_item)| {
                        (
                            name.into(),
                            match Self::from_component_item_with_depth(engine, &nested_item, depth + 1)  {
                                Ok(item) => item,
                                // Best-effort parsing: skip malformed nested items
                                Err(e) => Self::Unknown {
                                    debug: Some(format!("Error: {:?}", e)),
                                },
                            },
                        )
                    })
                    .collect();
                Ok(Self::Component { imports, exports })
            }

            ComponentItem::ComponentInstance(ty) => {
                let exports = ty
                    .exports(engine)
                    .map(|(name, nested_item)| {
                        (
                            name.into(),
                            match Self::from_component_item_with_depth(engine, &nested_item, depth + 1)  {
                                Ok(item) => item,
                                // Best-effort parsing: skip malformed nested items
                                Err(e) => Self::Unknown {
                                    debug: Some(format!("Error: {:?}", e)),
                                },
                            },
                        )
                    })
                    .collect();
                Ok(Self::ComponentInstance { exports })
            },

            ComponentItem::Type(ty ) => {
                // TODO(v0.4): Replace with WIT type name via `wit-parser` or canonical string
                Ok(Self::Type(format!("{:?}", ty)))
            },

            ComponentItem::Resource(ty ) => {
                // TODO(v0.4): Replace with WIT type name via `wit-parser` or canonical string
                Ok(Self::Resource(format!("{:?}", ty)))
            },

        }

    }

    fn type_to_string(ty: &types::Type) -> String {
        match ty {
            types::Type::Bool => "bool".into(),
            types::Type::S8 => "s8".into(),
            types::Type::U8 => "u8".into(),
            types::Type::S16 => "s16".into(),
            types::Type::U16 => "u16".into(),
            types::Type::S32 => "s32".into(),
            types::Type::U32 => "u32".into(),
            types::Type::S64 => "s64".into(),
            types::Type::U64 => "u64".into(),
            types::Type::Float32 => "f32".into(),
            types::Type::Float64 => "f64".into(),
            types::Type::Char => "char".into(),
            types::Type::String => "string".into(),
            _ => format!("unknown({:?})", ty),
        }
    }    
}

impl std::fmt::Display for ComponentItemSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentFunc { params, results } => {
                write!(f, "func(")?;
                for (i, (name, ty)) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", name, ty)?;
                }
                write!(f, ")")?;
                if !results.is_empty() {
                    write!(f, " -> ")?;
                    for (i, ty) in results.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", ty)?;
                    }
                }
                Ok(())
            }
            Self::ComponentInstance { .. } => write!(f, "instance"),
            Self::Component { .. } => write!(f, "component"),
            Self::Module(_) => write!(f, "module"),
            Self::CoreFunc(_) => write!(f, "core-func"),
            Self::Type(t) => write!(f, "type({})", t),
            Self::Resource(r) => write!(f, "resource({})", r),
            Self::Unknown { debug: Some(d) } => write!(f, "unknown({})", d),
            Self::Unknown { debug: None } => write!(f, "unknown"),
        }
    }
}

/// Flattens a hierarchical component item map into a flat map with dot-separated keys.
///
/// This transformation is useful for:
/// - Displaying component interfaces in CLI (`arcella list --exports`)
/// - Generating flat dependency lists
/// - Simplifying manifest validation
///
/// # Example
///
/// Input:
/// ```text
/// {
///   "logger": ComponentInstance {
///     exports: { "log": ComponentFunc(...) }
///   }
/// }
/// ```
///
/// Output:
/// ```text
/// {
///   "logger": ComponentInstance(...),
///   "logger.log": ComponentFunc(...)
/// }
/// ```
pub fn flatten_component_tree(
    tree: &HashMap<String, ComponentItemSpec>,
) -> HashMap<String, ComponentItemSpec> {
    let mut flat = HashMap::new();
    flatten_component_tree_recursive(tree, "", &mut flat);
    flat
}

/// Recursive helper for `flatten_component_tree`.
///
/// Internal use only.
fn flatten_component_tree_recursive(
    tree: &HashMap<String, ComponentItemSpec>,
    prefix: &str,
    output: &mut HashMap<String, ComponentItemSpec>,
) {
    for (name, item) in tree {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };

        // Insert the current node
        output.insert(key.clone(), item.clone());

        // Recurse into nested structures
        match item {
            ComponentItemSpec::ComponentInstance { exports } => {
                flatten_component_tree_recursive(exports, &key, output);
            }
            ComponentItemSpec::Component { imports: _, exports } => {
                // For components, we flatten both imports and exports under the same key?
                // But imports are usually not nested in exports.
                // For now, flatten only exports (imports are top-level in practice).
                flatten_component_tree_recursive(exports, &key, output);
                // Optionally: flatten imports under "key.imports.*" — but likely unnecessary.
            }
            _ => {
                // Leaf node — nothing to recurse into
            }
        }
    }
}