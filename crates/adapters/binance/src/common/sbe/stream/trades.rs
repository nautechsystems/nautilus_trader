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

//! Trades stream event decoder.
//!
//! Message layout (after 8-byte header):
//! - eventTime: i64 (microseconds)
//! - transactTime: i64 (microseconds)
//! - priceExponent: i8
//! - qtyExponent: i8
//! - trades group (groupSizeEncoding: u16 blockLength + u32 numInGroup):
//!   - id: i64
//!   - price: i64 (mantissa)
//!   - qty: i64 (mantissa)
//!   - isBuyerMaker: u8
//! - symbol: varString8

use ustr::Ustr;

use super::{MessageHeader, StreamDecodeError};
use crate::common::sbe::{cursor::SbeCursor, error::SbeDecodeError};

/// Individual trade within a trades stream event.
#[derive(Debug, Clone, Copy)]
pub struct Trade {
    /// Trade ID.
    pub id: i64,
    /// Price mantissa.
    pub price_mantissa: i64,
    /// Quantity mantissa.
    pub qty_mantissa: i64,
    /// True if buyer is the maker (seller initiated the trade).
    pub is_buyer_maker: bool,
}

impl Trade {
    /// Encoded length per trade entry.
    pub const ENCODED_LENGTH: usize = 25;

    /// Decode a single trade from cursor.
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short.
    fn decode(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        Ok(Self {
            id: cursor.read_i64_le()?,
            price_mantissa: cursor.read_i64_le()?,
            qty_mantissa: cursor.read_i64_le()?,
            is_buyer_maker: cursor.read_u8()? != 0,
        })
    }
}

/// Trades stream event (may contain multiple trades).
#[derive(Debug, Clone)]
pub struct TradesStreamEvent {
    /// Event timestamp in microseconds.
    pub event_time_us: i64,
    /// Transaction timestamp in microseconds.
    pub transact_time_us: i64,
    /// Price exponent (prices = mantissa * 10^exponent).
    pub price_exponent: i8,
    /// Quantity exponent (quantities = mantissa * 10^exponent).
    pub qty_exponent: i8,
    /// Trades in this event.
    pub trades: Vec<Trade>,
    /// Trading symbol.
    pub symbol: Ustr,
}

impl TradesStreamEvent {
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

        let mut cursor = SbeCursor::new_at(buf, MessageHeader::ENCODED_LENGTH);

        let event_time_us = cursor.read_i64_le()?;
        let transact_time_us = cursor.read_i64_le()?;
        let price_exponent = cursor.read_i8()?;
        let qty_exponent = cursor.read_i8()?;

        let (block_length, num_in_group) = cursor.read_group_header()?;
        let trades = cursor.read_group(block_length, num_in_group, Trade::decode)?;

        let symbol_str = cursor.read_var_string8()?;

        Ok(Self {
            event_time_us,
            transact_time_us,
            price_exponent,
            qty_exponent,
            trades,
            symbol: Ustr::from(&symbol_str),
        })
    }

    /// Get price as f64 for a trade.
    #[inline]
    #[must_use]
    pub fn trade_price(&self, trade: &Trade) -> f64 {
        super::mantissa_to_f64(trade.price_mantissa, self.price_exponent)
    }

    /// Get quantity as f64 for a trade.
    #[inline]
    #[must_use]
    pub fn trade_qty(&self, trade: &Trade) -> f64 {
        super::mantissa_to_f64(trade.qty_mantissa, self.qty_exponent)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::sbe::stream::{STREAM_SCHEMA_ID, template_id};

    fn make_valid_buffer(num_trades: usize) -> Vec<u8> {
        let trade_block_len = 25u16;
        let body_size = 18 + 6 + (num_trades * trade_block_len as usize) + 8; // fixed + group header + trades + symbol
        let mut buf = vec![0u8; 8 + body_size];

        // Header
        buf[0..2].copy_from_slice(&18u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::TRADES_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        // Body
        let body = &mut buf[8..];
        body[0..8].copy_from_slice(&1000000i64.to_le_bytes()); // event_time_us
        body[8..16].copy_from_slice(&1000001i64.to_le_bytes()); // transact_time_us
        body[16] = (-2i8) as u8; // price_exponent
        body[17] = (-8i8) as u8; // qty_exponent

        // Group header
        body[18..20].copy_from_slice(&trade_block_len.to_le_bytes());
        body[20..24].copy_from_slice(&(num_trades as u32).to_le_bytes());

        // Trades
        let mut offset = 24;
        for i in 0..num_trades {
            body[offset..offset + 8].copy_from_slice(&(i as i64 + 1).to_le_bytes()); // id
            body[offset + 8..offset + 16].copy_from_slice(&4200000i64.to_le_bytes()); // price
            body[offset + 16..offset + 24].copy_from_slice(&100000000i64.to_le_bytes()); // qty
            body[offset + 24] = u8::from(i % 2 == 0); // is_buyer_maker
            offset += trade_block_len as usize;
        }

        // Symbol: "BTCUSDT"
        body[offset] = 7;
        body[offset + 1..offset + 8].copy_from_slice(b"BTCUSDT");

        buf
    }

    #[rstest]
    fn test_decode_valid_single_trade() {
        let buf = make_valid_buffer(1);
        let event = TradesStreamEvent::decode(&buf).unwrap();

        assert_eq!(event.event_time_us, 1000000);
        assert_eq!(event.transact_time_us, 1000001);
        assert_eq!(event.trades.len(), 1);
        assert_eq!(event.trades[0].id, 1);
        assert!(event.trades[0].is_buyer_maker);
        assert_eq!(event.symbol, "BTCUSDT");
    }

    #[rstest]
    fn test_decode_valid_multiple_trades() {
        let buf = make_valid_buffer(5);
        let event = TradesStreamEvent::decode(&buf).unwrap();

        assert_eq!(event.trades.len(), 5);
        for (i, trade) in event.trades.iter().enumerate() {
            assert_eq!(trade.id, i as i64 + 1);
        }
    }

    #[rstest]
    fn test_decode_truncated_trades() {
        let mut buf = make_valid_buffer(3);
        buf.truncate(50); // Truncate in the middle of trades
        let err = TradesStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = make_valid_buffer(1);
        buf[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = TradesStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }
}
