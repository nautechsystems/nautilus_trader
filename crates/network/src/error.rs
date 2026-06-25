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

//! Network error types.

use std::{fmt::Display, io};

use thiserror::Error;

/// Error type for send operations in network clients.
#[derive(Error, Debug)]
pub enum SendError {
    /// The client has been closed or is disconnecting.
    #[error("send failed: client closed or disconnecting")]
    Closed,
    /// Timed out waiting for the client to become active.
    #[error("send failed: timeout waiting for active state")]
    Timeout,
    /// Failed to send because the writer channel is closed.
    #[error("send failed: broken pipe ({0})")]
    BrokenPipe(String),
}

/// Result type for client configuration validation.
pub type NetworkConfigResult<T> = Result<T, NetworkConfigError>;

/// A validation error for a network client configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkConfigError {
    /// A field value is empty or outside its accepted range.
    Invalid { field: String, reason: String },
    /// Multiple validation errors were collected.
    Multiple { errors: Vec<Self> },
}

impl NetworkConfigError {
    /// Creates a [`NetworkConfigError::Invalid`] for `field` with the given `reason`.
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Converts collected errors into a single result.
    ///
    /// Returns `Ok(())` when `errors` is empty, the sole error when one was collected, or a
    /// [`NetworkConfigError::Multiple`] otherwise.
    pub(crate) fn collect(mut errors: Vec<Self>) -> NetworkConfigResult<()> {
        match errors.len() {
            0 => Ok(()),
            1 => Err(errors.remove(0)),
            _ => Err(Self::Multiple { errors }),
        }
    }
}

impl Display for NetworkConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::Multiple { errors } => {
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for NetworkConfigError {}

pub(crate) fn is_connection_drop_io_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(io::ErrorKind::BrokenPipe, true)]
    #[case(io::ErrorKind::ConnectionAborted, true)]
    #[case(io::ErrorKind::ConnectionReset, true)]
    #[case(io::ErrorKind::NotConnected, true)]
    #[case(io::ErrorKind::TimedOut, true)]
    #[case(io::ErrorKind::UnexpectedEof, true)]
    #[case(io::ErrorKind::InvalidInput, false)]
    #[case(io::ErrorKind::PermissionDenied, false)]
    fn connection_drop_io_error_classification(
        #[case] kind: io::ErrorKind,
        #[case] expected: bool,
    ) {
        let err = io::Error::from(kind);

        assert_eq!(is_connection_drop_io_error(&err), expected);
    }

    #[rstest]
    fn test_invalid_display() {
        let err = NetworkConfigError::invalid("url", "must not be empty");

        assert_eq!(err.to_string(), "invalid url: must not be empty");
    }

    #[rstest]
    fn test_multiple_display_joins_errors() {
        let err = NetworkConfigError::Multiple {
            errors: vec![
                NetworkConfigError::invalid("url", "must not be empty"),
                NetworkConfigError::invalid("idle_timeout_ms", "must be positive, was 0"),
            ],
        };

        assert_eq!(
            err.to_string(),
            "invalid url: must not be empty; invalid idle_timeout_ms: must be positive, was 0"
        );
    }

    #[rstest]
    fn test_collect_returns_bare_error_for_single() {
        let errors = vec![NetworkConfigError::invalid("url", "must not be empty")];

        let result = NetworkConfigError::collect(errors);

        assert!(matches!(result, Err(NetworkConfigError::Invalid { field, .. }) if field == "url"));
    }
}
