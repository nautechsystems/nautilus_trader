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

//! BestBidAsk stream event decoder.
//!
//! Message layout (after 8-byte header):
//! - eventTime: i64 (microseconds)
//! - bookUpdateId: i64
//! - priceExponent: i8
//! - qtyExponent: i8
//! - bidPrice: i64 (mantissa)
//! - bidQty: i64 (mantissa)
//! - askPrice: i64 (mantissa)
//! - askQty: i64 (mantissa)
//! - symbol: varString8

use ustr::Ustr;

use super::{MessageHeader, StreamDecodeError, decode_var_string8, read_i8, read_i64_le};

/// Best bid/ask stream event.
#[derive(Debug, Clone)]
pub struct BestBidAskStreamEvent {
    /// Event timestamp in microseconds.
    pub event_time_us: i64,
    /// Book update ID for sequencing.
    pub book_update_id: i64,
    /// Price exponent (prices = mantissa * 10^exponent).
    pub price_exponent: i8,
    /// Quantity exponent (quantities = mantissa * 10^exponent).
    pub qty_exponent: i8,
    /// Best bid price mantissa.
    pub bid_price_mantissa: i64,
    /// Best bid quantity mantissa.
    pub bid_qty_mantissa: i64,
    /// Best ask price mantissa.
    pub ask_price_mantissa: i64,
    /// Best ask quantity mantissa.
    pub ask_qty_mantissa: i64,
    /// Trading symbol.
    pub symbol: Ustr,
}

impl BestBidAskStreamEvent {
    /// Fixed block length (excluding header and variable-length data).
    pub const BLOCK_LENGTH: usize = 50;

    /// Minimum buffer size needed (header + block + 1-byte string length).
    pub const MIN_BUFFER_SIZE: usize = MessageHeader::ENCODED_LENGTH + Self::BLOCK_LENGTH + 1;

    /// Decode from SBE buffer (including 8-byte header).
    ///
    /// # Errors
    ///
    /// Returns error if buffer is too short or contains invalid data.
    pub fn decode(buf: &[u8]) -> Result<Self, StreamDecodeError> {
        let header = MessageHeader::decode(buf)?;
        header.validate_schema()?;

        let body = &buf[MessageHeader::ENCODED_LENGTH..];

        if body.len() < Self::BLOCK_LENGTH + 1 {
            return Err(StreamDecodeError::BufferTooShort {
                expected: Self::MIN_BUFFER_SIZE,
                actual: buf.len(),
            });
        }

        let event_time_us = read_i64_le(body, 0)?;
        let book_update_id = read_i64_le(body, 8)?;
        let price_exponent = read_i8(body, 16)?;
        let qty_exponent = read_i8(body, 17)?;
        let bid_price_mantissa = read_i64_le(body, 18)?;
        let bid_qty_mantissa = read_i64_le(body, 26)?;
        let ask_price_mantissa = read_i64_le(body, 34)?;
        let ask_qty_mantissa = read_i64_le(body, 42)?;

        let (symbol_str, _) = decode_var_string8(&body[50..])?;

        Ok(Self {
            event_time_us,
            book_update_id,
            price_exponent,
            qty_exponent,
            bid_price_mantissa,
            bid_qty_mantissa,
            ask_price_mantissa,
            ask_qty_mantissa,
            symbol: Ustr::from(&symbol_str),
        })
    }

    /// Get bid price as f64.
    #[inline]
    #[must_use]
    pub fn bid_price(&self) -> f64 {
        super::mantissa_to_f64(self.bid_price_mantissa, self.price_exponent)
    }

    /// Get bid quantity as f64.
    #[inline]
    #[must_use]
    pub fn bid_qty(&self) -> f64 {
        super::mantissa_to_f64(self.bid_qty_mantissa, self.qty_exponent)
    }

    /// Get ask price as f64.
    #[inline]
    #[must_use]
    pub fn ask_price(&self) -> f64 {
        super::mantissa_to_f64(self.ask_price_mantissa, self.price_exponent)
    }

    /// Get ask quantity as f64.
    #[inline]
    #[must_use]
    pub fn ask_qty(&self) -> f64 {
        super::mantissa_to_f64(self.ask_qty_mantissa, self.qty_exponent)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::sbe::stream::{STREAM_SCHEMA_ID, template_id};

    fn make_valid_buffer() -> Vec<u8> {
        let mut buf = vec![0u8; 70];

        // Header
        buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
        buf[2..4].copy_from_slice(&template_id::BEST_BID_ASK_STREAM_EVENT.to_le_bytes());
        buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

        // Body
        let body = &mut buf[8..];
        body[0..8].copy_from_slice(&1000000i64.to_le_bytes()); // event_time_us
        body[8..16].copy_from_slice(&12345i64.to_le_bytes()); // book_update_id
        body[16] = (-2i8) as u8; // price_exponent
        body[17] = (-8i8) as u8; // qty_exponent
        body[18..26].copy_from_slice(&4200000i64.to_le_bytes()); // bid_price
        body[26..34].copy_from_slice(&100000000i64.to_le_bytes()); // bid_qty
        body[34..42].copy_from_slice(&4200100i64.to_le_bytes()); // ask_price
        body[42..50].copy_from_slice(&200000000i64.to_le_bytes()); // ask_qty

        // Symbol: "BTCUSDT" (7 bytes)
        body[50] = 7;
        body[51..58].copy_from_slice(b"BTCUSDT");

        buf
    }

    #[rstest]
    fn test_decode_valid() {
        let buf = make_valid_buffer();
        let event = BestBidAskStreamEvent::decode(&buf).unwrap();

        assert_eq!(event.event_time_us, 1000000);
        assert_eq!(event.book_update_id, 12345);
        assert_eq!(event.price_exponent, -2);
        assert_eq!(event.qty_exponent, -8);
        assert_eq!(event.symbol, "BTCUSDT");
        assert!((event.bid_price() - 42000.0).abs() < 0.01);
    }

    #[rstest]
    fn test_decode_truncated_header() {
        let buf = [0u8; 5];
        let err = BestBidAskStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_truncated_body() {
        let mut buf = make_valid_buffer();
        buf.truncate(40); // Truncate in the middle of body
        let err = BestBidAskStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_wrong_schema() {
        let mut buf = make_valid_buffer();
        buf[4..6].copy_from_slice(&99u16.to_le_bytes()); // Wrong schema
        let err = BestBidAskStreamEvent::decode(&buf).unwrap_err();
        assert!(matches!(err, StreamDecodeError::SchemaMismatch { .. }));
    }
}
