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

use std::fmt;

use crate::{ErrorKind, ErrorStatus};

/// A structured error type for NautilusTrader.
///
/// Inspired by [OpenDAL's error design](https://xuanwo.io/en-us/reports/2022-46/),
/// this type carries:
///
/// - [`ErrorKind`] — what kind of error (for programmatic matching).
/// - [`ErrorStatus`] — whether the caller should retry.
/// - `operation` — which API call triggered the error.
/// - `context` — structured key-value pairs for debugging.
/// - `source` — the underlying cause (via `anyhow::Error`).
///
/// # Usage
///
/// ```
/// use nautilus_error::{NautilusError, ErrorKind};
///
/// fn get_order(id: &str) -> nautilus_error::Result<()> {
///     Err(NautilusError::new(ErrorKind::NotFound, "order not found")
///         .with_operation("Cache::get_order")
///         .with_context("client_order_id", id))
/// }
/// ```
pub struct NautilusError {
    kind: ErrorKind,
    message: String,
    status: ErrorStatus,
    operation: &'static str,
    context: Vec<(&'static str, String)>,
    source: Option<anyhow::Error>,
}

impl NautilusError {
    /// Creates a new error with the given kind and message.
    #[inline]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: ErrorStatus::default(),
            operation: "",
            context: Vec::new(),
            source: None,
        }
    }

    // -- Builders (chainable) --

    /// Sets the operation that triggered this error.
    ///
    /// If an operation was already set, the previous one is pushed
    /// into context as `"called"` to preserve the call chain.
    #[inline]
    #[must_use]
    pub fn with_operation(mut self, operation: &'static str) -> Self {
        if !self.operation.is_empty() {
            self.context.push(("called", self.operation.to_string()));
        }
        self.operation = operation;
        self
    }

    /// Adds a key-value context pair to the error.
    #[inline]
    #[must_use]
    pub fn with_context(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.context.push((key, value.into()));
        self
    }

    /// Sets the error status to temporary (retryable).
    #[inline]
    #[must_use]
    pub fn set_temporary(mut self) -> Self {
        self.status = ErrorStatus::Temporary;
        self
    }

    /// Sets the error status to persistent (was temporary, retries exhausted).
    #[inline]
    #[must_use]
    pub fn set_persistent(mut self) -> Self {
        self.status = ErrorStatus::Persistent;
        self
    }

    /// Sets the underlying source error.
    ///
    /// # Panics
    ///
    /// Debug-asserts that the source has not already been set.
    #[inline]
    #[must_use]
    pub fn set_source(mut self, src: impl Into<anyhow::Error>) -> Self {
        debug_assert!(self.source.is_none(), "source error has already been set");
        self.source = Some(src.into());
        self
    }

    // -- Accessors --

    /// Returns the error kind.
    #[inline]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the error status.
    #[inline]
    pub const fn status(&self) -> ErrorStatus {
        self.status
    }

    /// Returns the operation that triggered this error.
    #[inline]
    pub const fn operation(&self) -> &'static str {
        self.operation
    }

    /// Returns the structured context pairs.
    #[inline]
    pub fn context(&self) -> &[(&'static str, String)] {
        &self.context
    }

    /// Returns `true` if this error is retryable.
    #[inline]
    pub const fn is_retryable(&self) -> bool {
        self.status.is_retryable()
    }

    /// Returns the error message.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }
}

// -- Display: compact single-line format --
impl fmt::Display for NautilusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}", self.kind, self.status)?;

        if self.operation.is_empty() {
            write!(f, ")")?;
        } else {
            write!(f, ") at {}", self.operation)?;
        }

        if !self.context.is_empty() {
            write!(f, ", context: {{")?;
            for (i, (key, value)) in self.context.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{key}: {value}")?;
            }
            write!(f, "}}")?;
        }

        write!(f, " => {}", self.message)?;

        Ok(())
    }
}

// -- Debug: full multi-line format with source chain --
impl fmt::Debug for NautilusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} ({}) at {}", self.kind, self.status, self.operation)?;
        writeln!(f)?;
        writeln!(f, "    Message: {}", self.message)?;

        if !self.context.is_empty() {
            writeln!(f)?;
            writeln!(f, "    Context:")?;
            for (key, value) in &self.context {
                writeln!(f, "        {key}: {value}")?;
            }
        }

        if let Some(source) = &self.source {
            writeln!(f)?;
            writeln!(f, "    Source: {source:?}")?;
        }

        Ok(())
    }
}

// -- std::error::Error impl --
impl std::error::Error for NautilusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// -- Conversion from anyhow::Error (wraps as Unexpected) --
impl From<anyhow::Error> for NautilusError {
    fn from(err: anyhow::Error) -> Self {
        Self::new(ErrorKind::Unexpected, err.to_string()).set_source(err)
    }
}

// -- Conversion from std::io::Error --
impl From<std::io::Error> for NautilusError {
    fn from(err: std::io::Error) -> Self {
        let status = if err.kind() == std::io::ErrorKind::TimedOut
            || err.kind() == std::io::ErrorKind::ConnectionReset
            || err.kind() == std::io::ErrorKind::ConnectionRefused
        {
            ErrorStatus::Temporary
        } else {
            ErrorStatus::Permanent
        };

        Self {
            kind: ErrorKind::Io,
            message: err.to_string(),
            status,
            operation: "",
            context: Vec::new(),
            source: Some(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn basic_error() {
        let err = NautilusError::new(ErrorKind::NotFound, "order not found")
            .with_operation("Cache::get_order")
            .with_context("client_order_id", "O-001");

        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.status(), ErrorStatus::Permanent);
        assert!(!err.is_retryable());
        assert_eq!(err.operation(), "Cache::get_order");
        assert_eq!(err.context().len(), 1);
    }

    #[test]
    fn temporary_error() {
        let err = NautilusError::new(ErrorKind::ConnectionFailed, "connection reset")
            .set_temporary()
            .with_operation("WebSocketClient::connect")
            .with_context("url", "wss://ws.binance.com");

        assert!(err.is_retryable());
        assert_eq!(err.status(), ErrorStatus::Temporary);
    }

    #[test]
    fn operation_chain() {
        let err = NautilusError::new(ErrorKind::Timeout, "request timed out")
            .with_operation("HttpClient::send")
            .with_operation("BinanceAdapter::fetch_orders");

        // Previous operation pushed to context as "called"
        assert_eq!(err.operation(), "BinanceAdapter::fetch_orders");
        assert_eq!(err.context()[0], ("called", "HttpClient::send".to_string()));
    }

    #[test]
    fn display_compact() {
        let err = NautilusError::new(ErrorKind::NotFound, "order not found")
            .with_operation("Cache::get_order")
            .with_context("client_order_id", "O-001");

        let display = err.to_string();
        assert!(display.contains("NOT_FOUND"));
        assert!(display.contains("permanent"));
        assert!(display.contains("Cache::get_order"));
        assert!(display.contains("client_order_id: O-001"));
        assert!(display.contains("order not found"));
    }

    #[test]
    fn debug_multiline() {
        let err = NautilusError::new(ErrorKind::ConnectionFailed, "connection reset by peer")
            .set_temporary()
            .with_operation("WebSocketClient::connect")
            .with_context("url", "wss://ws.binance.com")
            .with_context("attempt", "3");

        let debug = format!("{err:?}");
        assert!(debug.contains("CONNECTION_FAILED"));
        assert!(debug.contains("temporary"));
        assert!(debug.contains("Message: connection reset by peer"));
        assert!(debug.contains("url: wss://ws.binance.com"));
        assert!(debug.contains("attempt: 3"));
    }

    #[test]
    fn with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = NautilusError::new(ErrorKind::ConnectionFailed, "cannot connect to venue")
            .set_temporary()
            .with_operation("ExecClient::connect")
            .set_source(io_err);

        assert!(err.source().is_some());
    }

    #[test]
    fn from_io_error_temporary() {
        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let err = NautilusError::from(io_err);

        assert_eq!(err.kind(), ErrorKind::Io);
        assert!(err.is_retryable());
    }

    #[test]
    fn from_io_error_permanent() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = NautilusError::from(io_err);

        assert_eq!(err.kind(), ErrorKind::Io);
        assert!(!err.is_retryable());
    }

    #[test]
    fn result_type_alias() {
        fn example() -> crate::Result<u32> {
            Err(NautilusError::new(ErrorKind::InvalidArgument, "bad input"))
        }

        let result = example();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidArgument);
    }

    #[test]
    fn std_error_trait() {
        let err: Box<dyn std::error::Error> =
            Box::new(NautilusError::new(ErrorKind::Unexpected, "something broke"));
        assert!(err.to_string().contains("something broke"));
    }
}
