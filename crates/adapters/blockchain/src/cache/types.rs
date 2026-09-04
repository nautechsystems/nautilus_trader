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

use alloy::primitives::{I256, U160, U256};
use sqlx::{
    Database, Decode, Encode, Postgres, Type,
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgHasArrayType, PgTypeInfo},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I256Pg(pub I256);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U256Pg(pub U256);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct U160Pg(pub U160);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct U128Pg(pub u128);

macro_rules! impl_pg_numeric {
    ($wrapper:ty, $inner:ty, $pg_type:literal, $display_name:literal) => {
        impl Type<Postgres> for $wrapper {
            fn type_info() -> PgTypeInfo {
                PgTypeInfo::with_name($pg_type)
            }
        }

        // PostgreSQL numeric values require decimal Display encoding
        impl<'q> Encode<'q, Postgres> for $wrapper {
            fn encode_by_ref(
                &self,
                buf: &mut <Postgres as Database>::ArgumentBuffer,
            ) -> Result<IsNull, BoxDynError> {
                let value = self.0.to_string();
                <&str as Encode<Postgres>>::encode(&value, buf)
            }
        }

        impl<'r> Decode<'r, Postgres> for $wrapper {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let value = <String as Decode<Postgres>>::decode(value)?;
                let value = <$inner>::from_str(&value)
                    .map_err(|e| format!("Failed to parse {}: {e}", $display_name))?;
                Ok(Self(value))
            }
        }

        impl PgHasArrayType for $wrapper {
            fn array_type_info() -> PgTypeInfo {
                PgTypeInfo::with_name(concat!("_", $pg_type))
            }
        }
    };
}

impl_pg_numeric!(I256Pg, I256, "i256", "I256");
impl_pg_numeric!(U256Pg, U256, "u256", "U256");
impl_pg_numeric!(U160Pg, U160, "u160", "U160");
impl_pg_numeric!(U128Pg, u128, "u128", "U128");
