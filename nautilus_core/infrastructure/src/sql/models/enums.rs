// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::enums::{AssetClass, CurrencyType};
use sqlx::{
    database::{HasArguments, HasValueRef},
    encode::IsNull,
    error::BoxDynError,
    postgres::PgTypeInfo,
    types::Type,
    Decode, Postgres,
};

pub struct CurrencyTypeModel(pub CurrencyType);

impl sqlx::Encode<'_, sqlx::Postgres> for CurrencyTypeModel {
    fn encode_by_ref(&self, buf: &mut <Postgres as HasArguments<'_>>::ArgumentBuffer) -> IsNull {
        let currency_type_str = match self.0 {
            CurrencyType::Crypto => "CRYPTO",
            CurrencyType::Fiat => "FIAT",
            CurrencyType::CommodityBacked => "COMMODITY_BACKED",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(currency_type_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CurrencyTypeModel {
    fn decode(value: <Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let currency_type_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let currency_type = CurrencyType::from_str(currency_type_str).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid currency type: {}", currency_type_str).into())
        })?;
        Ok(CurrencyTypeModel(currency_type))
    }
}

impl sqlx::Type<sqlx::Postgres> for CurrencyTypeModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("currency_type")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}

pub struct AssetClassModel(pub AssetClass);

impl sqlx::Encode<'_, sqlx::Postgres> for AssetClassModel {
    fn encode_by_ref(&self, buf: &mut <Postgres as HasArguments<'_>>::ArgumentBuffer) -> IsNull {
        let asset_type_str = match self.0 {
            AssetClass::FX => "FX",
            AssetClass::Equity => "EQUITY",
            AssetClass::Commodity => "COMMODITY",
            AssetClass::Debt => "DEBT",
            AssetClass::Index => "INDEX",
            AssetClass::Cryptocurrency => "CRYPTOCURRENCY",
            AssetClass::Alternative => "ALTERNATIVE",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(asset_type_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for AssetClassModel {
    fn decode(value: <Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let asset_class_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let asset_class = AssetClass::from_str(asset_class_str).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid asset class: {}", asset_class_str).into())
        })?;
        Ok(AssetClassModel(asset_class))
    }
}

impl sqlx::Type<sqlx::Postgres> for AssetClassModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("asset_class")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}
