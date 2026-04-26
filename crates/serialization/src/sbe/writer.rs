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

//! Pre-sized SBE byte writer for sequential encoding.
//!
//! The writer uses narrow `unsafe` blocks to avoid zero-initializing the backing
//! buffer: safe-layout slice conversions between `[u8]` and `[MaybeUninit<u8>]`,
//! and pointer-based aligned stores into the `MaybeUninit<u8>` slots. The crate
//! otherwise forbids unsafe code; this module is the only file in the SBE
//! codec that opts in, and every unsafe block carries a SAFETY comment
//! documenting the invariant it relies on.

#![allow(
    unsafe_code,
    reason = "SBE encoder needs uninit buffer writes to avoid a memset pass"
)]

use std::mem::MaybeUninit;

/// Pre-sized SBE byte writer for sequential encoding.
///
/// Wraps a mutable `[MaybeUninit<u8>]` slice and tracks position, providing
/// typed write methods that automatically advance the cursor. The caller is
/// responsible for sizing the backing buffer to hold the full encoded
/// message; writing past the end panics.
///
/// The writer uses `MaybeUninit<u8>` to avoid requiring the target buffer to
/// be zero-initialized. Callers wrapping an already-initialized `&mut [u8]`
/// can use [`SbeWriter::new`]; callers writing into uninit capacity (e.g.
/// `Vec::spare_capacity_mut`) can use [`SbeWriter::new_uninit`].
#[derive(Debug)]
pub struct SbeWriter<'a> {
    buf: &'a mut [MaybeUninit<u8>],
    pos: usize,
}

impl<'a> SbeWriter<'a> {
    /// Creates a new writer over an initialized byte buffer.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        // SAFETY: `MaybeUninit<u8>` has the same layout as `u8`, and going
        // from `&mut [u8]` to `&mut [MaybeUninit<u8>]` is always valid (the
        // elements are already initialized; we simply allow overwriting them
        // without reading first).
        let uninit = unsafe {
            std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<MaybeUninit<u8>>(), buf.len())
        };
        Self {
            buf: uninit,
            pos: 0,
        }
    }

    /// Creates a new writer over an uninitialized byte buffer.
    #[inline]
    pub fn new_uninit(buf: &'a mut [MaybeUninit<u8>]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current write position in the buffer.
    #[inline]
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// Total buffer length.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the buffer is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Writes a single byte and advances by 1.
    #[inline]
    pub fn write_u8(&mut self, value: u8) {
        self.buf[self.pos].write(value);
        self.pos += 1;
    }

    /// Writes a signed single byte and advances by 1.
    #[inline]
    pub fn write_i8(&mut self, value: i8) {
        self.buf[self.pos].write(value as u8);
        self.pos += 1;
    }

    /// Writes a u16 little-endian and advances by 2 bytes.
    #[inline]
    pub fn write_u16_le(&mut self, value: u16) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes an i16 little-endian and advances by 2 bytes.
    #[inline]
    pub fn write_i16_le(&mut self, value: i16) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes a u32 little-endian and advances by 4 bytes.
    #[inline]
    pub fn write_u32_le(&mut self, value: u32) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes an i32 little-endian and advances by 4 bytes.
    #[inline]
    pub fn write_i32_le(&mut self, value: i32) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes a u64 little-endian and advances by 8 bytes.
    #[inline]
    pub fn write_u64_le(&mut self, value: u64) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes an i64 little-endian and advances by 8 bytes.
    #[inline]
    pub fn write_i64_le(&mut self, value: i64) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes a u128 little-endian and advances by 16 bytes.
    #[inline]
    pub fn write_u128_le(&mut self, value: u128) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes an i128 little-endian and advances by 16 bytes.
    #[inline]
    pub fn write_i128_le(&mut self, value: i128) {
        self.write_array(value.to_le_bytes());
    }

    /// Writes a slice of bytes and advances by its length.
    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let end = self.pos + bytes.len();
        // Bounds-check the destination via safe slice indexing, then copy
        // through raw pointers. Constructing a `&mut [u8]` over the
        // `MaybeUninit<u8>` slots would be UB even when only written; pointer
        // writes do not impose the "reference refers to initialized memory"
        // invariant.
        let dst_ptr = self.buf[self.pos..end].as_mut_ptr().cast::<u8>();
        unsafe {
            // SAFETY: `dst_ptr` points to `bytes.len()` consecutive
            // `MaybeUninit<u8>` slots (same layout as u8); we initialize
            // every slot in the region. Source and destination cannot overlap
            // because `bytes` is an immutable borrow distinct from `self.buf`.
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst_ptr, bytes.len());
        }
        self.pos = end;
    }

    // Mirrors `SbeCursor::read_array`: const-generic slice-to-array writes let
    // LLVM lower each call to a single aligned store.
    #[inline]
    fn write_array<const N: usize>(&mut self, bytes: [u8; N]) {
        let end = self.pos + N;
        let dst_ptr = self.buf[self.pos..end].as_mut_ptr().cast::<[u8; N]>();
        unsafe {
            // SAFETY: slice bounds were checked by the indexing above, so
            // `dst_ptr` points to an N-element `MaybeUninit<u8>` region with
            // the same layout as `[u8; N]`. We overwrite all N bytes.
            *dst_ptr = bytes;
        }
        self.pos = end;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_starts_at_zero() {
        let mut buf = [0u8; 4];
        let writer = SbeWriter::new(&mut buf);
        assert_eq!(writer.pos(), 0);
        assert_eq!(writer.len(), 4);
    }

    #[rstest]
    fn test_write_u8() {
        let mut buf = [0u8; 2];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_u8(0x42);
        writer.write_u8(0xFF);
        assert_eq!(writer.pos(), 2);
        assert_eq!(buf, [0x42, 0xFF]);
    }

    #[rstest]
    fn test_write_u16_le() {
        let mut buf = [0u8; 2];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_u16_le(0x1234);
        assert_eq!(writer.pos(), 2);
        assert_eq!(buf, [0x34, 0x12]);
    }

    #[rstest]
    fn test_write_i64_le() {
        let value: i64 = -1_234_567_890_123_456_789;
        let mut buf = [0u8; 8];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_i64_le(value);
        assert_eq!(writer.pos(), 8);
        assert_eq!(buf, value.to_le_bytes());
    }

    #[rstest]
    fn test_write_u128_le() {
        let value: u128 = 0x0123_4567_89AB_CDEF_FEDC_BA98_7654_3210;
        let mut buf = [0u8; 16];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_u128_le(value);
        assert_eq!(writer.pos(), 16);
        assert_eq!(buf, value.to_le_bytes());
    }

    #[rstest]
    fn test_write_bytes() {
        let mut buf = [0u8; 5];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_bytes(&[1, 2, 3]);
        writer.write_bytes(&[4, 5]);
        assert_eq!(writer.pos(), 5);
        assert_eq!(buf, [1, 2, 3, 4, 5]);
    }

    #[rstest]
    fn test_write_into_uninit() {
        let mut buf: [MaybeUninit<u8>; 4] = [const { MaybeUninit::uninit() }; 4];
        let mut writer = SbeWriter::new_uninit(&mut buf);
        writer.write_u8(0xAA);
        writer.write_u16_le(0x1234);
        writer.write_u8(0xBB);
        assert_eq!(writer.pos(), 4);
        // SAFETY: all 4 bytes were written by the writer above.
        let initialized: [u8; 4] = unsafe { std::mem::transmute(buf) };
        assert_eq!(initialized, [0xAA, 0x34, 0x12, 0xBB]);
    }

    #[rstest]
    #[should_panic(expected = "index out of bounds")]
    fn test_write_past_end_panics() {
        let mut buf = [0u8; 2];
        let mut writer = SbeWriter::new(&mut buf);
        writer.write_u8(0);
        writer.write_u8(0);
        writer.write_u8(0);
    }
}
