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

// SBE decode cursor - all read methods return SbeDecodeError::BufferUnderrun when buffer too short
#![allow(clippy::missing_errors_doc)]

//! Zero-copy SBE byte cursor for sequential decoding.

use super::error::{MAX_GROUP_SIZE, SbeDecodeError};

/// Zero-copy SBE byte cursor for sequential decoding.
///
/// Wraps a byte slice and tracks position, providing typed read methods
/// that automatically advance the cursor.
#[derive(Debug, Clone)]
pub struct SbeCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> SbeCursor<'a> {
    /// Creates a new cursor at position 0.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Creates a cursor starting at a specific offset.
    #[must_use]
    pub const fn new_at(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    /// Current position in the buffer.
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Remaining bytes from current position.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Returns the underlying buffer.
    #[must_use]
    pub const fn buffer(&self) -> &'a [u8] {
        self.buf
    }

    /// Returns remaining bytes as a slice.
    #[must_use]
    pub fn peek(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    /// Ensures at least `n` bytes remain.
    ///
    /// # Errors
    ///
    /// Returns `BufferTooShort` if fewer than `n` bytes remain.
    pub fn require(&self, n: usize) -> Result<(), SbeDecodeError> {
        if self.remaining() < n {
            return Err(SbeDecodeError::BufferTooShort {
                expected: self.pos + n,
                actual: self.buf.len(),
            });
        }
        Ok(())
    }

    /// Advances position by `n` bytes.
    ///
    /// # Errors
    ///
    /// Returns `BufferTooShort` if fewer than `n` bytes remain.
    pub fn advance(&mut self, n: usize) -> Result<(), SbeDecodeError> {
        self.require(n)?;
        self.pos += n;
        Ok(())
    }

    /// Skips `n` bytes without bounds checking.
    ///
    /// # Safety
    ///
    /// Caller must ensure `n` bytes are available.
    pub fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    /// Resets cursor to start of buffer.
    pub fn reset(&mut self) {
        self.pos = 0;
    }

    /// Sets cursor to a specific position.
    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Reads a u8 and advances by 1 byte.
    pub fn read_u8(&mut self) -> Result<u8, SbeDecodeError> {
        self.require(1)?;
        let value = self.buf[self.pos];
        self.pos += 1;
        Ok(value)
    }

    /// Reads an i8 and advances by 1 byte.
    pub fn read_i8(&mut self) -> Result<i8, SbeDecodeError> {
        self.require(1)?;
        let value = self.buf[self.pos] as i8;
        self.pos += 1;
        Ok(value)
    }

    /// Reads a u16 little-endian and advances by 2 bytes.
    pub fn read_u16_le(&mut self) -> Result<u16, SbeDecodeError> {
        self.require(2)?;
        let value = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(value)
    }

    /// Reads an i16 little-endian and advances by 2 bytes.
    pub fn read_i16_le(&mut self) -> Result<i16, SbeDecodeError> {
        self.require(2)?;
        let value = i16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(value)
    }

    /// Reads a u32 little-endian and advances by 4 bytes.
    pub fn read_u32_le(&mut self) -> Result<u32, SbeDecodeError> {
        self.require(4)?;
        let value = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(value)
    }

    /// Reads an i32 little-endian and advances by 4 bytes.
    pub fn read_i32_le(&mut self) -> Result<i32, SbeDecodeError> {
        self.require(4)?;
        let value = i32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(value)
    }

    /// Reads a u64 little-endian and advances by 8 bytes.
    pub fn read_u64_le(&mut self) -> Result<u64, SbeDecodeError> {
        self.require(8)?;
        let value = u64::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
            self.buf[self.pos + 4],
            self.buf[self.pos + 5],
            self.buf[self.pos + 6],
            self.buf[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(value)
    }

    /// Reads an i64 little-endian and advances by 8 bytes.
    pub fn read_i64_le(&mut self) -> Result<i64, SbeDecodeError> {
        self.require(8)?;
        let value = i64::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
            self.buf[self.pos + 4],
            self.buf[self.pos + 5],
            self.buf[self.pos + 6],
            self.buf[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(value)
    }

    /// Reads an optional i64 where `i64::MIN` represents None.
    pub fn read_optional_i64_le(&mut self) -> Result<Option<i64>, SbeDecodeError> {
        let value = self.read_i64_le()?;
        Ok(if value == i64::MIN { None } else { Some(value) })
    }

    /// Reads N bytes and advances.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], SbeDecodeError> {
        self.require(n)?;
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Reads a varString8 (1-byte length prefix + UTF-8 data).
    ///
    /// Returns empty string if length is 0.
    pub fn read_var_string8(&mut self) -> Result<String, SbeDecodeError> {
        let len = self.read_u8()? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        self.require(len)?;
        let s = std::str::from_utf8(&self.buf[self.pos..self.pos + len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?
            .to_string();
        self.pos += len;
        Ok(s)
    }

    /// Reads a varString8 as a &str (zero-copy).
    pub fn read_var_string8_ref(&mut self) -> Result<&'a str, SbeDecodeError> {
        let len = self.read_u8()? as usize;
        if len == 0 {
            return Ok("");
        }
        self.require(len)?;
        let s = std::str::from_utf8(&self.buf[self.pos..self.pos + len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?;
        self.pos += len;
        Ok(s)
    }

    /// Skips a varData8 field (1-byte length prefix + binary data).
    ///
    /// Used for skipping binary fields that should not be decoded as UTF-8.
    pub fn skip_var_data8(&mut self) -> Result<(), SbeDecodeError> {
        let len = self.read_u8()? as usize;
        if len > 0 {
            self.advance(len)?;
        }
        Ok(())
    }

    /// Reads a varData8 field (1-byte length prefix + binary data).
    ///
    /// Returns the raw bytes without UTF-8 decoding.
    pub fn read_var_bytes8(&mut self) -> Result<Vec<u8>, SbeDecodeError> {
        let len = self.read_u8()? as usize;
        if len == 0 {
            return Ok(Vec::new());
        }
        self.require(len)?;
        let bytes = self.buf[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Ok(bytes)
    }

    /// Reads group header (u16 block_length + u32 num_in_group).
    ///
    /// Returns (block_length, num_in_group).
    pub fn read_group_header(&mut self) -> Result<(u16, u32), SbeDecodeError> {
        let block_length = self.read_u16_le()?;
        let num_in_group = self.read_u32_le()?;

        if num_in_group > MAX_GROUP_SIZE {
            return Err(SbeDecodeError::GroupSizeTooLarge {
                count: num_in_group,
                max: MAX_GROUP_SIZE,
            });
        }

        Ok((block_length, num_in_group))
    }

    /// Reads compact group header (u16 block_length + u16 num_in_group).
    ///
    /// Returns (block_length, num_in_group).
    pub fn read_group_header_16(&mut self) -> Result<(u16, u16), SbeDecodeError> {
        let block_length = self.read_u16_le()?;
        let num_in_group = self.read_u16_le()?;

        if u32::from(num_in_group) > MAX_GROUP_SIZE {
            return Err(SbeDecodeError::GroupSizeTooLarge {
                count: u32::from(num_in_group),
                max: MAX_GROUP_SIZE,
            });
        }

        Ok((block_length, num_in_group))
    }

    /// Iterates over a group, calling `decode_item` for each element.
    ///
    /// The decoder function receives a cursor positioned at the start of each item
    /// and should decode the item without advancing past `block_length` bytes.
    pub fn read_group<T, F>(
        &mut self,
        block_length: u16,
        num_in_group: u32,
        mut decode_item: F,
    ) -> Result<Vec<T>, SbeDecodeError>
    where
        F: FnMut(&mut Self) -> Result<T, SbeDecodeError>,
    {
        let block_len = block_length as usize;
        let count = num_in_group as usize;

        // Validate we have enough bytes for all items
        self.require(count * block_len)?;

        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let item_start = self.pos;
            let item = decode_item(self)?;
            items.push(item);

            // Advance to next item boundary (respects block_length even if decoder read less)
            self.pos = item_start + block_len;
        }

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_starts_at_zero() {
        let buf = [1, 2, 3, 4];
        let cursor = SbeCursor::new(&buf);
        assert_eq!(cursor.pos(), 0);
        assert_eq!(cursor.remaining(), 4);
    }

    #[rstest]
    fn test_new_at_starts_at_offset() {
        let buf = [1, 2, 3, 4];
        let cursor = SbeCursor::new_at(&buf, 2);
        assert_eq!(cursor.pos(), 2);
        assert_eq!(cursor.remaining(), 2);
    }

    #[rstest]
    fn test_read_u8() {
        let buf = [0x42, 0xFF];
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_u8().unwrap(), 0x42);
        assert_eq!(cursor.pos(), 1);

        assert_eq!(cursor.read_u8().unwrap(), 0xFF);
        assert_eq!(cursor.pos(), 2);

        assert!(cursor.read_u8().is_err());
    }

    #[rstest]
    fn test_read_i8() {
        let buf = [0x7F, 0x80]; // 127, -128
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_i8().unwrap(), 127);
        assert_eq!(cursor.read_i8().unwrap(), -128);
    }

    #[rstest]
    fn test_read_u16_le() {
        let buf = [0x34, 0x12]; // 0x1234 in little-endian
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_u16_le().unwrap(), 0x1234);
        assert_eq!(cursor.pos(), 2);
    }

    #[rstest]
    fn test_read_i64_le() {
        let value: i64 = -1234567890123456789;
        let buf = value.to_le_bytes();
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_i64_le().unwrap(), value);
        assert_eq!(cursor.pos(), 8);
    }

    #[rstest]
    fn test_read_optional_i64_null() {
        let buf = i64::MIN.to_le_bytes();
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_optional_i64_le().unwrap(), None);
    }

    #[rstest]
    fn test_read_optional_i64_present() {
        let value: i64 = 12345;
        let buf = value.to_le_bytes();
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_optional_i64_le().unwrap(), Some(12345));
    }

    #[rstest]
    fn test_read_var_string8() {
        let mut buf = vec![5]; // length = 5
        buf.extend_from_slice(b"hello");
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_var_string8().unwrap(), "hello");
        assert_eq!(cursor.pos(), 6); // 1 + 5
    }

    #[rstest]
    fn test_read_var_string8_empty() {
        let buf = [0]; // length = 0
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.read_var_string8().unwrap(), "");
        assert_eq!(cursor.pos(), 1);
    }

    #[rstest]
    fn test_read_var_string8_invalid_utf8() {
        let buf = [2, 0xFF, 0xFE]; // length = 2, invalid UTF-8
        let mut cursor = SbeCursor::new(&buf);

        assert!(matches!(
            cursor.read_var_string8(),
            Err(SbeDecodeError::InvalidUtf8)
        ));
    }

    #[rstest]
    fn test_read_group_header() {
        // block_length = 24, num_in_group = 3
        let buf = [24, 0, 3, 0, 0, 0];
        let mut cursor = SbeCursor::new(&buf);

        let (block_len, count) = cursor.read_group_header().unwrap();
        assert_eq!(block_len, 24);
        assert_eq!(count, 3);
        assert_eq!(cursor.pos(), 6);
    }

    #[rstest]
    fn test_read_group_header_too_large() {
        // num_in_group = MAX_GROUP_SIZE + 1
        let count = MAX_GROUP_SIZE + 1;
        let mut buf = vec![24, 0]; // block_length = 24
        buf.extend_from_slice(&count.to_le_bytes());
        let mut cursor = SbeCursor::new(&buf);

        assert!(matches!(
            cursor.read_group_header(),
            Err(SbeDecodeError::GroupSizeTooLarge { .. })
        ));
    }

    #[rstest]
    fn test_read_group() {
        // 2 items, each 4 bytes containing a u32
        let mut buf = Vec::new();
        buf.extend_from_slice(&100u32.to_le_bytes()); // item 0
        buf.extend_from_slice(&200u32.to_le_bytes()); // item 1

        let mut cursor = SbeCursor::new(&buf);
        let items: Vec<u32> = cursor.read_group(4, 2, |c| c.read_u32_le()).unwrap();

        assert_eq!(items, vec![100, 200]);
        assert_eq!(cursor.pos(), 8);
    }

    #[rstest]
    fn test_read_group_respects_block_length() {
        // 2 items, block_length = 8, but we only read 4 bytes per item
        let mut buf = Vec::new();
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]); // padding
        buf.extend_from_slice(&200u32.to_le_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]); // padding

        let mut cursor = SbeCursor::new(&buf);
        let items: Vec<u32> = cursor.read_group(8, 2, |c| c.read_u32_le()).unwrap();

        assert_eq!(items, vec![100, 200]);
        assert_eq!(cursor.pos(), 16); // 2 * 8
    }

    #[rstest]
    fn test_require_success() {
        let buf = [1, 2, 3, 4];
        let cursor = SbeCursor::new(&buf);

        assert!(cursor.require(4).is_ok());
        assert!(cursor.require(3).is_ok());
    }

    #[rstest]
    fn test_require_failure() {
        let buf = [1, 2];
        let cursor = SbeCursor::new(&buf);

        let err = cursor.require(5).unwrap_err();
        assert!(matches!(
            err,
            SbeDecodeError::BufferTooShort {
                expected: 5,
                actual: 2
            }
        ));
    }

    #[rstest]
    fn test_advance() {
        let buf = [1, 2, 3, 4];
        let mut cursor = SbeCursor::new(&buf);

        cursor.advance(2).unwrap();
        assert_eq!(cursor.pos(), 2);

        cursor.advance(2).unwrap();
        assert_eq!(cursor.pos(), 4);

        assert!(cursor.advance(1).is_err());
    }

    #[rstest]
    fn test_peek() {
        let buf = [1, 2, 3, 4];
        let mut cursor = SbeCursor::new(&buf);

        assert_eq!(cursor.peek(), &[1, 2, 3, 4]);

        cursor.advance(2).unwrap();
        assert_eq!(cursor.peek(), &[3, 4]);
    }

    #[rstest]
    fn test_reset() {
        let buf = [1, 2, 3, 4];
        let mut cursor = SbeCursor::new(&buf);

        cursor.advance(3).unwrap();
        assert_eq!(cursor.pos(), 3);

        cursor.reset();
        assert_eq!(cursor.pos(), 0);
    }
}
