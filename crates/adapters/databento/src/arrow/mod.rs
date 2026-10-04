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

//! Apache Arrow schema and encoding/decoding for Databento types.

pub mod imbalance;
pub mod statistics;

use std::{collections::HashMap, fmt::Display, str::FromStr, sync::Arc};

use arrow::array::{Array, UInt8Array};
use nautilus_model::{enums::FromU8, identifiers::InstrumentId};
use nautilus_serialization::arrow::{
    EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, StringColumnRef,
    enum_dictionary_data_type,
};

fn parse_metadata(
    metadata: &HashMap<String, String>,
) -> Result<(InstrumentId, u8, u8), EncodingError> {
    let instrument_id_str = metadata
        .get(KEY_INSTRUMENT_ID)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_INSTRUMENT_ID))?;
    let instrument_id = InstrumentId::from_str(instrument_id_str)
        .map_err(|e| EncodingError::ParseError(KEY_INSTRUMENT_ID, e.to_string()))?;

    let price_precision = metadata
        .get(KEY_PRICE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_PRICE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_PRICE_PRECISION, e.to_string()))?;

    let size_precision = metadata
        .get(KEY_SIZE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_SIZE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_SIZE_PRECISION, e.to_string()))?;

    Ok((instrument_id, price_precision, size_precision))
}

enum EnumColumn<'a> {
    Strings(StringColumnRef<'a>, &'static str),
    Codes(&'a UInt8Array, &'static str),
}

impl<'a> EnumColumn<'a> {
    fn try_from_column(
        column: &'a Arc<dyn Array>,
        field: &'static str,
        index: usize,
    ) -> Result<Self, EncodingError> {
        if let Some(values) = StringColumnRef::try_from_array(column.as_ref()) {
            return Ok(Self::Strings(values, field));
        }
        column
            .as_any()
            .downcast_ref::<UInt8Array>()
            .map(|values| Self::Codes(values, field))
            .ok_or_else(|| {
                EncodingError::InvalidColumnType(
                    field,
                    index,
                    enum_dictionary_data_type(),
                    column.data_type().clone(),
                )
            })
    }

    fn decode<T>(&self, row: usize) -> Result<T, EncodingError>
    where
        T: FromStr + FromU8,
        T::Err: Display,
    {
        match self {
            Self::Strings(values, field) => values
                .value(row)
                .parse::<T>()
                .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}"))),
            Self::Codes(values, field) => {
                let value = values.value(row);
                T::from_u8(value).ok_or_else(|| {
                    EncodingError::ParseError(field, format!("Invalid enum value, was {value}"))
                })
            }
        }
    }

    fn decode_optional<T, F>(
        &self,
        row: usize,
        legacy_none: &str,
        decode_code: F,
    ) -> Result<Option<T>, EncodingError>
    where
        T: FromStr,
        T::Err: Display,
        F: Fn(u8) -> Option<T>,
    {
        match self {
            Self::Strings(values, field) => {
                let value = values.value(row);
                if value.eq_ignore_ascii_case(legacy_none) {
                    Ok(None)
                } else {
                    value
                        .parse::<T>()
                        .map(Some)
                        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
                }
            }
            Self::Codes(values, field) => {
                let value = values.value(row);
                if value == 0 {
                    Ok(None)
                } else {
                    decode_code(value).map(Some).ok_or_else(|| {
                        EncodingError::ParseError(field, format!("Invalid enum value, was {value}"))
                    })
                }
            }
        }
    }
}
