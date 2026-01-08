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

//! Depth diff stream event decoder.
//!
//! Message layout (after 8-byte header):
//! - eventTime: i64 (microseconds)
//! - firstBookUpdateId: i64
//! - lastBookUpdateId: i64
//! - priceExponent: i8
//! - qtyExponent: i8
//! - bids group (groupSize16Encoding: u16 blockLength + u16 numInGroup):
//!   - price: i64 (mantissa)
//!   - qty: i64 (mantissa)
//! - asks group (groupSize16Encoding: u16 blockLength + u16 numInGroup):
//!   - price: i64 (mantissa)
//!   - qty: i64 (mantissa)
//! - symbol: varString8

use ustr::Ustr;

use super::{MessageHeader, PriceLevel, StreamDecodeError};
use crate::common::sbe::cursor::SbeCursor;

/// Depth diff stream event (incremental order book updates).
#[derive(Debug, Clone)]
pub struct DepthDiffStreamEvent {
    /// Event timestamp in microseconds.
    pub event_time_us: i64,
    /// First book update ID in this diff.
    pub first_book_update_id: i64,
    /// Last book update ID in this diff.
    pub last_book_update_id: i64,
    /// Price exponent (prices = mantissa * 10^exponent).
    pub price_exponent: i8,
    /// Quantity exponent (quantities = mantissa * 10^exponent).
    pub qty_exponent: i8,
    /// Bid level updates (qty=0 means remove level).
    pub bids: Vec<PriceLevel>,
    /// Ask level updates (qty=0 means remove level).
    pub asks: Vec<PriceLevel>,
    /// Trading symbol.
    pub symbol: Ustr,
}

impl DepthDiffStreamEvent {
    /// Fixed block length (excluding header, groups, and variable-length data).
    pub const BLOCK_LENGTH: usize = 26;

    /// Decode from SBE buffer (including 8-byte header).
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short, group size exceeds limits,
    /// or data is otherwise invalid.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        let header = MessageHeader::decode(buf)?;
        header.validate_schema()?;

        let mut cursor = SbeCursor::new_at(buf, MessageHeader::ENCODED_LENGTH);

        let event_time_us = cursor.read_i64_le()?;
        let first_book_update_id = cursor.read_i64_le()?;
        let last_book_update_id = cursor.read_i64_le()?;
        let price_exponent = cursor.read_i8()?;
        let qty_exponent = cursor.read_i8()?;

        let (bid_block_length, num_bids) = cursor.read_group_header_16()?;
        let bids = cursor.read_group(bid_block_length, u32::from(num_bids), PriceLevel::decode)?;

        let (ask_block_length, num_asks) = cursor.read_group_header_16()?;
        let asks = cursor.read_group(ask_block_length, u32::from(num_asks), PriceLevel::decode)?;

        let symbol_str = cursor.read_var_string8()?;

        Ok(Self {
            event_time_us,
            first_book_update_id,
            last_book_update_id,
            price_exponent,
            qty_exponent,
            bids,
            asks,
            symbol: Ustr::from(&symbol_str),
        })
    }

    /// Get price as f64 for a level.
    #[inline]
    #[must_use]
    pub fn level_price(&self, level: &PriceLevel) -> f64 {
        super::mantissa_to_f64(level.price_mantissa, self.price_exponent)
    }

    /// Get quantity as f64 for a level.
    #[inline]
    #[must_use]
    pub fn level_qty(&self, level: &PriceLevel) -> f64 {
        super::mantissa_to_f64(level.qty_mantissa, self.qty_exponent)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::sbe::stream::{STREAM_SCHEMA_ID, template_id};

    fn make_valid_buffer(num_bids: usize, num_asks: usize) -> Vec<u8> {
        let level_block_len = 16u16;
        let body_size = 26
            + 4
            + (num_bids * level_block_len as usize)
            + 4
            + (num_asks * level_block_len as usize)
            + 8;
        let mut buf = vec![0u8; 8 + body_size];

        // Header
        buf[0..2].copy_from_slice(&26u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::DEPTH_DIFF_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        // Body
        let body = &mut buf[8..];
        body[0..8].copy_from_slice(&1000000i64.to_le_bytes()); // event_time_us
        body[8..16].copy_from_slice(&12345i64.to_le_bytes()); // first_book_update_id
        body[16..24].copy_from_slice(&12350i64.to_le_bytes()); // last_book_update_id
        body[24] = (-2i8) as u8; // price_exponent
        body[25] = (-8i8) as u8; // qty_exponent

        let mut offset = 26;

        // Bids group header
        body[offset..offset + 2].copy_from_slice(&level_block_len.to_le_bytes());
        body[offset + 2..offset + 4].copy_from_slice(&(num_bids as u16).to_le_bytes());
        offset += 4;

        // Bids
        for i in 0..num_bids {
            body[offset..offset + 8].copy_from_slice(&(4200000i64 - i as i64 * 100).to_le_bytes());
            body[offset + 8..offset + 16].copy_from_slice(&100000000i64.to_le_bytes());
            offset += level_block_len as usize;
        }

        // Asks group header
        body[offset..offset + 2].copy_from_slice(&level_block_len.to_le_bytes());
        body[offset + 2..offset + 4].copy_from_slice(&(num_asks as u16).to_le_bytes());
        offset += 4;

        // Asks
        for i in 0..num_asks {
            body[offset..offset + 8].copy_from_slice(&(4200100i64 + i as i64 * 100).to_le_bytes());
            body[offset + 8..offset + 16].copy_from_slice(&200000000i64.to_le_bytes());
            offset += level_block_len as usize;
        }

        // Symbol: "BTCUSDT"
        body[offset] = 7;
        body[offset + 1..offset + 8].copy_from_slice(b"BTCUSDT");

        buf
    }

    #[rstest]
    fn test_decode_valid() {
        let buf = make_valid_buffer(3, 2);
        let event = DepthDiffStreamEvent::decode(&buf).unwrap();

        assert_eq!(event.event_time_us, 1000000);
        assert_eq!(event.first_book_update_id, 12345);
        assert_eq!(event.last_book_update_id, 12350);
        assert_eq!(event.bids.len(), 3);
        assert_eq!(event.asks.len(), 2);
        assert_eq!(event.symbol, "BTCUSDT");
    }

    #[rstest]
    fn test_decode_empty_updates() {
        let buf = make_valid_buffer(0, 0);
        let event = DepthDiffStreamEvent::decode(&buf).unwrap();

        assert!(event.bids.is_empty());
        assert!(event.asks.is_empty());
    }

    #[rstest]
    fn test_decode_truncated() {
        let mut buf = make_valid_buffer(5, 5);
        buf.truncate(60); // Truncate in the middle
        let err = DepthDiffStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = make_valid_buffer(3, 2);
        buf[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = DepthDiffStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }
}
