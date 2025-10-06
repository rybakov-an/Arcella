use serde::{Deserialize, Serialize};

/// Commands accepted by the ALME server.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "cmd", content = "args")]
pub enum AlmeRequest {
    Ping,
    Status,
    ListModules,
    // To be added in v0.7:
    // Install { path: String },
    // Start { id: String },
    // Stop { id: String },
}

/// ALME server response format.
#[derive(Serialize, Deserialize, Debug)]
pub struct AlmeResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}