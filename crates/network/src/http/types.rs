// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! HTTP types including status codes, methods, and responses.

use std::{collections::HashMap, hash::Hash};

use bytes::Bytes;
use http::{StatusCode, status::InvalidStatusCode};
use reqwest::Method;

/// An HTTP status code.
///
/// Wraps [`http::StatusCode`] to reuse its validation and convenience methods.
#[derive(Clone, Debug)]
pub struct HttpStatus {
    inner: StatusCode,
}

impl HttpStatus {
    /// Creates an [`HttpStatus`] from a [`StatusCode`].
    #[must_use]
    pub const fn new(code: StatusCode) -> Self {
        Self { inner: code }
    }

    /// Returns the status code as a `u16` (e.g., `200` for OK).
    #[inline]
    #[must_use]
    pub const fn as_u16(&self) -> u16 {
        self.inner.as_u16()
    }

    /// Returns the three-digit ASCII representation of this status (e.g., `"200"`).
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    /// Checks if this status is in the 1xx (informational) range.
    #[inline]
    #[must_use]
    pub fn is_informational(&self) -> bool {
        self.inner.is_informational()
    }

    /// Checks if this status is in the 2xx (success) range.
    #[inline]
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.inner.is_success()
    }

    /// Checks if this status is in the 3xx (redirection) range.
    #[inline]
    #[must_use]
    pub fn is_redirection(&self) -> bool {
        self.inner.is_redirection()
    }

    /// Checks if this status is in the 4xx (client error) range.
    #[inline]
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        self.inner.is_client_error()
    }

    /// Checks if this status is in the 5xx (server error) range.
    #[inline]
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        self.inner.is_server_error()
    }
}

impl TryFrom<u16> for HttpStatus {
    type Error = InvalidStatusCode;

    /// Attempts to construct a [`HttpStatus`] from a `u16`.
    ///
    /// # Errors
    ///
    /// Returns an error if the code is not in the valid `100..999` range.
    fn try_from(code: u16) -> Result<Self, Self::Error> {
        Ok(Self {
            inner: StatusCode::from_u16(code)?,
        })
    }
}

/// An HTTP method supported by [`super::HttpClient`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}

impl From<HttpMethod> for Method {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::GET => Self::GET,
            HttpMethod::POST => Self::POST,
            HttpMethod::PUT => Self::PUT,
            HttpMethod::DELETE => Self::DELETE,
            HttpMethod::PATCH => Self::PATCH,
        }
    }
}

/// The status, selected headers, and raw body returned by an HTTP request.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: HttpStatus,
    /// The response headers as a map of key-value pairs.
    pub headers: HashMap<String, String>,
    /// The raw response body.
    pub body: Bytes,
}
