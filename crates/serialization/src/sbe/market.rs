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

//! Hand-written SBE codecs for Nautilus market data types.

mod bars;
mod book;
mod common;
mod data_any;
mod ticks;

use nautilus_model::data::{
    Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
    OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
};

use self::common::{HEADER_LENGTH, decode_header, encode_header, validate_header};
use super::{SbeCursor, SbeDecodeError, SbeEncodeError};

pub const MARKET_SCHEMA_ID: u16 = 1;
pub const MARKET_SCHEMA_VERSION: u16 = 0;

pub(super) mod data_any_variant {
    pub const ORDER_BOOK_DELTA: u16 = 0;
    pub const ORDER_BOOK_DELTAS: u16 = 1;
    pub const ORDER_BOOK_DEPTH10: u16 = 2;
    pub const QUOTE: u16 = 3;
    pub const TRADE: u16 = 4;
    pub const BAR: u16 = 5;
    pub const MARK_PRICE: u16 = 6;
    pub const INDEX_PRICE: u16 = 7;
    pub const FUNDING_RATE: u16 = 8;
    pub const INSTRUMENT_STATUS: u16 = 9;
    pub const INSTRUMENT_CLOSE: u16 = 10;
}

pub(super) mod template_id {
    pub const BOOK_ORDER: u16 = 30_001;
    pub const ORDER_BOOK_DELTA: u16 = 30_002;
    pub const ORDER_BOOK_DELTAS: u16 = 30_003;
    pub const ORDER_BOOK_DEPTH10: u16 = 30_004;
    pub const QUOTE_TICK: u16 = 30_005;
    pub const TRADE_TICK: u16 = 30_006;
    pub const BAR_TYPE: u16 = 30_007;
    pub const BAR: u16 = 30_008;
    pub const MARK_PRICE_UPDATE: u16 = 30_009;
    pub const INDEX_PRICE_UPDATE: u16 = 30_010;
    pub const FUNDING_RATE_UPDATE: u16 = 30_011;
    pub const INSTRUMENT_STATUS: u16 = 30_012;
    pub const INSTRUMENT_CLOSE: u16 = 30_013;
    pub const DATA_ANY: u16 = 30_014;
}

#[expect(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum DataAny {
    OrderBookDelta(OrderBookDelta),
    OrderBookDeltas(OrderBookDeltas),
    OrderBookDepth10(OrderBookDepth10),
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    FundingRate(FundingRateUpdate),
    InstrumentStatus(InstrumentStatus),
    InstrumentClose(InstrumentClose),
}

pub trait ToSbe {
    /// Encodes the value into an SBE message buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if any field cannot be encoded into the target SBE wire format.
    fn to_sbe(&self) -> Result<Vec<u8>, SbeEncodeError>;

    /// Encodes the value into the provided SBE message buffer.
    ///
    /// This method clears any existing bytes in `buf` before encoding.
    ///
    /// # Errors
    ///
    /// Returns an error if any field cannot be encoded into the target SBE wire format.
    fn to_sbe_into(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        let bytes = self.to_sbe()?;
        buf.clear();
        buf.extend_from_slice(&bytes);
        Ok(())
    }
}

pub trait FromSbe: Sized {
    /// Decodes the value from an SBE message buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the header is invalid or the payload is malformed.
    fn from_sbe(bytes: &[u8]) -> Result<Self, SbeDecodeError>;
}

pub(super) trait MarketSbeMessage: Sized {
    const TEMPLATE_ID: u16;
    const BLOCK_LENGTH: u16;

    fn encode_body(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError>;

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError>;

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
    }
}

impl<T> ToSbe for T
where
    T: MarketSbeMessage,
{
    #[inline]
    fn to_sbe(&self) -> Result<Vec<u8>, SbeEncodeError> {
        let encoded_size = HEADER_LENGTH + self.encoded_body_size();
        let mut buf = Vec::with_capacity(encoded_size);
        encode_market_message(self, &mut buf, encoded_size)?;
        Ok(buf)
    }

    #[inline]
    fn to_sbe_into(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        let encoded_size = HEADER_LENGTH + self.encoded_body_size();
        encode_market_message(self, buf, encoded_size)
    }
}

impl<T> FromSbe for T
where
    T: MarketSbeMessage,
{
    #[inline]
    fn from_sbe(bytes: &[u8]) -> Result<Self, SbeDecodeError> {
        let mut cursor = SbeCursor::new(bytes);
        let header = decode_header(&mut cursor)?;
        validate_header(&header, T::TEMPLATE_ID, T::BLOCK_LENGTH)?;
        T::decode_body(&mut cursor)
    }
}

#[inline]
fn encode_market_message<T>(
    value: &T,
    buf: &mut Vec<u8>,
    encoded_size: usize,
) -> Result<(), SbeEncodeError>
where
    T: MarketSbeMessage,
{
    buf.clear();

    if buf.capacity() < encoded_size {
        buf.reserve(encoded_size - buf.capacity());
    }

    encode_header(
        buf,
        T::BLOCK_LENGTH,
        T::TEMPLATE_ID,
        MARKET_SCHEMA_ID,
        MARKET_SCHEMA_VERSION,
    );
    value.encode_body(buf)?;
    debug_assert_eq!(buf.len(), encoded_size);
    Ok(())
}
