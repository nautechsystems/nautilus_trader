use serde::{Deserialize, Serialize};

/// WebSocket subscribe message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexWsSubscribe {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}

impl AsterdexWsSubscribe {
    pub fn new(params: Vec<String>, id: u64) -> Self {
        Self {
            method: "SUBSCRIBE".to_string(),
            params,
            id,
        }
    }
}

/// WebSocket unsubscribe message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexWsUnsubscribe {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}

impl AsterdexWsUnsubscribe {
    pub fn new(params: Vec<String>, id: u64) -> Self {
        Self {
            method: "UNSUBSCRIBE".to_string(),
            params,
            id,
        }
    }
}

/// WebSocket response message
#[derive(Debug, Clone, Deserialize)]
pub struct AsterdexWsResponse {
    pub result: Option<serde_json::Value>,
    pub id: Option<u64>,
}

/// WebSocket stream message
#[derive(Debug, Clone, Deserialize)]
pub struct AsterdexWsStreamMessage {
    pub stream: Option<String>,
    pub data: serde_json::Value,
}
