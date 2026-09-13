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

//! Procedural macros for Nautilus model and serialization types.

#![warn(clippy::pedantic)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]

mod model;
mod serialization;
mod support;

use proc_macro::TokenStream;

/// Expands a named-field struct into a Serde-backed custom data type.
///
/// Generates the required Serde derives, `HasTsInit`, `CustomDataTrait`, `Data` conversions,
/// constructor, display implementation, and optional PyO3 bindings. The struct must contain
/// `ts_event` and `ts_init` fields.
///
/// Use `#[custom_data(pyo3)]` to generate Python bindings.
/// Use `#[custom_data(pyo3, no_display)]` to provide a custom display implementation.
/// Use `stub_module = "nautilus_trader.<module>"` with `pyo3` to emit stub metadata.
/// Use `#[custom_data_field(serde)]` for fields requiring the generic Serde Python bridge.
#[proc_macro_attribute]
pub fn custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    model::expand_custom_data(attr.into(), item.into()).into()
}

/// Adds Arrow schema, encoding, and decoding implementations to a custom data struct.
///
/// Apply this above `#[nautilus_model::custom_data]` for macro-generated model behavior, or use it
/// alone when the model traits are implemented manually. Call
/// `nautilus_serialization::ensure_custom_data_registered::<T>()` before catalog or streaming
/// serialization.
///
/// Supported field types include `InstrumentId`, `AccountId`, `Currency`, `BarType`, `Params`,
/// `Price`, `Quantity`, `Decimal`, optional prices and quantities, `UnixNanos`, `f64`, `f32`, `bool`,
/// `String`, `u64`, `i64`, `u32`, `i32`, `Vec<f64>`, and `Vec<u8>`.
/// Prices and quantities use nullable `Decimal128(38, 16)` columns. `Vec<u8>` is the deliberate
/// opaque-byte escape hatch for custom schemas.
/// `Decimal` values preserve their numeric value at scale 16, but not the source value's
/// trailing-zero scale.
/// Use `#[custom_data_field(native_enum)]` on a native enum that implements `Display` and
/// `FromStr` to encode it as a compact dictionary of display names.
/// Use `#[custom_data_field(serde)]` on a field to store any Serde serializable field as a
/// Serde JSON-backed Arrow `Utf8` column. Python field access for such fields comes from
/// `nautilus_model`'s `custom_data` macro, not this macro.
///
/// Use `#[arrow_custom_data(pyo3)]` to generate `PyArrow` `encode_record_batch_py` and
/// `decode_record_batch_py` methods; this is independent from the model macro's `pyo3` option.
/// Use `stub_module = "nautilus_trader.<module>"` with `pyo3` to emit pyo3-stub-gen metadata.
#[proc_macro_attribute]
pub fn arrow_custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    serialization::expand_arrow_custom_data(attr.into(), item.into()).into()
}
