// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Depth snapshot stream event decoder.
//!
//! Message layout (after 8-byte header):
//! - eventTime: i64 (microseconds)
//! - bookUpdateId: i64
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

use super::{
    GroupSize16Encoding, MessageHeader, PriceLevel, StreamDecodeError, decode_var_string8, read_i8,
    read_i64_le,
};

/// Depth snapshot stream event (top N levels of order book).
#[derive(Debug, Clone)]
pub struct DepthSnapshotStreamEvent {
    /// Event timestamp in microseconds.
    pub event_time_us: i64,
    /// Book update ID for sequencing.
    pub book_update_id: i64,
    /// Price exponent (prices = mantissa * 10^exponent).
    pub price_exponent: i8,
    /// Quantity exponent (quantities = mantissa * 10^exponent).
    pub qty_exponent: i8,
    /// Bid levels (best bid first).
    pub bids: Vec<PriceLevel>,
    /// Ask levels (best ask first).
    pub asks: Vec<PriceLevel>,
    /// Trading symbol.
    pub symbol: Ustr,
}

impl DepthSnapshotStreamEvent {
    /// Fixed block length (excluding header, groups, and variable-length data).
    pub const BLOCK_LENGTH: usize = 18;

    /// Decode from SBE buffer (including 8-byte header).
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short, group size exceeds limits,
    /// or data is otherwise invalid.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        let header = MessageHeader::decode(buf)?;
        header.validate_schema()?;

        let body = &buf[MessageHeader::ENCODED_LENGTH..];

        let min_body_size = Self::BLOCK_LENGTH + GroupSize16Encoding::ENCODED_LENGTH;
        if body.len() < min_body_size {
            return Err(StreamDecodeError::BufferTooShort {
                expected: MessageHeader::ENCODED_LENGTH + min_body_size,
                actual: buf.len(),
            });
        }

        let event_time_us = read_i64_le(body, 0)?;
        let book_update_id = read_i64_le(body, 8)?;
        let price_exponent = read_i8(body, 16)?;
        let qty_exponent = read_i8(body, 17)?;

        let mut offset = Self::BLOCK_LENGTH;

        // Group size limit enforced inside GroupSize16Encoding::decode
        let bids_group = GroupSize16Encoding::decode(&body[offset..])?;
        let num_bids = bids_group.num_in_group as usize;
        let bid_block_length = bids_group.block_length as usize;
        offset += GroupSize16Encoding::ENCODED_LENGTH;

        let bids_data_size = num_bids * bid_block_length;
        if body.len() < offset + bids_data_size + GroupSize16Encoding::ENCODED_LENGTH {
            return Err(StreamDecodeError::BufferTooShort {
                expected: MessageHeader::ENCODED_LENGTH
                    + offset
                    + bids_data_size
                    + GroupSize16Encoding::ENCODED_LENGTH,
                actual: buf.len(),
            });
        }

        let mut bids = Vec::with_capacity(num_bids);
        for _ in 0..num_bids {
            bids.push(PriceLevel::decode(&body[offset..])?);
            offset += bid_block_length;
        }

        let asks_group = GroupSize16Encoding::decode(&body[offset..])?;
        let num_asks = asks_group.num_in_group as usize;
        let ask_block_length = asks_group.block_length as usize;
        offset += GroupSize16Encoding::ENCODED_LENGTH;

        let asks_data_size = num_asks * ask_block_length;
        if body.len() < offset + asks_data_size + 1 {
            return Err(StreamDecodeError::BufferTooShort {
                expected: MessageHeader::ENCODED_LENGTH + offset + asks_data_size + 1,
                actual: buf.len(),
            });
        }

        let mut asks = Vec::with_capacity(num_asks);
        for _ in 0..num_asks {
            asks.push(PriceLevel::decode(&body[offset..])?);
            offset += ask_block_length;
        }

        let (symbol_str, _) = decode_var_string8(&body[offset..])?;

        Ok(Self {
            event_time_us,
            book_update_id,
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
        let body_size = 18
            + 4
            + (num_bids * level_block_len as usize)
            + 4
            + (num_asks * level_block_len as usize)
            + 8;
        let mut buf = vec![0u8; 8 + body_size];

        // Header
        buf[0..2].copy_from_slice(&18u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::DEPTH_SNAPSHOT_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        // Body
        let body = &mut buf[8..];
        body[0..8].copy_from_slice(&1000000i64.to_le_bytes()); // event_time_us
        body[8..16].copy_from_slice(&12345i64.to_le_bytes()); // book_update_id
        body[16] = (-2i8) as u8; // price_exponent
        body[17] = (-8i8) as u8; // qty_exponent

        let mut offset = 18;

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
        let buf = make_valid_buffer(5, 5);
        let event = DepthSnapshotStreamEvent::decode(&buf).unwrap();

        assert_eq!(event.event_time_us, 1000000);
        assert_eq!(event.book_update_id, 12345);
        assert_eq!(event.bids.len(), 5);
        assert_eq!(event.asks.len(), 5);
        assert_eq!(event.symbol, "BTCUSDT");
    }

    #[rstest]
    fn test_decode_empty_books() {
        let buf = make_valid_buffer(0, 0);
        let event = DepthSnapshotStreamEvent::decode(&buf).unwrap();

        assert!(event.bids.is_empty());
        assert!(event.asks.is_empty());
    }

    #[rstest]
    fn test_decode_truncated() {
        let mut buf = make_valid_buffer(10, 10);
        buf.truncate(100); // Truncate in the middle
        let err = DepthSnapshotStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = make_valid_buffer(5, 5);
        buf[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = DepthSnapshotStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }
}
