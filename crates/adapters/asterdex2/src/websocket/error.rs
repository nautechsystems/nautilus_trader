use std::fmt;

#[derive(Debug)]
pub enum AsterdexWebSocketError {
    Connection(String),
    Subscription(String),
    ParseMessage(String),
    InvalidMessage(String),
}

impl fmt::Display for AsterdexWebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsterdexWebSocketError::Connection(e) => write!(f, "Connection error: {}", e),
            AsterdexWebSocketError::Subscription(e) => write!(f, "Subscription error: {}", e),
            AsterdexWebSocketError::ParseMessage(e) => write!(f, "Parse message error: {}", e),
            AsterdexWebSocketError::InvalidMessage(e) => write!(f, "Invalid message: {}", e),
        }
    }
}

impl std::error::Error for AsterdexWebSocketError {}
