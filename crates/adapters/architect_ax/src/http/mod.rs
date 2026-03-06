//! HTTP REST API client for Ax.
//!
//! This module provides a two-layer HTTP client architecture:
//! - Raw client: Low-level API methods matching venue endpoints
//! - Domain client: High-level methods using Nautilus types
//!
//! Features:
//! - Bearer token authentication
//! - Automatic retry with backoff
//! - Rate limit handling
//! - Request/response models
//! - Parsing to Nautilus domain types

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;
