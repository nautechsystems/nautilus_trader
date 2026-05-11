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

use std::str::FromStr;

use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, Date64Array, Decimal128Array, Float32Array, Float64Array,
        Int32Array, Int64Array, StringArray, StringViewArray, TimestampMicrosecondArray,
        TimestampMillisecondArray, TimestampNanosecondArray, TimestampSecondArray, UInt32Array,
        UInt64Array,
    },
    datatypes::DataType,
    record_batch::RecordBatch,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    types::{Price, Quantity},
};

use crate::enums::CryptoHFTDataExchange;

/// Creates a Nautilus instrument ID from CHD exchange and raw symbol values.
#[must_use]
pub fn instrument_id(exchange: CryptoHFTDataExchange, raw_symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::new(raw_symbol), exchange.as_venue())
}

/// Returns the first matching Arrow column from `batch`.
#[must_use]
pub fn column<'a>(batch: &'a RecordBatch, names: &[&str]) -> Option<&'a ArrayRef> {
    names
        .iter()
        .find_map(|name| batch.schema().index_of(name).ok())
        .map(|idx| &batch.columns()[idx])
}

/// Returns a required Arrow column from `batch`.
///
/// # Errors
///
/// Returns an error when none of the provided names exist.
pub fn required_column<'a>(batch: &'a RecordBatch, names: &[&str]) -> anyhow::Result<&'a ArrayRef> {
    column(batch, names)
        .ok_or_else(|| anyhow::anyhow!("missing CHD column, expected one of {names:?}"))
}

/// Returns a column value as a string, preserving decimal text where possible.
#[must_use]
pub fn value_as_string(array: &ArrayRef, row: usize) -> Option<String> {
    if array.is_null(row) {
        return None;
    }

    macro_rules! downcast_value {
        ($ty:ty) => {
            if let Some(values) = array.as_any().downcast_ref::<$ty>() {
                return Some(values.value(row).to_string());
            }
        };
    }

    downcast_value!(StringArray);
    downcast_value!(StringViewArray);
    downcast_value!(Int64Array);
    downcast_value!(Int32Array);
    downcast_value!(UInt64Array);
    downcast_value!(UInt32Array);
    downcast_value!(Float64Array);
    downcast_value!(Float32Array);
    downcast_value!(BooleanArray);
    downcast_value!(TimestampNanosecondArray);
    downcast_value!(TimestampMicrosecondArray);
    downcast_value!(TimestampMillisecondArray);
    downcast_value!(TimestampSecondArray);
    downcast_value!(Date64Array);

    if let Some(values) = array.as_any().downcast_ref::<Decimal128Array>() {
        return Some(format_decimal(values.value(row), values.scale()));
    }

    None
}

/// Returns a column value as `u64`.
#[must_use]
pub fn value_as_u64(array: &ArrayRef, row: usize) -> Option<u64> {
    value_as_string(array, row)?.parse::<u64>().ok()
}

/// Returns a column value as `i64`.
#[must_use]
pub fn value_as_i64(array: &ArrayRef, row: usize) -> Option<i64> {
    value_as_string(array, row)?.parse::<i64>().ok()
}

/// Returns a column value as `bool`.
#[must_use]
pub fn value_as_bool(array: &ArrayRef, row: usize) -> Option<bool> {
    if array.is_null(row) {
        return None;
    }
    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return Some(values.value(row));
    }
    match value_as_string(array, row)?.to_ascii_lowercase().as_str() {
        "true" | "1" | "buy" | "bid" | "b" => Some(true),
        "false" | "0" | "sell" | "ask" | "a" => Some(false),
        _ => None,
    }
}

/// Parses an Arrow column value as a Nautilus price.
///
/// # Errors
///
/// Returns an error when the value is missing or not a valid price.
pub fn parse_price(array: &ArrayRef, row: usize) -> anyhow::Result<Price> {
    let value = value_as_string(array, row)
        .ok_or_else(|| anyhow::anyhow!("missing price value at row {row}"))?;
    Price::from_str(&value).map_err(|e| anyhow::anyhow!("invalid price '{value}': {e}"))
}

/// Parses an Arrow column value as a Nautilus quantity.
///
/// # Errors
///
/// Returns an error when the value is missing or not a valid quantity.
pub fn parse_quantity(array: &ArrayRef, row: usize) -> anyhow::Result<Quantity> {
    let value = value_as_string(array, row)
        .ok_or_else(|| anyhow::anyhow!("missing quantity value at row {row}"))?;
    Quantity::from_str(&value).map_err(|e| anyhow::anyhow!("invalid quantity '{value}': {e}"))
}

/// Converts a CHD timestamp integer to UNIX nanoseconds.
///
/// CHD files can contain exchange timestamps in seconds, milliseconds,
/// microseconds or nanoseconds depending on dataset/exchange. The magnitude
/// heuristic keeps the parser robust while preserving nanosecond timestamps.
#[must_use]
pub fn timestamp_to_unix_nanos(value: i64) -> UnixNanos {
    let abs_value = value.unsigned_abs();
    let nanos = if abs_value >= 100_000_000_000_000_000 {
        value
    } else if abs_value >= 100_000_000_000_000 {
        value.saturating_mul(1_000)
    } else if abs_value >= 100_000_000_000 {
        value.saturating_mul(1_000_000)
    } else {
        value.saturating_mul(1_000_000_000)
    };
    UnixNanos::from(nanos as u64)
}

/// Parses a timestamp from the first available column.
///
/// # Errors
///
/// Returns an error when no timestamp column exists or the value is invalid.
pub fn parse_timestamp(
    batch: &RecordBatch,
    row: usize,
    names: &[&str],
) -> anyhow::Result<UnixNanos> {
    let col = required_column(batch, names)?;
    let value = value_as_i64(col, row)
        .ok_or_else(|| anyhow::anyhow!("invalid timestamp at row {row} for {names:?}"))?;
    Ok(timestamp_to_unix_nanos(value))
}

/// Picks `ts_init` from received-time columns, falling back to `ts_event`.
#[must_use]
pub fn parse_ts_init_or_event(batch: &RecordBatch, row: usize, ts_event: UnixNanos) -> UnixNanos {
    column(batch, &["received_time", "local_timestamp", "ts_init"])
        .and_then(|col| value_as_i64(col, row))
        .map_or(ts_event, timestamp_to_unix_nanos)
}

/// Rescales a price to a target precision.
#[must_use]
pub fn rescale_price(price: Price, precision: u8) -> Price {
    if price.precision == precision {
        return price;
    }
    Price::new(price.as_f64(), precision)
}

/// Rescales a quantity to a target precision.
#[must_use]
pub fn rescale_quantity(quantity: Quantity, precision: u8) -> Quantity {
    if quantity.precision == precision {
        return quantity;
    }
    Quantity::new(quantity.as_f64(), precision)
}

fn format_decimal(raw: i128, scale: i8) -> String {
    if scale <= 0 {
        return raw.to_string();
    }

    let scale = scale as usize;
    let negative = raw < 0;
    let digits = raw.abs().to_string();

    let formatted = if digits.len() <= scale {
        format!("0.{digits:0>scale$}")
    } else {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    };

    if negative {
        format!("-{formatted}")
    } else {
        formatted
    }
}

/// Returns a short display name for an Arrow data type.
#[must_use]
pub fn data_type_name(data_type: &DataType) -> String {
    format!("{data_type:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_units_are_detected_by_magnitude() {
        assert_eq!(
            timestamp_to_unix_nanos(1_700_000_000).as_u64(),
            1_700_000_000_000_000_000
        );
        assert_eq!(
            timestamp_to_unix_nanos(1_700_000_000_000).as_u64(),
            1_700_000_000_000_000_000
        );
        assert_eq!(
            timestamp_to_unix_nanos(1_700_000_000_000_000).as_u64(),
            1_700_000_000_000_000_000
        );
        assert_eq!(
            timestamp_to_unix_nanos(1_700_000_000_000_000_000).as_u64(),
            1_700_000_000_000_000_000
        );
    }

    #[test]
    fn decimal_formatting_preserves_scale() {
        assert_eq!(format_decimal(12345, 2), "123.45");
        assert_eq!(format_decimal(12, 4), "0.0012");
        assert_eq!(format_decimal(-123, 2), "-1.23");
    }
}
