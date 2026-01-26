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

//! Unified SBE decode error type for Binance adapters.

use std::fmt::Display;

/// Maximum allowed group size to prevent DoS from malformed data.
pub const MAX_GROUP_SIZE: u32 = 10_000;

/// SBE decode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SbeDecodeError {
    /// Buffer too short to decode expected data.
    BufferTooShort {
        /// Expected minimum bytes.
        expected: usize,
        /// Actual bytes available.
        actual: usize,
    },
    /// Schema ID mismatch.
    SchemaMismatch {
        /// Expected schema ID.
        expected: u16,
        /// Actual schema ID.
        actual: u16,
    },
    /// Schema version mismatch.
    VersionMismatch {
        /// Expected schema version.
        expected: u16,
        /// Actual schema version.
        actual: u16,
    },
    /// Unknown template ID.
    UnknownTemplateId(u16),
    /// Group count exceeds safety limit.
    GroupSizeTooLarge {
        /// Actual count.
        count: u32,
        /// Maximum allowed.
        max: u32,
    },
    /// Invalid block length.
    InvalidBlockLength {
        /// Expected block length.
        expected: u16,
        /// Actual block length.
        actual: u16,
    },
    /// Invalid UTF-8 in string field.
    InvalidUtf8,
}

impl Display for SbeDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooShort { expected, actual } => {
                write!(
                    f,
                    "Buffer too short: expected {expected} bytes, was {actual}"
                )
            }
            Self::SchemaMismatch { expected, actual } => {
                write!(f, "Schema ID mismatch: expected {expected}, was {actual}")
            }
            Self::VersionMismatch { expected, actual } => {
                write!(
                    f,
                    "Schema version mismatch: expected {expected}, was {actual}"
                )
            }
            Self::UnknownTemplateId(id) => write!(f, "Unknown template ID: {id}"),
            Self::GroupSizeTooLarge { count, max } => {
                write!(f, "Group size {count} exceeds maximum {max}")
            }
            Self::InvalidBlockLength { expected, actual } => {
                write!(f, "Invalid block length: expected {expected}, was {actual}")
            }
            Self::InvalidUtf8 => write!(f, "Invalid UTF-8 in string field"),
        }
    }
}

impl std::error::Error for SbeDecodeError {}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_buffer_too_short_display() {
        let err = SbeDecodeError::BufferTooShort {
            expected: 100,
            actual: 50,
        };
        assert_eq!(
            err.to_string(),
            "Buffer too short: expected 100 bytes, was 50"
        );
    }

    #[rstest]
    fn test_schema_mismatch_display() {
        let err = SbeDecodeError::SchemaMismatch {
            expected: 3,
            actual: 1,
        };
        assert_eq!(err.to_string(), "Schema ID mismatch: expected 3, was 1");
    }

    #[rstest]
    fn test_group_size_too_large_display() {
        let err = SbeDecodeError::GroupSizeTooLarge {
            count: 50000,
            max: 10000,
        };
        assert_eq!(err.to_string(), "Group size 50000 exceeds maximum 10000");
    }

    #[rstest]
    fn test_error_equality() {
        let err1 = SbeDecodeError::InvalidUtf8;
        let err2 = SbeDecodeError::InvalidUtf8;
        assert_eq!(err1, err2);
    }
}
