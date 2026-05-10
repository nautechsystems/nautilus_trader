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

//! Generic SBE primitive decoders.

use std::str;

use super::{MAX_GROUP_SIZE, SbeDecodeError};

/// Group header encoding (u16 block length + u32 count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupSizeEncoding {
    /// Encoded block length of each group entry.
    pub block_length: u16,
    /// Number of entries in the group.
    pub num_in_group: u32,
}

impl GroupSizeEncoding {
    /// Encoded length in bytes.
    pub const ENCODED_LENGTH: usize = 6;

    /// Decodes a group header from `buf`.
    ///
    /// # Errors
    ///
    /// Returns `BufferTooShort` if fewer than 6 bytes are available and
    /// `GroupSizeTooLarge` when `num_in_group` exceeds [`MAX_GROUP_SIZE`].
    pub fn decode(buf: &[u8]) -> Result<Self, SbeDecodeError> {
        if buf.len() < Self::ENCODED_LENGTH {
            return Err(SbeDecodeError::BufferTooShort {
                expected: Self::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }

        let num_in_group = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);
        if num_in_group > MAX_GROUP_SIZE {
            return Err(SbeDecodeError::GroupSizeTooLarge {
                count: num_in_group,
                max: MAX_GROUP_SIZE,
            });
        }

        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            num_in_group,
        })
    }
}

/// Compact group header encoding (u16 block length + u16 count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupSize16Encoding {
    /// Encoded block length of each group entry.
    pub block_length: u16,
    /// Number of entries in the group.
    pub num_in_group: u16,
}

impl GroupSize16Encoding {
    /// Encoded length in bytes.
    pub const ENCODED_LENGTH: usize = 4;

    /// Decodes a compact group header from `buf`.
    ///
    /// # Errors
    ///
    /// Returns `BufferTooShort` if fewer than 4 bytes are available and
    /// `GroupSizeTooLarge` when `num_in_group` exceeds [`MAX_GROUP_SIZE`].
    pub fn decode(buf: &[u8]) -> Result<Self, SbeDecodeError> {
        if buf.len() < Self::ENCODED_LENGTH {
            return Err(SbeDecodeError::BufferTooShort {
                expected: Self::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }

        let num_in_group = u16::from_le_bytes([buf[2], buf[3]]);
        if u32::from(num_in_group) > MAX_GROUP_SIZE {
            return Err(SbeDecodeError::GroupSizeTooLarge {
                count: u32::from(num_in_group),
                max: MAX_GROUP_SIZE,
            });
        }

        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            num_in_group,
        })
    }
}

/// Decodes a varString8 field (u8 length + UTF-8 bytes).
///
/// Returns the decoded `&str` and number of bytes consumed.
///
/// # Errors
///
/// Returns `BufferTooShort` when the buffer does not contain the full field and
/// `InvalidUtf8` when the payload bytes are not valid UTF-8.
pub fn decode_var_string8(buf: &[u8]) -> Result<(&str, usize), SbeDecodeError> {
    if buf.is_empty() {
        return Err(SbeDecodeError::BufferTooShort {
            expected: 1,
            actual: 0,
        });
    }

    let len = usize::from(buf[0]);
    let total_len = 1 + len;
    if buf.len() < total_len {
        return Err(SbeDecodeError::BufferTooShort {
            expected: total_len,
            actual: buf.len(),
        });
    }

    let s = str::from_utf8(&buf[1..total_len]).map_err(|_| SbeDecodeError::InvalidUtf8)?;
    Ok((s, total_len))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_group_size_decode_too_short() {
        let err = GroupSizeEncoding::decode(&[0, 0, 0]).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_group_size_decode_too_large() {
        let mut buf = [0u8; GroupSizeEncoding::ENCODED_LENGTH];
        buf[2..6].copy_from_slice(&(MAX_GROUP_SIZE + 1).to_le_bytes());
        let err = GroupSizeEncoding::decode(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::GroupSizeTooLarge { .. }));
    }

    #[rstest]
    fn test_group_size_16_decode_too_large() {
        let mut buf = [0u8; GroupSize16Encoding::ENCODED_LENGTH];
        buf[2..4].copy_from_slice(&(MAX_GROUP_SIZE as u16 + 1).to_le_bytes());
        let err = GroupSize16Encoding::decode(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::GroupSizeTooLarge { .. }));
    }

    #[rstest]
    fn test_decode_var_string8_valid() {
        let buf = [5u8, b'H', b'E', b'L', b'L', b'O'];
        let (s, consumed) = decode_var_string8(&buf).unwrap();
        assert_eq!(s, "HELLO");
        assert_eq!(consumed, 6);
    }

    #[rstest]
    fn test_decode_var_string8_invalid_utf8() {
        let buf = [2u8, 0xFF, 0xFF];
        let err = decode_var_string8(&buf).unwrap_err();
        assert_eq!(err, SbeDecodeError::InvalidUtf8);
    }
}
