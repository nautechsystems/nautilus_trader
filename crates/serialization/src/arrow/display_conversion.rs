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

//! Shared field definitions and numeric conversions for display Arrow batches.

use arrow::datatypes::{DataType, Field};
use nautilus_model::types::{Price, Quantity, fixed::MAX_FLOAT_PRECISION};
use rust_decimal::prelude::ToPrimitive;

use super::timestamp_data_type;

/// Upper bound on precision the display encoders accept. Values above this are
/// treated as pathological sentinels (most notably `ERROR_PRICE`, which carries
/// `precision: 255`) and emit `NaN`. Legitimate high-precision inputs top out at
/// `nautilus_model::defi::WEI_PRECISION` (18).
pub(super) const DISPLAY_MAX_PRECISION: u8 = 18;

/// Builds a `Utf8` field with the given name and nullability.
pub(super) fn utf8_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable)
}

/// Builds a `Float64` field with the given name and nullability.
pub(super) fn float64_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Float64, nullable)
}

/// Builds a `Timestamp(Nanosecond, Some("UTC"))` field with the given name and nullability.
pub(super) fn timestamp_field(name: &str, nullable: bool) -> Field {
    Field::new(name, timestamp_data_type(), nullable)
}

/// Converts a [`Price`] to `f64` for display without panicking.
///
/// Returns [`f64::NAN`] for sentinel values (`PRICE_UNDEF`, `PRICE_ERROR`,
/// and the `ERROR_PRICE` synthetic with `precision: 255`), so clear-style
/// order book deltas and error sentinels render as missing cells instead of
/// bogus numeric values. [`Price::as_f64`] panics when the `defi` feature is
/// enabled and precision exceeds [`MAX_FLOAT_PRECISION`] (16), so the conversion
/// falls back to [`rust_decimal::Decimal`] in that range. The
/// decimal path returns [`f64::NAN`] if the value is outside `f64` range.
pub(super) fn price_to_f64(price: &Price) -> f64 {
    if price.is_undefined() || price.is_error() || price.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if price.precision <= MAX_FLOAT_PRECISION {
        price.as_f64()
    } else {
        price.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

/// Converts a [`Quantity`] to `f64` for display without panicking.
///
/// See [`price_to_f64`] for the rationale. Returns [`f64::NAN`] for the
/// `QUANTITY_UNDEF` sentinel and for pathological precisions.
pub(super) fn quantity_to_f64(quantity: &Quantity) -> f64 {
    if quantity.is_undefined() || quantity.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if quantity.precision <= MAX_FLOAT_PRECISION {
        quantity.as_f64()
    } else {
        quantity.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}
