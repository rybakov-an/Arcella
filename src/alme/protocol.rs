use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct AlmeRequest {
    /// Command name in hierarchical format, e.g., "log:tail", "module:status"
    pub cmd: String,

    /// Optional arguments for the command
    #[serde(default)]
    pub args: serde_json::Value,
}

/// ALME server response format.
#[derive(Serialize, Deserialize, Debug)]
pub struct AlmeResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl AlmeResponse {
    pub fn success(message: &str, data: Option<serde_json::Value>) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data,
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            message: message.to_string(),
            data: None,
        }
    }
}
