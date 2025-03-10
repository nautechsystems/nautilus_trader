// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::enums::{
    AggregationSource, AggressorSide, AssetClass, BarAggregation, CurrencyType, PriceType,
    TrailingOffsetType,
};
use sqlx::{
    Database, Decode, Postgres, encode::IsNull, error::BoxDynError, postgres::PgTypeInfo,
    types::Type,
};

pub struct CurrencyTypeModel(pub CurrencyType);
pub struct PriceTypeModel(pub PriceType);
pub struct BarAggregationModel(pub BarAggregation);
pub struct AssetClassModel(pub AssetClass);
pub struct TrailingOffsetTypeModel(pub TrailingOffsetType);
pub struct AggressorSideModel(pub AggressorSide);
pub struct AggregationSourceModel(pub AggregationSource);

impl sqlx::Encode<'_, sqlx::Postgres> for CurrencyTypeModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let currency_type_str = match self.0 {
            CurrencyType::Crypto => "CRYPTO",
            CurrencyType::Fiat => "FIAT",
            CurrencyType::CommodityBacked => "COMMODITY_BACKED",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(currency_type_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CurrencyTypeModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
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

impl sqlx::Encode<'_, sqlx::Postgres> for AssetClassModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
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
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
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

impl sqlx::Encode<'_, sqlx::Postgres> for TrailingOffsetTypeModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let trailing_offset_type_str = match self.0 {
            TrailingOffsetType::NoTrailingOffset => "NO_TRAILING_OFFSET",
            TrailingOffsetType::Price => "PRICE",
            TrailingOffsetType::BasisPoints => "BASIS_POINTS",
            TrailingOffsetType::Ticks => "TICKS",
            TrailingOffsetType::PriceTier => "PRICE_TIER",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(trailing_offset_type_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for TrailingOffsetTypeModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let trailing_offset_type_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let trailing_offset_type =
            TrailingOffsetType::from_str(trailing_offset_type_str).map_err(|_| {
                sqlx::Error::Decode(
                    format!("Invalid trailing offset type: {}", trailing_offset_type_str).into(),
                )
            })?;
        Ok(TrailingOffsetTypeModel(trailing_offset_type))
    }
}

impl sqlx::Type<sqlx::Postgres> for TrailingOffsetTypeModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("trailing_offset_type")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for AggressorSideModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let aggressor_side_str = match self.0 {
            AggressorSide::NoAggressor => "NO_AGGRESSOR",
            AggressorSide::Buyer => "BUYER",
            AggressorSide::Seller => "SELLER",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(aggressor_side_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for AggressorSideModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let aggressor_side_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let aggressor_side = AggressorSide::from_str(aggressor_side_str).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid aggressor side: {}", aggressor_side_str).into())
        })?;
        Ok(AggressorSideModel(aggressor_side))
    }
}

impl sqlx::Type<sqlx::Postgres> for AggressorSideModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("aggressor_side")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for AggregationSourceModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let aggregation_source_str = match self.0 {
            AggregationSource::Internal => "INTERNAL",
            AggregationSource::External => "EXTERNAL",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(aggregation_source_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for AggregationSourceModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let aggregation_source_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let aggregation_source =
            AggregationSource::from_str(aggregation_source_str).map_err(|_| {
                sqlx::Error::Decode(
                    format!("Invalid aggregation source: {}", aggregation_source_str).into(),
                )
            })?;
        Ok(AggregationSourceModel(aggregation_source))
    }
}

impl sqlx::Type<sqlx::Postgres> for AggregationSourceModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("aggregation_source")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for BarAggregationModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let bar_aggregation_str = match self.0 {
            BarAggregation::Tick => "TICK",
            BarAggregation::TickImbalance => "TICK_IMBALANCE",
            BarAggregation::TickRuns => "TICK_RUNS",
            BarAggregation::Volume => "VOLUME",
            BarAggregation::VolumeImbalance => "VOLUME_IMBALANCE",
            BarAggregation::VolumeRuns => "VOLUME_RUNS",
            BarAggregation::Value => "VALUE",
            BarAggregation::ValueImbalance => "VALUE_IMBALANCE",
            BarAggregation::ValueRuns => "VALUE_RUNS",
            BarAggregation::Millisecond => "TIME",
            BarAggregation::Second => "SECOND",
            BarAggregation::Minute => "MINUTE",
            BarAggregation::Hour => "HOUR",
            BarAggregation::Day => "DAY",
            BarAggregation::Week => "WEEK",
            BarAggregation::Month => "MONTH",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(bar_aggregation_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BarAggregationModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let bar_aggregation_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let bar_aggregation = BarAggregation::from_str(bar_aggregation_str).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid bar aggregation: {}", bar_aggregation_str).into())
        })?;
        Ok(BarAggregationModel(bar_aggregation))
    }
}

impl sqlx::Type<sqlx::Postgres> for BarAggregationModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("bar_aggregation")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for PriceTypeModel {
    fn encode_by_ref(
        &self,
        buf: &mut <Postgres as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        let price_type_str = match self.0 {
            PriceType::Bid => "BID",
            PriceType::Ask => "ASK",
            PriceType::Mid => "MID",
            PriceType::Last => "LAST",
            PriceType::Mark => "MARK",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode(price_type_str, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for PriceTypeModel {
    fn decode(value: <Postgres as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let price_type_str: &str = <&str as Decode<sqlx::Postgres>>::decode(value)?;
        let price_type = PriceType::from_str(price_type_str).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid price type: {}", price_type_str).into())
        })?;
        Ok(PriceTypeModel(price_type))
    }
}

impl sqlx::Type<sqlx::Postgres> for PriceTypeModel {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("price_type")
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        *ty == Self::type_info() || <&str as Type<sqlx::Postgres>>::compatible(ty)
    }
}
