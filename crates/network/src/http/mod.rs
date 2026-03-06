//! A high-performance HTTP client implementation.

pub mod client;
pub mod error;
pub mod types;

// Re-exports
pub use client::{HttpClient, InnerHttpClient};
pub use error::HttpClientError;
pub use reqwest::{Error as ReqwestError, Method, Response, StatusCode, Url, header::USER_AGENT};
pub use types::{HttpMethod, HttpResponse, HttpStatus};
