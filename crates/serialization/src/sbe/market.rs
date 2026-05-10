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
use super::{SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter};

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

/// Extension of [`FromSbe`] that reuses allocations between decodes.
///
/// Scalar messages decode without heap allocation, so they do not need this trait. Types with
/// growable internal buffers (for example [`OrderBookDeltas`] with its `Vec<OrderBookDelta>`)
/// implement it to let callers supply a pre-allocated scratch buffer and avoid per-message
/// allocation in hot paths.
pub trait FromSbeReuse: FromSbe {
    /// Scratch buffer whose allocation is reused across decodes.
    type Scratch;

    /// Decodes a value from an SBE message buffer, reusing `scratch`'s allocation.
    ///
    /// On success, ownership of the allocation moves from `scratch` into the returned value and
    /// `scratch` is left in its empty state. To continue reusing the allocation, move the buffer
    /// back from the returned value (for example `scratch = std::mem::take(&mut result.deltas)`).
    ///
    /// # Errors
    ///
    /// Returns an error if the header is invalid or the payload is malformed.
    fn from_sbe_reuse(bytes: &[u8], scratch: &mut Self::Scratch) -> Result<Self, SbeDecodeError>;
}

pub(super) trait MarketSbeMessage: Sized {
    const TEMPLATE_ID: u16;
    const BLOCK_LENGTH: u16;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError>;

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
        encode_into_uninit(self, &mut buf, encoded_size)?;
        Ok(buf)
    }

    #[inline]
    fn to_sbe_into(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        let encoded_size = HEADER_LENGTH + self.encoded_body_size();
        buf.clear();
        buf.reserve(encoded_size);
        encode_into_uninit(self, buf, encoded_size)
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
        validate_header(header, T::TEMPLATE_ID, T::BLOCK_LENGTH)?;
        T::decode_body(&mut cursor)
    }
}

// Writes an SBE message into the spare capacity of `buf` without zero
// initialization, then commits the length on success. Caller must ensure
// `buf.len() == 0` and `buf.capacity() >= encoded_size`.
#[inline]
#[allow(
    unsafe_code,
    reason = "set_len commits writes the SbeWriter has already made into spare capacity"
)]
#[allow(
    clippy::panic_in_result_fn,
    reason = "load-bearing safety check for the unsafe set_len; panic is the right outcome"
)]
fn encode_into_uninit<T>(
    value: &T,
    buf: &mut Vec<u8>,
    encoded_size: usize,
) -> Result<(), SbeEncodeError>
where
    T: MarketSbeMessage,
{
    debug_assert_eq!(buf.len(), 0);
    debug_assert!(buf.capacity() >= encoded_size);

    let spare = &mut buf.spare_capacity_mut()[..encoded_size];
    let mut writer = SbeWriter::new_uninit(spare);
    encode_header(
        &mut writer,
        T::BLOCK_LENGTH,
        T::TEMPLATE_ID,
        MARKET_SCHEMA_ID,
        MARKET_SCHEMA_VERSION,
    );
    value.encode_body(&mut writer)?;

    // Load-bearing for the unsafe `set_len` below: this is the invariant that
    // converts the writer's per-byte initialization into Vec-level safety. Run
    // in release builds too so a future size mismatch panics rather than
    // commits uninit bytes.
    assert_eq!(
        writer.pos(),
        encoded_size,
        "SBE encode_body wrote {} bytes but encoded_body_size reported {}",
        writer.pos(),
        encoded_size,
    );

    // SAFETY: the writer panics if it attempts to write past `encoded_size`,
    // the assert above confirms it wrote exactly `encoded_size` bytes, and
    // errors propagate before `set_len` runs. Reaching this line means the
    // first `encoded_size` bytes of `buf` hold initialized u8 values.
    unsafe {
        buf.set_len(encoded_size);
    }
    Ok(())
}
