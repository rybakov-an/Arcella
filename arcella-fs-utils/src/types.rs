// arcella/arcella-fs-utils/src/types.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use indexmap::IndexMap;
use std::collections::HashSet;
use std::path::PathBuf;

use arcella_types::config::Value as TomlValue;


// Неизменяемые параметры — можно клонировать свободно
#[derive(Debug, Clone)]
pub struct ConfigLoadParams {
    pub prefix: Vec<String>,
    pub config_dir: PathBuf,
}

// Изменяемое состояние — передаётся по &mut
pub struct ConfigLoadState {
    pub current_depth: usize,
    pub visited_paths: HashSet<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TomlFileData {
    pub includes: Vec<String>,
    pub values: IndexMap<String, (TomlValue, usize)>,
}

