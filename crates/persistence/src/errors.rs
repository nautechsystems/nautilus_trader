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

//! Typed persistence errors.
//!
//! Most public APIs in this crate return [`anyhow::Result`] for ergonomic error chaining.
//! This module exposes a small thiserror enum with the variants callers most often need to
//! distinguish (e.g. "operation not supported" vs "operation failed"). Producers wrap the
//! enum in `anyhow::Error` and consumers downcast:
//!
//! ```ignore
//! use nautilus_persistence::errors::PersistenceError;
//!
//! if matches!(
//!     err.downcast_ref::<PersistenceError>(),
//!     Some(PersistenceError::Unsupported(_))
//! ) {
//!     // The concrete backend does not support the requested operation.
//! }
//! ```
//!
//! Migrating individual functions to return `Result<T, PersistenceError>` directly is a
//! follow-up; the downcast pattern lets new variants ship without breaking existing
//! `anyhow::Result` signatures.

use thiserror::Error;

/// Typed errors that callers may want to programmatically distinguish.
#[derive(Debug, Error)]
pub enum PersistenceError {
    /// The operation is not supported by this concrete backend.
    #[error("Operation not supported by this backend: {0}")]
    Unsupported(String),
}

impl PersistenceError {
    /// Convenience constructor for [`PersistenceError::Unsupported`].
    #[must_use]
    pub fn unsupported(operation: impl Into<String>) -> Self {
        Self::Unsupported(operation.into())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn unsupported_downcasts_through_anyhow() {
        let err: anyhow::Error =
            anyhow::Error::from(PersistenceError::unsupported("vacuum_catalog"));
        match err.downcast_ref::<PersistenceError>() {
            Some(PersistenceError::Unsupported(op)) => assert_eq!(op, "vacuum_catalog"),
            other => panic!("Expected Unsupported, received {other:?}"),
        }
    }
}
