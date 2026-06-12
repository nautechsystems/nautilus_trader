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

//! SQL database model definitions and schemas.

use std::fmt::Display;

use sqlx::{Row, postgres::PgRow};

pub mod accounts;
pub mod data;
pub mod enums;
pub mod general;
pub mod instruments;
pub mod orders;
pub mod positions;
pub mod types;

pub(crate) fn read_u8(row: &PgRow, column: &str) -> Result<u8, sqlx::Error> {
    let value = row.try_get::<i32, _>(column)?;
    i32_to_u8(value, column)
}

pub(crate) fn read_u16(row: &PgRow, column: &str) -> Result<u16, sqlx::Error> {
    let value = row.try_get::<i32, _>(column)?;
    i32_to_u16(value, column)
}

pub(crate) fn read_usize(row: &PgRow, column: &str) -> Result<usize, sqlx::Error> {
    let value = row.try_get::<i32, _>(column)?;
    i32_to_usize(value, column)
}

pub(crate) fn read_u64(row: &PgRow, column: &str) -> Result<u64, sqlx::Error> {
    let value = row.try_get::<i64, _>(column)?;
    i64_to_u64(value, column)
}

pub(crate) fn i64_to_u64(value: i64, label: &str) -> Result<u64, sqlx::Error> {
    u64::try_from(value).map_err(|e| decode_error(label, value, e))
}

fn i32_to_u8(value: i32, label: &str) -> Result<u8, sqlx::Error> {
    u8::try_from(value).map_err(|e| decode_error(label, value, e))
}

fn i32_to_u16(value: i32, label: &str) -> Result<u16, sqlx::Error> {
    u16::try_from(value).map_err(|e| decode_error(label, value, e))
}

fn i32_to_usize(value: i32, label: &str) -> Result<usize, sqlx::Error> {
    usize::try_from(value).map_err(|e| decode_error(label, value, e))
}

fn decode_error(label: &str, value: impl Display, error: impl Display) -> sqlx::Error {
    sqlx::Error::Decode(format!("Invalid {label} value {value}: {error}").into())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{i32_to_u8, i32_to_u16, i32_to_usize, i64_to_u64};

    #[rstest]
    #[case::negative(-1, "Invalid price_precision value -1")]
    #[case::too_large(i32::from(u8::MAX) + 1, "Invalid price_precision value 256")]
    fn i32_to_u8_rejects_out_of_range(#[case] value: i32, #[case] expected: &str) {
        let error = i32_to_u8(value, "price_precision").unwrap_err();

        assert_decode_error_contains(&error, expected);
    }

    #[rstest]
    #[case::negative(-1, "Invalid iso4217 value -1")]
    #[case::too_large(i32::from(u16::MAX) + 1, "Invalid iso4217 value 65536")]
    fn i32_to_u16_rejects_out_of_range(#[case] value: i32, #[case] expected: &str) {
        let error = i32_to_u16(value, "iso4217").unwrap_err();

        assert_decode_error_contains(&error, expected);
    }

    #[rstest]
    fn i32_to_usize_rejects_negative_values() {
        let error = i32_to_usize(-1, "step").unwrap_err();

        assert_decode_error_contains(&error, "Invalid step value -1");
    }

    #[rstest]
    fn i64_to_u64_rejects_negative_values() {
        let error = i64_to_u64(-1, "duration_ns").unwrap_err();

        assert_decode_error_contains(&error, "Invalid duration_ns value -1");
    }

    fn assert_decode_error_contains(error: &sqlx::Error, expected: &str) {
        assert!(matches!(error, sqlx::Error::Decode(_)));
        assert!(error.to_string().contains(expected));
    }
}
