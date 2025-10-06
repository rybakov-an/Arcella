use std::sync::Arc;

use crate::{config};
use crate::error::{ArcellaError, Result as ArcellaResult};

pub struct ModuleCache {
}

impl ModuleCache {
    pub async fn new(
        config: &Arc<config::Config>,
    ) -> ArcellaResult<Self> {
        Ok(Self {
        })
    }

}