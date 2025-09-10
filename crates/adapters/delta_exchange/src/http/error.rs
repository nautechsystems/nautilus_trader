// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Defines the error structures and enumerations for the Delta Exchange integration.

use nautilus_network::http::HttpClientError;
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

/// Represents the JSON structure of a successful response from Delta Exchange API.
#[derive(Clone, Debug, Deserialize)]
pub struct DeltaExchangeSuccessResponse<T> {
    /// Indicates if the request was successful.
    pub success: bool,
    /// The response data.
    pub result: T,
    /// Optional metadata (pagination, etc.).
    pub meta: Option<serde_json::Value>,
}

/// Represents the JSON structure of an error response from Delta Exchange API.
#[derive(Clone, Debug, Deserialize)]
pub struct DeltaExchangeErrorResponse {
    /// Indicates if the request was successful (always false for errors).
    pub success: bool,
    /// The error details.
    pub error: DeltaExchangeErrorDetails,
}

/// Contains the specific error details provided by the Delta Exchange API.
#[derive(Clone, Debug, Deserialize)]
pub struct DeltaExchangeErrorDetails {
    /// Error code identifier.
    pub code: String,
    /// Additional context for the error.
    pub context: Option<serde_json::Value>,
}

/// Internal error body structure for parsing various error formats.
#[derive(Deserialize)]
pub(crate) struct ErrorBody {
    pub message: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
}

/// A typed error enumeration for the Delta Exchange HTTP client.
#[derive(Debug, Error)]
pub enum DeltaExchangeHttpError {
    /// Error variant when credentials are missing but the request requires authentication.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,

    /// Error variant when credential signing fails.
    #[error("Credential signing error: {0}")]
    CredentialError(String),

    /// Errors returned directly by Delta Exchange API.
    #[error("Delta Exchange API error [{error_code}]: {message}")]
    ApiError { error_code: String, message: String },

    /// Authentication errors (401, 403).
    #[error("Authentication failed: {message}")]
    AuthenticationError { message: String },

    /// Rate limiting errors (429).
    #[error("Rate limit exceeded: {message}")]
    RateLimitError { message: String },

    /// Insufficient margin or balance errors.
    #[error("Insufficient margin: {message}")]
    InsufficientMargin { message: String },

    /// Order not found errors.
    #[error("Order not found: {message}")]
    OrderNotFound { message: String },

    /// Invalid parameter errors.
    #[error("Invalid parameter: {message}")]
    InvalidParameter { message: String },

    /// Market disruption errors.
    #[error("Market disrupted: {message}")]
    MarketDisrupted { message: String },

    /// Failure during JSON serialization/deserialization.
    #[error("JSON serialization error: {0}")]
    JsonError(String),

    /// URL encoding errors.
    #[error("URL encoding error: {0}")]
    UrlEncodingError(String),

    /// Underlying network or HTTP client error.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),

    /// Any unknown HTTP status or unexpected response from Delta Exchange.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },

    /// Request timeout errors.
    #[error("Request timeout: {message}")]
    Timeout { message: String },

    /// Connection errors.
    #[error("Connection error: {message}")]
    ConnectionError { message: String },
}

impl DeltaExchangeHttpError {
    /// Create an API error from Delta Exchange error response.
    pub fn from_api_error(error: DeltaExchangeErrorDetails) -> Self {
        let message = error.context
            .as_ref()
            .and_then(|ctx| ctx.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        match error.code.as_str() {
            "insufficient_margin" => Self::InsufficientMargin { message },
            "order_not_found" => Self::OrderNotFound { message },
            "invalid_parameter" => Self::InvalidParameter { message },
            "market_disrupted" => Self::MarketDisrupted { message },
            _ => Self::ApiError {
                error_code: error.code,
                message,
            },
        }
    }

    /// Create an error from HTTP status code and body.
    pub fn from_status_and_body(status: StatusCode, body: String) -> Self {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Self::AuthenticationError { message: body }
            }
            StatusCode::TOO_MANY_REQUESTS => {
                Self::RateLimitError { message: body }
            }
            StatusCode::REQUEST_TIMEOUT => {
                Self::Timeout { message: body }
            }
            _ => Self::UnexpectedStatus { status, body },
        }
    }

    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimitError { .. }
                | Self::Timeout { .. }
                | Self::ConnectionError { .. }
                | Self::HttpClientError(_)
                | Self::UnexpectedStatus { status, .. } if status.is_server_error()
        )
    }

    /// Check if the error is due to authentication issues.
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::MissingCredentials
                | Self::CredentialError(_)
                | Self::AuthenticationError { .. }
        )
    }

    /// Check if the error is due to rate limiting.
    pub fn is_rate_limit_error(&self) -> bool {
        matches!(self, Self::RateLimitError { .. })
    }

    /// Get the error message for logging.
    pub fn message(&self) -> String {
        match self {
            Self::MissingCredentials => "Missing credentials".to_string(),
            Self::CredentialError(msg) => msg.clone(),
            Self::ApiError { message, .. } => message.clone(),
            Self::AuthenticationError { message } => message.clone(),
            Self::RateLimitError { message } => message.clone(),
            Self::InsufficientMargin { message } => message.clone(),
            Self::OrderNotFound { message } => message.clone(),
            Self::InvalidParameter { message } => message.clone(),
            Self::MarketDisrupted { message } => message.clone(),
            Self::JsonError(msg) => msg.clone(),
            Self::UrlEncodingError(msg) => msg.clone(),
            Self::HttpClientError(err) => err.to_string(),
            Self::UnexpectedStatus { body, .. } => body.clone(),
            Self::Timeout { message } => message.clone(),
            Self::ConnectionError { message } => message.clone(),
        }
    }
}

impl From<serde_json::Error> for DeltaExchangeHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<serde_urlencoded::ser::Error> for DeltaExchangeHttpError {
    fn from(error: serde_urlencoded::ser::Error) -> Self {
        Self::UrlEncodingError(error.to_string())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_api_error_creation() {
        let error_details = DeltaExchangeErrorDetails {
            code: "insufficient_margin".to_string(),
            context: Some(json!({"additional_margin_required": "100.50"})),
        };

        let error = DeltaExchangeHttpError::from_api_error(error_details);
        assert!(matches!(error, DeltaExchangeHttpError::InsufficientMargin { .. }));
    }

    #[test]
    fn test_status_error_creation() {
        let error = DeltaExchangeHttpError::from_status_and_body(
            StatusCode::UNAUTHORIZED,
            "Invalid API key".to_string(),
        );
        assert!(matches!(error, DeltaExchangeHttpError::AuthenticationError { .. }));
    }

    #[test]
    fn test_error_retryable() {
        let rate_limit_error = DeltaExchangeHttpError::RateLimitError {
            message: "Rate limit exceeded".to_string(),
        };
        assert!(rate_limit_error.is_retryable());

        let auth_error = DeltaExchangeHttpError::AuthenticationError {
            message: "Invalid credentials".to_string(),
        };
        assert!(!auth_error.is_retryable());
    }

    #[test]
    fn test_error_auth_check() {
        let auth_error = DeltaExchangeHttpError::MissingCredentials;
        assert!(auth_error.is_auth_error());

        let api_error = DeltaExchangeHttpError::ApiError {
            error_code: "some_error".to_string(),
            message: "Some error".to_string(),
        };
        assert!(!api_error.is_auth_error());
    }

    #[test]
    fn test_error_rate_limit_check() {
        let rate_limit_error = DeltaExchangeHttpError::RateLimitError {
            message: "Too many requests".to_string(),
        };
        assert!(rate_limit_error.is_rate_limit_error());

        let other_error = DeltaExchangeHttpError::InvalidParameter {
            message: "Invalid param".to_string(),
        };
        assert!(!other_error.is_rate_limit_error());
    }
}
