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

use std::num::NonZero;

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, FromU8, FromU16,
        InstrumentCloseType, MarketStatusAction, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, Symbol, Venue},
    types::{Price, Quantity, fixed::FIXED_PRECISION, price::PriceRaw, quantity::QuantityRaw},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    super::{MAX_GROUP_SIZE, SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter},
    MARKET_SCHEMA_ID, MARKET_SCHEMA_VERSION,
};

pub(super) const PRICE_BLOCK_LENGTH: u16 = 17;
pub(super) const QUANTITY_BLOCK_LENGTH: u16 = 17;
pub(super) const DECIMAL_BLOCK_LENGTH: u16 = 16;
pub(super) const BAR_TYPE_BLOCK_LENGTH: u16 = 7;
pub(super) const BOOK_ORDER_BLOCK_LENGTH: u16 = 43;
pub(super) const ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH: u16 = 69;
pub(super) const DEPTH10_LEVEL_BLOCK_LENGTH: u16 = 34;
pub(super) const DEPTH10_LEVEL_COUNT: usize = 10;
pub(super) const DEPTH10_COUNTS_BLOCK_LENGTH: usize =
    DEPTH10_LEVEL_COUNT * std::mem::size_of::<u32>();
pub(super) const HEADER_LENGTH: usize = 8;
pub(super) const GROUP_HEADER_16_LENGTH: usize = 4;

const VAR_STRING16_MAX_LEN: usize = (u16::MAX - 1) as usize;

#[derive(Debug, Clone, Copy)]
pub(super) struct MessageHeader {
    pub block_length: u16,
    pub template_id: u16,
    pub schema_id: u16,
    pub version: u16,
}

#[inline]
pub(super) fn encode_header(
    writer: &mut SbeWriter<'_>,
    block_length: u16,
    template_id: u16,
    schema_id: u16,
    version: u16,
) {
    writer.write_u16_le(block_length);
    writer.write_u16_le(template_id);
    writer.write_u16_le(schema_id);
    writer.write_u16_le(version);
}

#[inline]
pub(super) fn decode_header(cursor: &mut SbeCursor<'_>) -> Result<MessageHeader, SbeDecodeError> {
    cursor.require(HEADER_LENGTH)?;
    Ok(MessageHeader {
        block_length: cursor.read_u16_le()?,
        template_id: cursor.read_u16_le()?,
        schema_id: cursor.read_u16_le()?,
        version: cursor.read_u16_le()?,
    })
}

#[inline]
pub(super) fn validate_header(
    header: MessageHeader,
    expected_template_id: u16,
    expected_block_length: u16,
) -> Result<(), SbeDecodeError> {
    if header.block_length != expected_block_length {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: expected_block_length,
            actual: header.block_length,
        });
    }

    if header.template_id != expected_template_id {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    if header.schema_id != MARKET_SCHEMA_ID {
        return Err(SbeDecodeError::SchemaMismatch {
            expected: MARKET_SCHEMA_ID,
            actual: header.schema_id,
        });
    }

    if header.version != MARKET_SCHEMA_VERSION {
        return Err(SbeDecodeError::VersionMismatch {
            expected: MARKET_SCHEMA_VERSION,
            actual: header.version,
        });
    }

    Ok(())
}

#[inline]
pub(super) fn encode_price(writer: &mut SbeWriter<'_>, price: &Price) {
    #[allow(clippy::useless_conversion)]
    let raw = i128::from(price.raw);

    writer.write_i128_le(raw);
    writer.write_u8(price.precision);
}

#[inline]
pub(super) fn decode_price(cursor: &mut SbeCursor<'_>) -> Result<Price, SbeDecodeError> {
    let raw_i128 = cursor.read_i128_le()?;
    let precision = cursor.read_u8()?;
    validate_precision("Price.precision", precision)?;

    #[cfg(not(feature = "high-precision"))]
    let raw = i64::try_from(raw_i128)
        .map_err(|_| SbeDecodeError::NumericOverflow { type_name: "Price" })?;

    #[cfg(feature = "high-precision")]
    let raw = raw_i128;

    Ok(Price::from_raw(raw as PriceRaw, precision))
}

#[inline]
pub(super) fn encode_quantity(writer: &mut SbeWriter<'_>, quantity: &Quantity) {
    #[allow(clippy::useless_conversion)]
    let raw = u128::from(quantity.raw);

    writer.write_u128_le(raw);
    writer.write_u8(quantity.precision);
}

#[inline]
pub(super) fn decode_quantity(cursor: &mut SbeCursor<'_>) -> Result<Quantity, SbeDecodeError> {
    let raw_u128 = cursor.read_u128_le()?;
    let precision = cursor.read_u8()?;
    validate_precision("Quantity.precision", precision)?;

    #[cfg(not(feature = "high-precision"))]
    let raw = u64::try_from(raw_u128).map_err(|_| SbeDecodeError::NumericOverflow {
        type_name: "Quantity",
    })?;

    #[cfg(feature = "high-precision")]
    let raw = raw_u128;

    Ok(Quantity::from_raw(raw as QuantityRaw, precision))
}

#[inline]
pub(super) fn encode_decimal(writer: &mut SbeWriter<'_>, value: &Decimal) {
    writer.write_bytes(&value.serialize());
}

pub(super) fn decode_decimal(cursor: &mut SbeCursor<'_>) -> Result<Decimal, SbeDecodeError> {
    let bytes = cursor.read_bytes(DECIMAL_BLOCK_LENGTH as usize)?;
    let bytes: [u8; DECIMAL_BLOCK_LENGTH as usize] = bytes
        .try_into()
        .map_err(|_| SbeDecodeError::InvalidValue { field: "Decimal" })?;
    Ok(Decimal::deserialize(bytes))
}

#[inline]
pub(super) fn encode_unix_nanos(writer: &mut SbeWriter<'_>, value: UnixNanos) {
    writer.write_u64_le(*value);
}

#[inline]
pub(super) fn decode_unix_nanos(cursor: &mut SbeCursor<'_>) -> Result<UnixNanos, SbeDecodeError> {
    Ok(cursor.read_u64_le()?.into())
}

#[inline]
pub(super) fn encode_instrument_id(
    writer: &mut SbeWriter<'_>,
    instrument_id: &InstrumentId,
) -> Result<(), SbeEncodeError> {
    encode_var_string16(writer, "InstrumentId.symbol", instrument_id.symbol.as_str())?;
    encode_var_string16(writer, "InstrumentId.venue", instrument_id.venue.as_str())
}

#[inline]
pub(super) fn decode_instrument_id(
    cursor: &mut SbeCursor<'_>,
) -> Result<InstrumentId, SbeDecodeError> {
    let symbol = Symbol::new(cursor.read_var_string16_ref()?);
    let venue = Venue::new(cursor.read_var_string16_ref()?);
    Ok(InstrumentId::new(symbol, venue))
}

// Sentinel length that marks a var-string16 slot as absent (None)
const VAR_STRING16_NULL: u16 = u16::MAX;

#[inline]
pub(super) fn encode_optional_ustr(
    writer: &mut SbeWriter<'_>,
    field: &'static str,
    value: Option<Ustr>,
) -> Result<(), SbeEncodeError> {
    match value {
        None => {
            writer.write_u16_le(VAR_STRING16_NULL);
            Ok(())
        }
        Some(s) => encode_var_string16(writer, field, s.as_str()),
    }
}

pub(super) fn decode_optional_ustr(
    cursor: &mut SbeCursor<'_>,
) -> Result<Option<Ustr>, SbeDecodeError> {
    let len = cursor.read_u16_le()?;
    if len == VAR_STRING16_NULL {
        return Ok(None);
    }

    if len == 0 {
        return Ok(Some(Ustr::from("")));
    }
    let bytes = cursor.read_bytes(usize::from(len))?;
    let s = std::str::from_utf8(bytes).map_err(|_| SbeDecodeError::InvalidUtf8)?;
    Ok(Some(Ustr::from(s)))
}

#[inline]
pub(super) fn encode_group_header_16(
    writer: &mut SbeWriter<'_>,
    group: &'static str,
    count: usize,
    block_length: u16,
) -> Result<(), SbeEncodeError> {
    if count > MAX_GROUP_SIZE as usize {
        return Err(SbeEncodeError::GroupSizeTooLarge {
            group,
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    let count_u16 = u16::try_from(count).map_err(|_| SbeEncodeError::GroupSizeTooLarge {
        group,
        count,
        max: u16::MAX as u32,
    })?;

    writer.write_u16_le(block_length);
    writer.write_u16_le(count_u16);
    Ok(())
}

#[inline]
pub(super) fn encode_var_string16(
    writer: &mut SbeWriter<'_>,
    field: &'static str,
    value: &str,
) -> Result<(), SbeEncodeError> {
    let len = value.len();
    if len > VAR_STRING16_MAX_LEN {
        return Err(SbeEncodeError::StringTooLong {
            field,
            len,
            max: VAR_STRING16_MAX_LEN,
        });
    }

    writer.write_u16_le(len as u16);
    writer.write_bytes(value.as_bytes());
    Ok(())
}

#[inline]
pub(super) fn encoded_instrument_id_size(instrument_id: &InstrumentId) -> usize {
    encoded_var_string16_size(instrument_id.symbol.as_str())
        + encoded_var_string16_size(instrument_id.venue.as_str())
}

#[inline]
pub(super) fn encoded_optional_ustr_size(value: Option<Ustr>) -> usize {
    match value {
        None => std::mem::size_of::<u16>(),
        Some(s) => encoded_var_string16_size(s.as_str()),
    }
}

#[inline]
pub(super) fn encoded_var_string16_size(value: &str) -> usize {
    std::mem::size_of::<u16>() + value.len()
}

pub(super) fn validate_precision(field: &'static str, precision: u8) -> Result<(), SbeDecodeError> {
    if precision > FIXED_PRECISION {
        return Err(SbeDecodeError::InvalidValue { field });
    }
    Ok(())
}

// Sentinel byte that marks an optional bool as absent (None)
const OPTIONAL_BOOL_NULL: u8 = 0xFF;

#[must_use]
pub(super) fn encode_optional_bool(value: Option<bool>) -> u8 {
    match value {
        None => OPTIONAL_BOOL_NULL,
        Some(true) => 1,
        Some(false) => 0,
    }
}

pub(super) fn decode_optional_bool(
    cursor: &mut SbeCursor<'_>,
    field: &'static str,
) -> Result<Option<bool>, SbeDecodeError> {
    match cursor.read_u8()? {
        0 => Ok(Some(false)),
        1 => Ok(Some(true)),
        OPTIONAL_BOOL_NULL => Ok(None),
        _ => Err(SbeDecodeError::InvalidValue { field }),
    }
}

#[inline]
pub(super) fn decode_aggressor_side(
    cursor: &mut SbeCursor<'_>,
) -> Result<AggressorSide, SbeDecodeError> {
    let value = cursor.read_u8()?;
    AggressorSide::from_u8(value).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "AggressorSide",
        value: u16::from(value),
    })
}

#[inline]
pub(super) fn decode_book_action(cursor: &mut SbeCursor<'_>) -> Result<BookAction, SbeDecodeError> {
    let value = cursor.read_u8()?;
    BookAction::from_u8(value).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "BookAction",
        value: u16::from(value),
    })
}

#[inline]
pub(super) fn decode_order_side(cursor: &mut SbeCursor<'_>) -> Result<OrderSide, SbeDecodeError> {
    let value = cursor.read_u8()?;
    OrderSide::from_u8(value).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "OrderSide",
        value: u16::from(value),
    })
}

pub(super) fn decode_instrument_close_type(
    cursor: &mut SbeCursor<'_>,
) -> Result<InstrumentCloseType, SbeDecodeError> {
    let value = cursor.read_u8()?;
    InstrumentCloseType::from_u8(value).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "InstrumentCloseType",
        value: u16::from(value),
    })
}

pub(super) fn decode_market_status_action(
    cursor: &mut SbeCursor<'_>,
) -> Result<MarketStatusAction, SbeDecodeError> {
    let value = cursor.read_u16_le()?;
    MarketStatusAction::from_u16(value).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "MarketStatusAction",
        value,
    })
}

pub(super) fn decode_aggregation_source(
    cursor: &mut SbeCursor<'_>,
) -> Result<AggregationSource, SbeDecodeError> {
    let value = cursor.read_u8()?;
    AggregationSource::from_repr(value as usize).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "AggregationSource",
        value: u16::from(value),
    })
}

pub(super) fn decode_bar_aggregation(
    cursor: &mut SbeCursor<'_>,
) -> Result<BarAggregation, SbeDecodeError> {
    let value = cursor.read_u8()?;
    BarAggregation::from_repr(value as usize).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "BarAggregation",
        value: u16::from(value),
    })
}

pub(super) fn decode_price_type(cursor: &mut SbeCursor<'_>) -> Result<PriceType, SbeDecodeError> {
    let value = cursor.read_u8()?;
    PriceType::from_repr(value as usize).ok_or(SbeDecodeError::InvalidEnumValue {
        type_name: "PriceType",
        value: u16::from(value),
    })
}

pub(super) fn decode_non_zero_step(step_raw: u32) -> Result<NonZero<usize>, SbeDecodeError> {
    let step = usize::try_from(step_raw).map_err(|_| SbeDecodeError::NumericOverflow {
        type_name: "BarSpecification.step",
    })?;
    NonZero::new(step).ok_or(SbeDecodeError::InvalidValue {
        field: "BarSpecification.step",
    })
}
