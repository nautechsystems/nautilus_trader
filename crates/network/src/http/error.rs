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

//! HTTP client error types.

use std::error::Error;

/// Errors returned by the HTTP client.
///
/// Includes generic transport errors, timeouts, and proxy configuration errors.
#[derive(thiserror::Error, Debug)]
pub enum HttpClientError {
    #[error("HTTP error occurred: {0}")]
    Error(String),

    #[error("HTTP transport error: {0}")]
    TransportError(String),

    #[error("HTTP request timed out: {0}")]
    TimeoutError(String),

    #[error("Invalid proxy URL: {0}")]
    InvalidProxy(String),

    #[error("Failed to build HTTP client: {0}")]
    ClientBuildError(String),
}

impl From<String> for HttpClientError {
    fn from(value: String) -> Self {
        Self::Error(value)
    }
}

pub(super) fn transport_error(e: &(dyn Error + 'static)) -> HttpClientError {
    let mut message = String::new();
    let mut cause = Some(e);
    let mut timed_out = false;

    while let Some(e) = cause {
        if !message.is_empty() {
            message.push_str(": ");
        }
        message.push_str(&e.to_string());

        timed_out |= e
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_timeout)
            || e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::TimedOut);
        cause = e.source();
    }

    if timed_out {
        HttpClientError::TimeoutError(message)
    } else {
        HttpClientError::TransportError(message)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::timeout(io::ErrorKind::TimedOut, true)]
    #[case::refused(io::ErrorKind::ConnectionRefused, false)]
    fn socket_errors_preserve_classification(#[case] kind: io::ErrorKind, #[case] timeout: bool) {
        let error = transport_error(&io::Error::new(kind, "socket failure"));
        match (error, timeout) {
            (HttpClientError::TimeoutError(message), true)
            | (HttpClientError::TransportError(message), false) => {
                assert_eq!(message, "socket failure");
            }
            (error, _) => panic!("unexpected classification: {error}"),
        }
    }
}
