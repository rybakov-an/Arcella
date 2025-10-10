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