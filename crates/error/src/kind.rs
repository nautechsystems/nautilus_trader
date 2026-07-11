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

use strum::{AsRefStr, Display, EnumString};

/// Classifies what kind of error occurred.
///
/// Callers can match on this to decide how to handle the error programmatically,
/// without inspecting string messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display, AsRefStr, EnumString)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ErrorKind {
    // -- General --
    /// An unexpected internal error that should not happen.
    Unexpected,

    /// The requested operation is not supported.
    Unsupported,

    /// Invalid configuration or parameters.
    InvalidConfig,

    /// Invalid input argument.
    InvalidArgument,

    /// A precondition or invariant was violated.
    PreconditionFailed,

    // -- Lookup / state --
    /// The requested entity was not found.
    NotFound,

    /// The entity already exists (duplicate).
    AlreadyExists,

    /// Permission denied or insufficient privileges.
    PermissionDenied,

    // -- Network / IO --
    /// A network or IO error.
    Io,

    /// Connection failed or was lost.
    ConnectionFailed,

    /// The operation timed out.
    Timeout,

    /// Rate limited by the remote service.
    RateLimited,

    // -- Execution / trading --
    /// Order was rejected by the venue or risk engine.
    OrderRejected,

    /// Order was denied by local risk checks.
    OrderDenied,

    /// Execution reconciliation failed.
    ReconciliationFailed,

    // -- Data --
    /// Data parsing or deserialization failed.
    ParseError,

    /// Data encoding or serialization failed.
    EncodeError,

    /// Invalid or corrupt data.
    DataCorrupted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_display() {
        assert_eq!(ErrorKind::NotFound.to_string(), "NOT_FOUND");
        assert_eq!(ErrorKind::Timeout.to_string(), "TIMEOUT");
        assert_eq!(ErrorKind::OrderRejected.to_string(), "ORDER_REJECTED");
    }

    #[test]
    fn error_kind_from_str() {
        assert_eq!(
            "NOT_FOUND".parse::<ErrorKind>().unwrap(),
            ErrorKind::NotFound
        );
    }
}
