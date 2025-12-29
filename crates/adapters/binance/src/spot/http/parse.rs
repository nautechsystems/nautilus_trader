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

//! SBE decode functions for Binance Spot HTTP responses.
//!
//! Each function decodes raw SBE bytes into domain types, validating the
//! message header (schema ID, version, template ID) before extracting fields.

use super::{
    error::SbeDecodeError,
    models::{BinanceDepth, BinancePriceLevel, BinanceTrade, BinanceTrades},
};
use crate::common::sbe::spot::{
    SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, bool_enum::BoolEnum,
    depth_response_codec::SBE_TEMPLATE_ID as DEPTH_TEMPLATE_ID,
    message_header_codec::ENCODED_LENGTH as HEADER_LENGTH,
    ping_response_codec::SBE_TEMPLATE_ID as PING_TEMPLATE_ID,
    server_time_response_codec::SBE_TEMPLATE_ID as SERVER_TIME_TEMPLATE_ID,
    trades_response_codec::SBE_TEMPLATE_ID as TRADES_TEMPLATE_ID,
};

/// Group size encoding length (u16 block_length + u32 num_in_group).
const GROUP_SIZE_LENGTH: usize = 6;

/// Maximum allowed group size to prevent OOM from malicious payloads.
const MAX_GROUP_SIZE: u32 = 10_000;

/// SBE message header.
#[derive(Debug, Clone, Copy)]
struct MessageHeader {
    #[allow(dead_code)]
    block_length: u16,
    template_id: u16,
    schema_id: u16,
    version: u16,
}

impl MessageHeader {
    /// Decode message header from buffer.
    fn decode(buf: &[u8]) -> Result<Self, SbeDecodeError> {
        if buf.len() < HEADER_LENGTH {
            return Err(SbeDecodeError::BufferTooShort {
                expected: HEADER_LENGTH,
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

    /// Validate schema ID and version.
    fn validate(&self) -> Result<(), SbeDecodeError> {
        if self.schema_id != SBE_SCHEMA_ID {
            return Err(SbeDecodeError::SchemaMismatch {
                expected: SBE_SCHEMA_ID,
                actual: self.schema_id,
            });
        }
        if self.version != SBE_SCHEMA_VERSION {
            return Err(SbeDecodeError::VersionMismatch {
                expected: SBE_SCHEMA_VERSION,
                actual: self.version,
            });
        }
        Ok(())
    }
}

/// Decode a ping response.
///
/// Ping response has no body (block_length = 0), just validates the header.
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
pub fn decode_ping(buf: &[u8]) -> Result<(), SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != PING_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    Ok(())
}

/// Decode a server time response.
///
/// Returns the server time as **microseconds** since epoch (SBE provides
/// microsecond precision vs JSON's milliseconds).
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
///
/// # Panics
///
/// This function will not panic as buffer lengths are validated before slicing.
pub fn decode_server_time(buf: &[u8]) -> Result<i64, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != SERVER_TIME_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let body_start = HEADER_LENGTH;
    let body_end = body_start + 8;

    if buf.len() < body_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: body_end,
            actual: buf.len(),
        });
    }

    // SAFETY: Length validated above
    let server_time = i64::from_le_bytes(buf[body_start..body_end].try_into().expect("slice len"));
    Ok(server_time)
}

/// Decode a depth response.
///
/// Returns the order book depth with bids and asks.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
///
/// # Panics
///
/// This function will not panic as buffer lengths are validated before slicing.
pub fn decode_depth(buf: &[u8]) -> Result<BinanceDepth, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != DEPTH_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Depth block: last_update_id (8) + price_exponent (1) + qty_exponent (1) = 10 bytes
    let block_start = HEADER_LENGTH;
    let block_end = block_start + 10;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    // SAFETY: Length validated above
    let last_update_id = i64::from_le_bytes(
        buf[block_start..block_start + 8]
            .try_into()
            .expect("slice len"),
    );
    let price_exponent = buf[block_start + 8] as i8;
    let qty_exponent = buf[block_start + 9] as i8;

    let (bids, bids_end) = decode_price_levels(&buf[block_end..])?;
    let (asks, _asks_end) = decode_price_levels(&buf[block_end + bids_end..])?;

    Ok(BinanceDepth {
        last_update_id,
        price_exponent,
        qty_exponent,
        bids,
        asks,
    })
}

/// Decode a trades response.
///
/// Returns the list of trades.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
pub fn decode_trades(buf: &[u8]) -> Result<BinanceTrades, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != TRADES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Trades block: price_exponent (1) + qty_exponent (1) = 2 bytes
    let block_start = HEADER_LENGTH;
    let block_end = block_start + 2;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    let price_exponent = buf[block_start] as i8;
    let qty_exponent = buf[block_start + 1] as i8;

    let trades = decode_trades_group(&buf[block_end..])?;

    Ok(BinanceTrades {
        price_exponent,
        qty_exponent,
        trades,
    })
}

/// Decode a group of price levels (bids or asks).
///
/// Returns (levels, bytes_consumed).
fn decode_price_levels(buf: &[u8]) -> Result<(Vec<BinancePriceLevel>, usize), SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let _block_length = u16::from_le_bytes([buf[0], buf[1]]);
    let count = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

    if count > MAX_GROUP_SIZE {
        return Err(SbeDecodeError::GroupSizeTooLarge {
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    let level_size = 16; // price (8) + qty (8)
    let total_size = GROUP_SIZE_LENGTH + (count as usize * level_size);

    if buf.len() < total_size {
        return Err(SbeDecodeError::BufferTooShort {
            expected: total_size,
            actual: buf.len(),
        });
    }

    let mut levels = Vec::with_capacity(count as usize);
    let mut offset = GROUP_SIZE_LENGTH;

    for _ in 0..count {
        let price_mantissa = i64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());

        levels.push(BinancePriceLevel {
            price_mantissa,
            qty_mantissa,
        });

        offset += level_size;
    }

    Ok((levels, total_size))
}

/// Decode a group of trades.
fn decode_trades_group(buf: &[u8]) -> Result<Vec<BinanceTrade>, SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let _block_length = u16::from_le_bytes([buf[0], buf[1]]);
    let count = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

    if count > MAX_GROUP_SIZE {
        return Err(SbeDecodeError::GroupSizeTooLarge {
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    // Trade: id(8) + price(8) + qty(8) + quoteQty(8) + time(8) + isBuyerMaker(1) + isBestMatch(1) = 42
    let trade_size = 42;
    let total_size = GROUP_SIZE_LENGTH + (count as usize * trade_size);

    if buf.len() < total_size {
        return Err(SbeDecodeError::BufferTooShort {
            expected: total_size,
            actual: buf.len(),
        });
    }

    let mut trades = Vec::with_capacity(count as usize);
    let mut offset = GROUP_SIZE_LENGTH;

    for _ in 0..count {
        let id = i64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
        let price_mantissa = i64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 16..offset + 24].try_into().unwrap());
        let quote_qty_mantissa =
            i64::from_le_bytes(buf[offset + 24..offset + 32].try_into().unwrap());
        let time = i64::from_le_bytes(buf[offset + 32..offset + 40].try_into().unwrap());
        let is_buyer_maker = BoolEnum::from(buf[offset + 40]) == BoolEnum::True;
        let is_best_match = BoolEnum::from(buf[offset + 41]) == BoolEnum::True;

        trades.push(BinanceTrade {
            id,
            price_mantissa,
            qty_mantissa,
            quote_qty_mantissa,
            time,
            is_buyer_maker,
            is_best_match,
        });

        offset += trade_size;
    }

    Ok(trades)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_header(block_length: u16, template_id: u16, schema_id: u16, version: u16) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..4].copy_from_slice(&template_id.to_le_bytes());
        buf[4..6].copy_from_slice(&schema_id.to_le_bytes());
        buf[6..8].copy_from_slice(&version.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_ping_valid() {
        // Ping: block_length=0, template_id=101, schema_id=3, version=1
        let buf = create_header(0, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        assert!(decode_ping(&buf).is_ok());
    }

    #[rstest]
    fn test_decode_ping_buffer_too_short() {
        let buf = [0u8; 4];
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_ping_schema_mismatch() {
        let buf = create_header(0, PING_TEMPLATE_ID, 99, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::SchemaMismatch { .. }));
    }

    #[rstest]
    fn test_decode_ping_wrong_template() {
        let buf = create_header(0, 999, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(999)));
    }

    #[rstest]
    fn test_decode_server_time_valid() {
        // ServerTime: block_length=8, template_id=102, schema_id=3, version=1
        let header = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let timestamp: i64 = 1734300000000; // Example timestamp

        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&timestamp.to_le_bytes());

        let result = decode_server_time(&buf).unwrap();
        assert_eq!(result, timestamp);
    }

    #[rstest]
    fn test_decode_server_time_buffer_too_short() {
        // Header only, missing body
        let buf = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_server_time_wrong_template() {
        let header = create_header(8, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_server_time_version_mismatch() {
        let header = create_header(8, SERVER_TIME_TEMPLATE_ID, SBE_SCHEMA_ID, 99);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::VersionMismatch { .. }));
    }

    fn create_group_header(block_length: u16, count: u32) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..6].copy_from_slice(&count.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_depth_valid() {
        // Depth: block_length=10, template_id=200
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: last_update_id (8) + price_exponent (1) + qty_exponent (1)
        let last_update_id: i64 = 123456789;
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.extend_from_slice(&last_update_id.to_le_bytes());
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Bids group: 2 levels
        buf.extend_from_slice(&create_group_header(16, 2));
        // Bid 1: price=100000000000, qty=50000000
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes());
        buf.extend_from_slice(&50_000_000i64.to_le_bytes());
        // Bid 2: price=99900000000, qty=30000000
        buf.extend_from_slice(&99_900_000_000i64.to_le_bytes());
        buf.extend_from_slice(&30_000_000i64.to_le_bytes());

        // Asks group: 1 level
        buf.extend_from_slice(&create_group_header(16, 1));
        // Ask 1: price=100100000000, qty=25000000
        buf.extend_from_slice(&100_100_000_000i64.to_le_bytes());
        buf.extend_from_slice(&25_000_000i64.to_le_bytes());

        let depth = decode_depth(&buf).unwrap();

        assert_eq!(depth.last_update_id, 123456789);
        assert_eq!(depth.price_exponent, -8);
        assert_eq!(depth.qty_exponent, -8);
        assert_eq!(depth.bids.len(), 2);
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.bids[0].price_mantissa, 100_000_000_000);
        assert_eq!(depth.bids[0].qty_mantissa, 50_000_000);
        assert_eq!(depth.asks[0].price_mantissa, 100_100_000_000);
    }

    #[rstest]
    fn test_decode_depth_empty_book() {
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes()); // last_update_id
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty bids
        buf.extend_from_slice(&create_group_header(16, 0));
        // Empty asks
        buf.extend_from_slice(&create_group_header(16, 0));

        let depth = decode_depth(&buf).unwrap();

        assert!(depth.bids.is_empty());
        assert!(depth.asks.is_empty());
    }

    #[rstest]
    fn test_decode_trades_valid() {
        // Trades: block_length=2, template_id=201
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: price_exponent (1) + qty_exponent (1)
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Trades group: 1 trade (42 bytes each)
        buf.extend_from_slice(&create_group_header(42, 1));

        // Trade: id(8) + price(8) + qty(8) + quoteQty(8) + time(8) + isBuyerMaker(1) + isBestMatch(1)
        let trade_id: i64 = 999;
        let price: i64 = 100_000_000_000;
        let qty: i64 = 10_000_000;
        let quote_qty: i64 = 1_000_000_000_000;
        let time: i64 = 1734300000000;
        let is_buyer_maker: u8 = 1; // true
        let is_best_match: u8 = 1; // true

        buf.extend_from_slice(&trade_id.to_le_bytes());
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
        buf.extend_from_slice(&quote_qty.to_le_bytes());
        buf.extend_from_slice(&time.to_le_bytes());
        buf.push(is_buyer_maker);
        buf.push(is_best_match);

        let trades = decode_trades(&buf).unwrap();

        assert_eq!(trades.price_exponent, -8);
        assert_eq!(trades.qty_exponent, -8);
        assert_eq!(trades.trades.len(), 1);
        assert_eq!(trades.trades[0].id, 999);
        assert_eq!(trades.trades[0].price_mantissa, 100_000_000_000);
        assert!(trades.trades[0].is_buyer_maker);
        assert!(trades.trades[0].is_best_match);
    }

    #[rstest]
    fn test_decode_trades_empty() {
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty trades group
        buf.extend_from_slice(&create_group_header(42, 0));

        let trades = decode_trades(&buf).unwrap();

        assert!(trades.trades.is_empty());
    }

    #[rstest]
    fn test_decode_depth_wrong_template() {
        let header = create_header(10, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 10]); // dummy block

        let err = decode_depth(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_trades_wrong_template() {
        let header = create_header(2, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 2]); // dummy block

        let err = decode_trades(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }
}
