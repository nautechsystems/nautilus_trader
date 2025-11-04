use std::fmt;

#[derive(Debug)]
pub enum AsterdexHttpError {
    HttpClient(String),
    ParseJson(String),
    ApiError { code: i32, msg: String },
    InvalidResponse(String),
}

impl fmt::Display for AsterdexHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsterdexHttpError::HttpClient(e) => write!(f, "HTTP client error: {}", e),
            AsterdexHttpError::ParseJson(e) => write!(f, "JSON parse error: {}", e),
            AsterdexHttpError::ApiError { code, msg } => {
                write!(f, "Asterdex API error [{}]: {}", code, msg)
            }
            AsterdexHttpError::InvalidResponse(e) => write!(f, "Invalid response: {}", e),
        }
    }
}

impl std::error::Error for AsterdexHttpError {}

impl From<serde_json::Error> for AsterdexHttpError {
    fn from(e: serde_json::Error) -> Self {
        AsterdexHttpError::ParseJson(e.to_string())
    }
}

// Asterdex API error response
#[derive(Debug, serde::Deserialize)]
pub struct AsterdexApiError {
    pub code: i32,
    pub msg: String,
}
