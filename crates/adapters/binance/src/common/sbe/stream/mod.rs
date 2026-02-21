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

// SBE stream decoders - all methods return StreamDecodeError on decode failure
#![allow(clippy::missing_errors_doc)]

//! Binance SBE market data stream decoders (schema 1:0).
//!
//! These decoders are hand-written for the 4 market data stream message types:
//! - [`TradesStreamEvent`] - Real-time trade data
//! - [`BestBidAskStreamEvent`] - Best bid/ask (BBO) updates
//! - [`DepthSnapshotStreamEvent`] - Order book snapshots (top N levels)
//! - [`DepthDiffStreamEvent`] - Incremental order book updates
//!
//! All decoders return `Result<T, StreamDecodeError>` to safely handle malformed
//! or truncated network data without panicking.

use std::fmt::Display;

use crate::common::sbe::{cursor::SbeCursor, error::SbeDecodeError};

mod best_bid_ask;
mod depth_diff;
mod depth_snapshot;
mod trades;

pub use best_bid_ask::BestBidAskStreamEvent;
pub use depth_diff::DepthDiffStreamEvent;
pub use depth_snapshot::DepthSnapshotStreamEvent;
pub use trades::{Trade, TradesStreamEvent};

/// Stream schema ID (from stream_1_0.xml).
pub const STREAM_SCHEMA_ID: u16 = 1;

/// Stream schema version.
pub const STREAM_SCHEMA_VERSION: u16 = 0;

/// Maximum allowed group size to prevent OOM from malicious payloads.
/// Binance depth streams typically have at most 5000 levels.
pub const MAX_GROUP_SIZE: usize = 10_000;

/// Message template IDs for stream events.
pub mod template_id {
    pub const TRADES_STREAM_EVENT: u16 = 10000;
    pub const BEST_BID_ASK_STREAM_EVENT: u16 = 10001;
    pub const DEPTH_SNAPSHOT_STREAM_EVENT: u16 = 10002;
    pub const DEPTH_DIFF_STREAM_EVENT: u16 = 10003;
}

/// Stream decode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDecodeError {
    /// Buffer too short to decode expected data.
    BufferTooShort { expected: usize, actual: usize },
    /// Group count exceeds safety limit.
    GroupSizeTooLarge { count: usize, max: usize },
    /// Invalid UTF-8 in symbol string.
    InvalidUtf8,
    /// Schema ID mismatch.
    SchemaMismatch { expected: u16, actual: u16 },
    /// Unknown template ID.
    UnknownTemplateId(u16),
}

impl Display for StreamDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooShort { expected, actual } => {
                write!(
                    f,
                    "Buffer too short: expected {expected} bytes, was {actual}"
                )
            }
            Self::GroupSizeTooLarge { count, max } => {
                write!(f, "Group size {count} exceeds maximum {max}")
            }
            Self::InvalidUtf8 => write!(f, "Invalid UTF-8 in symbol"),
            Self::SchemaMismatch { expected, actual } => {
                write!(f, "Schema mismatch: expected {expected}, was {actual}")
            }
            Self::UnknownTemplateId(id) => write!(f, "Unknown template ID: {id}"),
        }
    }
}

impl std::error::Error for StreamDecodeError {}

impl From<SbeDecodeError> for StreamDecodeError {
    fn from(err: SbeDecodeError) -> Self {
        match err {
            SbeDecodeError::BufferTooShort { expected, actual } => {
                Self::BufferTooShort { expected, actual }
            }
            SbeDecodeError::SchemaMismatch { expected, actual } => {
                Self::SchemaMismatch { expected, actual }
            }
            SbeDecodeError::VersionMismatch { .. } => Self::SchemaMismatch {
                expected: STREAM_SCHEMA_VERSION,
                actual: 0,
            },
            SbeDecodeError::UnknownTemplateId(id) => Self::UnknownTemplateId(id),
            SbeDecodeError::GroupSizeTooLarge { count, max } => Self::GroupSizeTooLarge {
                count: count as usize,
                max: max as usize,
            },
            SbeDecodeError::InvalidBlockLength { .. } => Self::BufferTooShort {
                expected: 0,
                actual: 0,
            },
            SbeDecodeError::InvalidUtf8 => Self::InvalidUtf8,
        }
    }
}

/// SBE message header (8 bytes).
#[derive(Debug, Clone, Copy)]
pub struct MessageHeader {
    pub block_length: u16,
    pub template_id: u16,
    pub schema_id: u16,
    pub version: u16,
}

impl MessageHeader {
    pub const ENCODED_LENGTH: usize = 8;

    /// Decode message header from buffer.
    ///
    /// # Errors
    ///
    /// Returns error if buffer is less than 8 bytes.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        if buf.len() < Self::ENCODED_LENGTH {
            return Err(StreamDecodeError::BufferTooShort {
                expected: Self::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }
        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            template_id: u16::from_le_bytes([buf[2], buf[3]]),
            schema_id: u16::from_le_bytes([buf[4], buf[5]]),
            version: u16::from_le_bytes([buf[6], buf[7]]),
        })
    }

    /// Validate schema ID matches expected stream schema.
    pub fn validate_schema(&self) -> Result<(), StreamDecodeError> {
        if self.schema_id != STREAM_SCHEMA_ID {
            return Err(StreamDecodeError::SchemaMismatch {
                expected: STREAM_SCHEMA_ID,
                actual: self.schema_id,
            });
        }
        Ok(())
    }
}

/// Price/quantity level in order book.
#[derive(Debug, Clone, Copy)]
pub struct PriceLevel {
    /// Price mantissa (multiply by 10^exponent to get actual price).
    pub price_mantissa: i64,
    /// Quantity mantissa (multiply by 10^exponent to get actual quantity).
    pub qty_mantissa: i64,
}

impl PriceLevel {
    pub const ENCODED_LENGTH: usize = 16;

    /// Decode price level from cursor.
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short.
    pub fn decode(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        Ok(Self {
            price_mantissa: cursor.read_i64_le()?,
            qty_mantissa: cursor.read_i64_le()?,
        })
    }
}

/// Convert mantissa and exponent to f64.
#[inline]
#[must_use]
pub fn mantissa_to_f64(mantissa: i64, exponent: i8) -> f64 {
    mantissa as f64 * 10_f64.powi(exponent as i32)
}

/// Decode a varString8 (1-byte length prefix + UTF-8 data).
///
/// Returns (string, bytes_consumed) on success.
///
/// # Errors
///
/// Returns error if buffer is too short or contains invalid UTF-8.
pub fn decode_var_string8(buf: &[u8]) -> Result<(String, usize), StreamDecodeError> {
    if buf.is_empty() {
        return Err(StreamDecodeError::BufferTooShort {
            expected: 1,
            actual: 0,
        });
    }

    let len = buf[0] as usize;
    let total_len = 1 + len;

    if buf.len() < total_len {
        return Err(StreamDecodeError::BufferTooShort {
            expected: total_len,
            actual: buf.len(),
        });
    }

    let s = std::str::from_utf8(&buf[1..total_len]).map_err(|_| StreamDecodeError::InvalidUtf8)?;

    Ok((s.to_string(), total_len))
}

/// Group size encoding (6 bytes: u16 block_length + u32 num_in_group).
#[derive(Debug, Clone, Copy)]
pub struct GroupSizeEncoding {
    pub block_length: u16,
    pub num_in_group: u32,
}

impl GroupSizeEncoding {
    pub const ENCODED_LENGTH: usize = 6;

    /// Decode group size encoding from buffer.
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short or group count exceeds safety limit.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        if buf.len() < Self::ENCODED_LENGTH {
            return Err(StreamDecodeError::BufferTooShort {
                expected: Self::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }

        let num_in_group = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

        if num_in_group as usize > MAX_GROUP_SIZE {
            return Err(StreamDecodeError::GroupSizeTooLarge {
                count: num_in_group as usize,
                max: MAX_GROUP_SIZE,
            });
        }

        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            num_in_group,
        })
    }
}

/// Group size 16 encoding (4 bytes: u16 block_length + u16 num_in_group).
#[derive(Debug, Clone, Copy)]
pub struct GroupSize16Encoding {
    pub block_length: u16,
    pub num_in_group: u16,
}

impl GroupSize16Encoding {
    pub const ENCODED_LENGTH: usize = 4;

    /// Decode group size 16 encoding from buffer.
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short or group count exceeds safety limit.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        if buf.len() < Self::ENCODED_LENGTH {
            return Err(StreamDecodeError::BufferTooShort {
                expected: Self::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }

        let num_in_group = u16::from_le_bytes([buf[2], buf[3]]);

        if num_in_group as usize > MAX_GROUP_SIZE {
            return Err(StreamDecodeError::GroupSizeTooLarge {
                count: num_in_group as usize,
                max: MAX_GROUP_SIZE,
            });
        }

        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            num_in_group,
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_mantissa_to_f64() {
        assert!((mantissa_to_f64(12345, -2) - 123.45).abs() < 1e-10);
        assert!((mantissa_to_f64(100, 0) - 100.0).abs() < 1e-10);
        assert!((mantissa_to_f64(5, 3) - 5000.0).abs() < 1e-10);
    }

    #[rstest]
    fn test_message_header_too_short() {
        let buf = [0u8; 7];
        let err = MessageHeader::decode(&buf).unwrap_err();
        assert_eq!(
            err,
            StreamDecodeError::BufferTooShort {
                expected: 8,
                actual: 7
            }
        );
    }

    #[rstest]
    fn test_group_size_too_large() {
        // Craft a buffer with num_in_group = MAX_GROUP_SIZE + 1
        let mut buf = [0u8; 6];
        let count = (MAX_GROUP_SIZE + 1) as u32;
        buf[2..6].copy_from_slice(&count.to_le_bytes());

        let err = GroupSizeEncoding::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::GroupSizeTooLarge { .. }));
    }

    #[rstest]
    fn test_decode_var_string8_empty_buffer() {
        let err = decode_var_string8(&[]).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_var_string8_truncated() {
        // Length says 10 bytes, but only 5 available
        let buf = [10u8, b'H', b'E', b'L', b'L'];
        let err = decode_var_string8(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_var_string8_valid() {
        let buf = [5u8, b'H', b'E', b'L', b'L', b'O'];
        let (s, consumed) = decode_var_string8(&buf).unwrap();
        assert_eq!(s, "HELLO");
        assert_eq!(consumed, 6);
    }

    #[rstest]
    fn test_schema_validation() {
        let header = MessageHeader {
            block_length: 50,
            template_id: 10001,
            schema_id: 99, // Wrong schema
            version: 0,
        };
        let err = header.validate_schema().unwrap_err();
        assert_eq!(
            err,
            StreamDecodeError::SchemaMismatch {
                expected: STREAM_SCHEMA_ID,
                actual: 99
            }
        );
    }
}
