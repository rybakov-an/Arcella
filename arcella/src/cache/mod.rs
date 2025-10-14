// arcella/arcella/src/cache/mod.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::Arc;

use crate::config::ArcellaConfig;
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct ModuleCache {
}

impl ModuleCache {
    pub async fn new(
        config: &Arc<ArcellaConfig>,
    ) -> ArcellaResult<Self> {
        Ok(Self {
        })
    }

}