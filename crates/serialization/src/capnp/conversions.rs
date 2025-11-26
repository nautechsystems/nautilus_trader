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

//! Conversion implementations between Nautilus types and Cap'n Proto.

use std::error::Error;

use indexmap::IndexMap;
use nautilus_model::{
    data::{
        FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
        QuoteTick, TradeTick,
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        depth::OrderBookDepth10,
        order::BookOrder,
    },
    enums::{
        AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, BookAction,
        BookType, ContingencyType, CurrencyType, InstrumentClass, InstrumentCloseType,
        LiquiditySide, MarketStatusAction, OmsType, OptionKind, OrderSide, OrderStatus, OrderType,
        PositionAdjustmentType, PositionSide, PriceType, RecordFlag, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionAdjusted, PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
        OrderListId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
    },
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;
use uuid::Uuid;

use super::{FromCapnp, ToCapnp};
use crate::{
    base_capnp, enums_capnp, identifiers_capnp, market_capnp, order_capnp, position_capnp,
    types_capnp,
};

trait CapnpWriteExt<'a, T>
where
    T: ToCapnp<'a>,
{
    fn write_capnp<F>(&self, init: F)
    where
        F: FnOnce() -> T::Builder;
}

impl<'a, T> CapnpWriteExt<'a, T> for T
where
    T: ToCapnp<'a>,
{
    fn write_capnp<F>(&self, init: F)
    where
        F: FnOnce() -> T::Builder,
    {
        self.to_capnp(init());
    }
}

impl<'a, T> CapnpWriteExt<'a, T> for Option<T>
where
    T: ToCapnp<'a>,
{
    fn write_capnp<F>(&self, init: F)
    where
        F: FnOnce() -> T::Builder,
    {
        if let Some(value) = self {
            value.to_capnp(init());
        }
    }
}

fn read_optional_from_capnp<'a, T, FHas, FGet>(
    has: FHas,
    get: FGet,
) -> Result<Option<T>, Box<dyn Error>>
where
    T: FromCapnp<'a>,
    FHas: FnOnce() -> bool,
    FGet: FnOnce() -> capnp::Result<<T as FromCapnp<'a>>::Reader>,
{
    if has() {
        let reader = get()?;
        Ok(Some(T::from_capnp(reader)?))
    } else {
        Ok(None)
    }
}

// ================================================================================================
// Base Types
// ================================================================================================

impl<'a> ToCapnp<'a> for nautilus_core::UUID4 {
    type Builder = base_capnp::u_u_i_d4::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(&self.as_bytes());
    }
}

impl<'a> FromCapnp<'a> for nautilus_core::UUID4 {
    type Reader = base_capnp::u_u_i_d4::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let bytes = reader.get_value()?;
        let bytes_array: [u8; 16] = bytes.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UUID4 bytes length",
            )
        })?;
        let uuid = Uuid::from_bytes(bytes_array);
        Ok(Self::from(uuid))
    }
}

// Decimal
// rust_decimal serialization format (16 bytes):
// - Bytes 0-3: flags (u32) - scale and sign
// - Bytes 4-7: lo (u32) - low 32 bits of coefficient
// - Bytes 8-11: mid (u32) - middle 32 bits of coefficient
// - Bytes 12-15: hi (u32) - high 32 bits of coefficient
fn decimal_to_parts(value: &Decimal) -> (u64, u64, u64, u32) {
    let bytes = value.serialize();
    let flags = u32::from_le_bytes(bytes[0..4].try_into().expect("flags slice"));
    let lo = u32::from_le_bytes(bytes[4..8].try_into().expect("lo slice"));
    let mid = u32::from_le_bytes(bytes[8..12].try_into().expect("mid slice"));
    let hi = u32::from_le_bytes(bytes[12..16].try_into().expect("hi slice"));
    (lo as u64, mid as u64, hi as u64, flags)
}

fn decimal_from_parts(lo: u64, mid: u64, hi: u64, flags: u32) -> Decimal {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&flags.to_le_bytes());
    bytes[4..8].copy_from_slice(&(lo as u32).to_le_bytes());
    bytes[8..12].copy_from_slice(&(mid as u32).to_le_bytes());
    bytes[12..16].copy_from_slice(&(hi as u32).to_le_bytes());
    Decimal::deserialize(bytes)
}

impl<'a> ToCapnp<'a> for Decimal {
    type Builder = types_capnp::decimal::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let (lo, mid, hi, flags) = decimal_to_parts(self);
        builder.set_flags(flags);
        builder.set_lo(lo);
        builder.set_mid(mid);
        builder.set_hi(hi);
    }
}

impl<'a> FromCapnp<'a> for Decimal {
    type Reader = types_capnp::decimal::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let flags = reader.get_flags();
        let lo = reader.get_lo();
        let mid = reader.get_mid();
        let hi = reader.get_hi();
        Ok(decimal_from_parts(lo, mid, hi, flags))
    }
}

// ================================================================================================
// Identifiers
// ================================================================================================

impl<'a> ToCapnp<'a> for TraderId {
    type Builder = identifiers_capnp::trader_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for TraderId {
    type Reader = identifiers_capnp::trader_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for StrategyId {
    type Builder = identifiers_capnp::strategy_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for StrategyId {
    type Reader = identifiers_capnp::strategy_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for ActorId {
    type Builder = identifiers_capnp::actor_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for ActorId {
    type Reader = identifiers_capnp::actor_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for AccountId {
    type Builder = identifiers_capnp::account_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for AccountId {
    type Reader = identifiers_capnp::account_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for ClientId {
    type Builder = identifiers_capnp::client_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for ClientId {
    type Reader = identifiers_capnp::client_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for ClientOrderId {
    type Builder = identifiers_capnp::client_order_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for ClientOrderId {
    type Reader = identifiers_capnp::client_order_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for VenueOrderId {
    type Builder = identifiers_capnp::venue_order_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for VenueOrderId {
    type Reader = identifiers_capnp::venue_order_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for TradeId {
    type Builder = identifiers_capnp::trade_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_cstr().to_str().expect("Valid UTF-8"));
    }
}

impl<'a> FromCapnp<'a> for TradeId {
    type Reader = identifiers_capnp::trade_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for PositionId {
    type Builder = identifiers_capnp::position_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for PositionId {
    type Reader = identifiers_capnp::position_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for ExecAlgorithmId {
    type Builder = identifiers_capnp::exec_algorithm_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for ExecAlgorithmId {
    type Reader = identifiers_capnp::exec_algorithm_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for ComponentId {
    type Builder = identifiers_capnp::component_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for ComponentId {
    type Reader = identifiers_capnp::component_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for OrderListId {
    type Builder = identifiers_capnp::order_list_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for OrderListId {
    type Reader = identifiers_capnp::order_list_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for Symbol {
    type Builder = identifiers_capnp::symbol::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for Symbol {
    type Reader = identifiers_capnp::symbol::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for Venue {
    type Builder = identifiers_capnp::venue::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_value(self.as_str());
    }
}

impl<'a> FromCapnp<'a> for Venue {
    type Reader = identifiers_capnp::venue::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let value = reader.get_value()?.to_str()?;
        Ok(value.into())
    }
}

impl<'a> ToCapnp<'a> for InstrumentId {
    type Builder = identifiers_capnp::instrument_id::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.symbol.to_capnp(builder.reborrow().init_symbol());
        self.venue.to_capnp(builder.init_venue());
    }
}

impl<'a> FromCapnp<'a> for InstrumentId {
    type Reader = identifiers_capnp::instrument_id::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let symbol = Symbol::from_capnp(reader.get_symbol()?)?;
        let venue = Venue::from_capnp(reader.get_venue()?)?;
        Ok(Self::new(symbol, venue))
    }
}

impl<'a> ToCapnp<'a> for Price {
    type Builder = types_capnp::price::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let raw = self.raw;

        #[cfg(not(feature = "high-precision"))]
        {
            let raw_i128 = raw as i128;
            let lo = raw_i128 as u64;
            let hi = (raw_i128 >> 64) as u64;

            let mut raw_builder = builder.reborrow().init_raw();
            raw_builder.set_lo(lo);
            raw_builder.set_hi(hi);
        }

        #[cfg(feature = "high-precision")]
        {
            let lo = raw as u64;
            let hi = (raw >> 64) as u64;

            let mut raw_builder = builder.reborrow().init_raw();
            raw_builder.set_lo(lo);
            raw_builder.set_hi(hi);
        }

        builder.set_precision(self.precision);
    }
}

impl<'a> FromCapnp<'a> for Price {
    type Reader = types_capnp::price::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let raw_reader = reader.get_raw()?;
        let lo = raw_reader.get_lo();
        let hi = raw_reader.get_hi();
        let precision = reader.get_precision();

        // Reconstruct i128 from two u64 halves with proper sign extension.
        // Casting hi through i64 first ensures the sign bit (MSB of hi) propagates
        // to all upper bits when widened to i128, preserving two's complement.
        let raw_i128 = ((hi as i64 as i128) << 64) | (lo as i128);

        #[cfg(not(feature = "high-precision"))]
        {
            let raw = i64::try_from(raw_i128).map_err(|_| -> Box<dyn Error> {
                "Price value overflows i64 in standard precision mode".into()
            })?;
            Ok(Price::from_raw(raw.into(), precision))
        }

        #[cfg(feature = "high-precision")]
        {
            Ok(Self::from_raw(raw_i128, precision))
        }
    }
}

impl<'a> ToCapnp<'a> for Quantity {
    type Builder = types_capnp::quantity::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let raw = self.raw;

        #[cfg(not(feature = "high-precision"))]
        {
            let raw_u128 = raw as u128;
            let lo = raw_u128 as u64;
            let hi = (raw_u128 >> 64) as u64;

            let mut raw_builder = builder.reborrow().init_raw();
            raw_builder.set_lo(lo);
            raw_builder.set_hi(hi);
        }

        #[cfg(feature = "high-precision")]
        {
            let lo = raw as u64;
            let hi = (raw >> 64) as u64;

            let mut raw_builder = builder.reborrow().init_raw();
            raw_builder.set_lo(lo);
            raw_builder.set_hi(hi);
        }

        builder.set_precision(self.precision);
    }
}

impl<'a> FromCapnp<'a> for Quantity {
    type Reader = types_capnp::quantity::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let raw_reader = reader.get_raw()?;
        let lo = raw_reader.get_lo();
        let hi = raw_reader.get_hi();
        let precision = reader.get_precision();

        // Reconstruct u128 from two u64 halves (unsigned, no sign extension needed)
        let raw_u128 = ((hi as u128) << 64) | (lo as u128);

        #[cfg(not(feature = "high-precision"))]
        {
            let raw = u64::try_from(raw_u128).map_err(|_| -> Box<dyn Error> {
                "Quantity value overflows u64 in standard precision mode".into()
            })?;
            Ok(Quantity::from_raw(raw.into(), precision))
        }

        #[cfg(feature = "high-precision")]
        {
            Ok(Self::from_raw(raw_u128, precision))
        }
    }
}

#[must_use]
pub fn currency_type_to_capnp(value: CurrencyType) -> enums_capnp::CurrencyType {
    match value {
        CurrencyType::Crypto => enums_capnp::CurrencyType::Crypto,
        CurrencyType::Fiat => enums_capnp::CurrencyType::Fiat,
        CurrencyType::CommodityBacked => enums_capnp::CurrencyType::CommodityBacked,
    }
}

#[must_use]
pub fn currency_type_from_capnp(value: enums_capnp::CurrencyType) -> CurrencyType {
    match value {
        enums_capnp::CurrencyType::Crypto => CurrencyType::Crypto,
        enums_capnp::CurrencyType::Fiat => CurrencyType::Fiat,
        enums_capnp::CurrencyType::CommodityBacked => CurrencyType::CommodityBacked,
    }
}

#[must_use]
pub fn account_type_to_capnp(value: AccountType) -> enums_capnp::AccountType {
    match value {
        AccountType::Cash => enums_capnp::AccountType::Cash,
        AccountType::Margin => enums_capnp::AccountType::Margin,
        AccountType::Betting => enums_capnp::AccountType::Betting,
        AccountType::Wallet => enums_capnp::AccountType::Wallet,
    }
}

#[must_use]
pub fn account_type_from_capnp(value: enums_capnp::AccountType) -> AccountType {
    match value {
        enums_capnp::AccountType::Cash => AccountType::Cash,
        enums_capnp::AccountType::Margin => AccountType::Margin,
        enums_capnp::AccountType::Betting => AccountType::Betting,
        enums_capnp::AccountType::Wallet => AccountType::Wallet,
    }
}

#[must_use]
pub fn aggressor_side_to_capnp(value: AggressorSide) -> enums_capnp::AggressorSide {
    match value {
        AggressorSide::NoAggressor => enums_capnp::AggressorSide::NoAggressor,
        AggressorSide::Buyer => enums_capnp::AggressorSide::Buyer,
        AggressorSide::Seller => enums_capnp::AggressorSide::Seller,
    }
}

#[must_use]
pub fn aggressor_side_from_capnp(value: enums_capnp::AggressorSide) -> AggressorSide {
    match value {
        enums_capnp::AggressorSide::NoAggressor => AggressorSide::NoAggressor,
        enums_capnp::AggressorSide::Buyer => AggressorSide::Buyer,
        enums_capnp::AggressorSide::Seller => AggressorSide::Seller,
    }
}

#[must_use]
pub fn asset_class_to_capnp(value: AssetClass) -> enums_capnp::AssetClass {
    match value {
        AssetClass::FX => enums_capnp::AssetClass::Fx,
        AssetClass::Equity => enums_capnp::AssetClass::Equity,
        AssetClass::Commodity => enums_capnp::AssetClass::Commodity,
        AssetClass::Debt => enums_capnp::AssetClass::Debt,
        AssetClass::Index => enums_capnp::AssetClass::Index,
        AssetClass::Cryptocurrency => enums_capnp::AssetClass::Cryptocurrency,
        AssetClass::Alternative => enums_capnp::AssetClass::Alternative,
    }
}

#[must_use]
pub fn asset_class_from_capnp(value: enums_capnp::AssetClass) -> AssetClass {
    match value {
        enums_capnp::AssetClass::Fx => AssetClass::FX,
        enums_capnp::AssetClass::Equity => AssetClass::Equity,
        enums_capnp::AssetClass::Commodity => AssetClass::Commodity,
        enums_capnp::AssetClass::Debt => AssetClass::Debt,
        enums_capnp::AssetClass::Index => AssetClass::Index,
        enums_capnp::AssetClass::Cryptocurrency => AssetClass::Cryptocurrency,
        enums_capnp::AssetClass::Alternative => AssetClass::Alternative,
    }
}

#[must_use]
pub fn instrument_class_to_capnp(value: InstrumentClass) -> enums_capnp::InstrumentClass {
    match value {
        InstrumentClass::Spot => enums_capnp::InstrumentClass::Spot,
        InstrumentClass::Swap => enums_capnp::InstrumentClass::Swap,
        InstrumentClass::Future => enums_capnp::InstrumentClass::Future,
        InstrumentClass::FuturesSpread => enums_capnp::InstrumentClass::FuturesSpread,
        InstrumentClass::Forward => enums_capnp::InstrumentClass::Forward,
        InstrumentClass::Cfd => enums_capnp::InstrumentClass::Cfd,
        InstrumentClass::Bond => enums_capnp::InstrumentClass::Bond,
        InstrumentClass::Option => enums_capnp::InstrumentClass::Option,
        InstrumentClass::OptionSpread => enums_capnp::InstrumentClass::OptionSpread,
        InstrumentClass::Warrant => enums_capnp::InstrumentClass::Warrant,
        InstrumentClass::SportsBetting => enums_capnp::InstrumentClass::SportsBetting,
        InstrumentClass::BinaryOption => enums_capnp::InstrumentClass::BinaryOption,
    }
}

#[must_use]
pub fn instrument_class_from_capnp(value: enums_capnp::InstrumentClass) -> InstrumentClass {
    match value {
        enums_capnp::InstrumentClass::Spot => InstrumentClass::Spot,
        enums_capnp::InstrumentClass::Swap => InstrumentClass::Swap,
        enums_capnp::InstrumentClass::Future => InstrumentClass::Future,
        enums_capnp::InstrumentClass::FuturesSpread => InstrumentClass::FuturesSpread,
        enums_capnp::InstrumentClass::Forward => InstrumentClass::Forward,
        enums_capnp::InstrumentClass::Cfd => InstrumentClass::Cfd,
        enums_capnp::InstrumentClass::Bond => InstrumentClass::Bond,
        enums_capnp::InstrumentClass::Option => InstrumentClass::Option,
        enums_capnp::InstrumentClass::OptionSpread => InstrumentClass::OptionSpread,
        enums_capnp::InstrumentClass::Warrant => InstrumentClass::Warrant,
        enums_capnp::InstrumentClass::SportsBetting => InstrumentClass::SportsBetting,
        enums_capnp::InstrumentClass::BinaryOption => InstrumentClass::BinaryOption,
    }
}

#[must_use]
pub fn option_kind_to_capnp(value: OptionKind) -> enums_capnp::OptionKind {
    match value {
        OptionKind::Call => enums_capnp::OptionKind::Call,
        OptionKind::Put => enums_capnp::OptionKind::Put,
    }
}

#[must_use]
pub fn option_kind_from_capnp(value: enums_capnp::OptionKind) -> OptionKind {
    match value {
        enums_capnp::OptionKind::Call => OptionKind::Call,
        enums_capnp::OptionKind::Put => OptionKind::Put,
    }
}

#[must_use]
pub fn order_side_to_capnp(value: OrderSide) -> enums_capnp::OrderSide {
    match value {
        OrderSide::NoOrderSide => enums_capnp::OrderSide::NoOrderSide,
        OrderSide::Buy => enums_capnp::OrderSide::Buy,
        OrderSide::Sell => enums_capnp::OrderSide::Sell,
    }
}

#[must_use]
pub fn order_side_from_capnp(value: enums_capnp::OrderSide) -> OrderSide {
    match value {
        enums_capnp::OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        enums_capnp::OrderSide::Buy => OrderSide::Buy,
        enums_capnp::OrderSide::Sell => OrderSide::Sell,
    }
}

#[must_use]
pub fn order_type_to_capnp(value: OrderType) -> enums_capnp::OrderType {
    match value {
        OrderType::Market => enums_capnp::OrderType::Market,
        OrderType::Limit => enums_capnp::OrderType::Limit,
        OrderType::StopMarket => enums_capnp::OrderType::StopMarket,
        OrderType::StopLimit => enums_capnp::OrderType::StopLimit,
        OrderType::MarketToLimit => enums_capnp::OrderType::MarketToLimit,
        OrderType::MarketIfTouched => enums_capnp::OrderType::MarketIfTouched,
        OrderType::LimitIfTouched => enums_capnp::OrderType::LimitIfTouched,
        OrderType::TrailingStopMarket => enums_capnp::OrderType::TrailingStopMarket,
        OrderType::TrailingStopLimit => enums_capnp::OrderType::TrailingStopLimit,
    }
}

#[must_use]
pub fn order_type_from_capnp(value: enums_capnp::OrderType) -> OrderType {
    match value {
        enums_capnp::OrderType::Market => OrderType::Market,
        enums_capnp::OrderType::Limit => OrderType::Limit,
        enums_capnp::OrderType::StopMarket => OrderType::StopMarket,
        enums_capnp::OrderType::StopLimit => OrderType::StopLimit,
        enums_capnp::OrderType::MarketToLimit => OrderType::MarketToLimit,
        enums_capnp::OrderType::MarketIfTouched => OrderType::MarketIfTouched,
        enums_capnp::OrderType::LimitIfTouched => OrderType::LimitIfTouched,
        enums_capnp::OrderType::TrailingStopMarket => OrderType::TrailingStopMarket,
        enums_capnp::OrderType::TrailingStopLimit => OrderType::TrailingStopLimit,
    }
}

#[must_use]
pub fn order_status_to_capnp(value: OrderStatus) -> enums_capnp::OrderStatus {
    match value {
        OrderStatus::Initialized => enums_capnp::OrderStatus::Initialized,
        OrderStatus::Denied => enums_capnp::OrderStatus::Denied,
        OrderStatus::Emulated => enums_capnp::OrderStatus::Emulated,
        OrderStatus::Released => enums_capnp::OrderStatus::Released,
        OrderStatus::Submitted => enums_capnp::OrderStatus::Submitted,
        OrderStatus::Accepted => enums_capnp::OrderStatus::Accepted,
        OrderStatus::Rejected => enums_capnp::OrderStatus::Rejected,
        OrderStatus::Canceled => enums_capnp::OrderStatus::Canceled,
        OrderStatus::Expired => enums_capnp::OrderStatus::Expired,
        OrderStatus::Triggered => enums_capnp::OrderStatus::Triggered,
        OrderStatus::PendingUpdate => enums_capnp::OrderStatus::PendingUpdate,
        OrderStatus::PendingCancel => enums_capnp::OrderStatus::PendingCancel,
        OrderStatus::PartiallyFilled => enums_capnp::OrderStatus::PartiallyFilled,
        OrderStatus::Filled => enums_capnp::OrderStatus::Filled,
    }
}

#[must_use]
pub fn order_status_from_capnp(value: enums_capnp::OrderStatus) -> OrderStatus {
    match value {
        enums_capnp::OrderStatus::Initialized => OrderStatus::Initialized,
        enums_capnp::OrderStatus::Denied => OrderStatus::Denied,
        enums_capnp::OrderStatus::Emulated => OrderStatus::Emulated,
        enums_capnp::OrderStatus::Released => OrderStatus::Released,
        enums_capnp::OrderStatus::Submitted => OrderStatus::Submitted,
        enums_capnp::OrderStatus::Accepted => OrderStatus::Accepted,
        enums_capnp::OrderStatus::Rejected => OrderStatus::Rejected,
        enums_capnp::OrderStatus::Canceled => OrderStatus::Canceled,
        enums_capnp::OrderStatus::Expired => OrderStatus::Expired,
        enums_capnp::OrderStatus::Triggered => OrderStatus::Triggered,
        enums_capnp::OrderStatus::PendingUpdate => OrderStatus::PendingUpdate,
        enums_capnp::OrderStatus::PendingCancel => OrderStatus::PendingCancel,
        enums_capnp::OrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        enums_capnp::OrderStatus::Filled => OrderStatus::Filled,
    }
}

#[must_use]
pub fn time_in_force_to_capnp(value: TimeInForce) -> enums_capnp::TimeInForce {
    match value {
        TimeInForce::Gtc => enums_capnp::TimeInForce::Gtc,
        TimeInForce::Ioc => enums_capnp::TimeInForce::Ioc,
        TimeInForce::Fok => enums_capnp::TimeInForce::Fok,
        TimeInForce::Gtd => enums_capnp::TimeInForce::Gtd,
        TimeInForce::Day => enums_capnp::TimeInForce::Day,
        TimeInForce::AtTheOpen => enums_capnp::TimeInForce::AtTheOpen,
        TimeInForce::AtTheClose => enums_capnp::TimeInForce::AtTheClose,
    }
}

#[must_use]
pub fn time_in_force_from_capnp(value: enums_capnp::TimeInForce) -> TimeInForce {
    match value {
        enums_capnp::TimeInForce::Gtc => TimeInForce::Gtc,
        enums_capnp::TimeInForce::Ioc => TimeInForce::Ioc,
        enums_capnp::TimeInForce::Fok => TimeInForce::Fok,
        enums_capnp::TimeInForce::Gtd => TimeInForce::Gtd,
        enums_capnp::TimeInForce::Day => TimeInForce::Day,
        enums_capnp::TimeInForce::AtTheOpen => TimeInForce::AtTheOpen,
        enums_capnp::TimeInForce::AtTheClose => TimeInForce::AtTheClose,
    }
}

#[must_use]
pub fn trigger_type_to_capnp(value: TriggerType) -> enums_capnp::TriggerType {
    match value {
        TriggerType::NoTrigger => enums_capnp::TriggerType::NoTrigger,
        TriggerType::Default => enums_capnp::TriggerType::Default,
        TriggerType::LastPrice => enums_capnp::TriggerType::LastPrice,
        TriggerType::MarkPrice => enums_capnp::TriggerType::MarkPrice,
        TriggerType::IndexPrice => enums_capnp::TriggerType::IndexPrice,
        TriggerType::BidAsk => enums_capnp::TriggerType::BidAsk,
        TriggerType::DoubleLast => enums_capnp::TriggerType::DoubleLast,
        TriggerType::DoubleBidAsk => enums_capnp::TriggerType::DoubleBidAsk,
        TriggerType::LastOrBidAsk => enums_capnp::TriggerType::LastOrBidAsk,
        TriggerType::MidPoint => enums_capnp::TriggerType::MidPoint,
    }
}

#[must_use]
pub fn trigger_type_from_capnp(value: enums_capnp::TriggerType) -> TriggerType {
    match value {
        enums_capnp::TriggerType::NoTrigger => TriggerType::NoTrigger,
        enums_capnp::TriggerType::Default => TriggerType::Default,
        enums_capnp::TriggerType::LastPrice => TriggerType::LastPrice,
        enums_capnp::TriggerType::MarkPrice => TriggerType::MarkPrice,
        enums_capnp::TriggerType::IndexPrice => TriggerType::IndexPrice,
        enums_capnp::TriggerType::BidAsk => TriggerType::BidAsk,
        enums_capnp::TriggerType::DoubleLast => TriggerType::DoubleLast,
        enums_capnp::TriggerType::DoubleBidAsk => TriggerType::DoubleBidAsk,
        enums_capnp::TriggerType::LastOrBidAsk => TriggerType::LastOrBidAsk,
        enums_capnp::TriggerType::MidPoint => TriggerType::MidPoint,
    }
}

#[must_use]
pub fn contingency_type_to_capnp(value: ContingencyType) -> enums_capnp::ContingencyType {
    match value {
        ContingencyType::NoContingency => enums_capnp::ContingencyType::NoContingency,
        ContingencyType::Oco => enums_capnp::ContingencyType::Oco,
        ContingencyType::Oto => enums_capnp::ContingencyType::Oto,
        ContingencyType::Ouo => enums_capnp::ContingencyType::Ouo,
    }
}

#[must_use]
pub fn contingency_type_from_capnp(value: enums_capnp::ContingencyType) -> ContingencyType {
    match value {
        enums_capnp::ContingencyType::NoContingency => ContingencyType::NoContingency,
        enums_capnp::ContingencyType::Oco => ContingencyType::Oco,
        enums_capnp::ContingencyType::Oto => ContingencyType::Oto,
        enums_capnp::ContingencyType::Ouo => ContingencyType::Ouo,
    }
}

#[must_use]
pub fn position_side_to_capnp(value: PositionSide) -> enums_capnp::PositionSide {
    match value {
        PositionSide::NoPositionSide => enums_capnp::PositionSide::NoPositionSide,
        PositionSide::Flat => enums_capnp::PositionSide::Flat,
        PositionSide::Long => enums_capnp::PositionSide::Long,
        PositionSide::Short => enums_capnp::PositionSide::Short,
    }
}

#[must_use]
pub fn position_side_from_capnp(value: enums_capnp::PositionSide) -> PositionSide {
    match value {
        enums_capnp::PositionSide::NoPositionSide => PositionSide::NoPositionSide,
        enums_capnp::PositionSide::Flat => PositionSide::Flat,
        enums_capnp::PositionSide::Long => PositionSide::Long,
        enums_capnp::PositionSide::Short => PositionSide::Short,
    }
}

#[must_use]
pub fn position_adjustment_type_to_capnp(
    value: PositionAdjustmentType,
) -> enums_capnp::PositionAdjustmentType {
    match value {
        PositionAdjustmentType::Commission => enums_capnp::PositionAdjustmentType::Commission,
        PositionAdjustmentType::Funding => enums_capnp::PositionAdjustmentType::Funding,
    }
}

#[must_use]
pub fn position_adjustment_type_from_capnp(
    value: enums_capnp::PositionAdjustmentType,
) -> PositionAdjustmentType {
    match value {
        enums_capnp::PositionAdjustmentType::Commission => PositionAdjustmentType::Commission,
        enums_capnp::PositionAdjustmentType::Funding => PositionAdjustmentType::Funding,
    }
}

#[must_use]
pub fn liquidity_side_to_capnp(value: LiquiditySide) -> enums_capnp::LiquiditySide {
    match value {
        LiquiditySide::NoLiquiditySide => enums_capnp::LiquiditySide::NoLiquiditySide,
        LiquiditySide::Maker => enums_capnp::LiquiditySide::Maker,
        LiquiditySide::Taker => enums_capnp::LiquiditySide::Taker,
    }
}

#[must_use]
pub fn liquidity_side_from_capnp(value: enums_capnp::LiquiditySide) -> LiquiditySide {
    match value {
        enums_capnp::LiquiditySide::NoLiquiditySide => LiquiditySide::NoLiquiditySide,
        enums_capnp::LiquiditySide::Maker => LiquiditySide::Maker,
        enums_capnp::LiquiditySide::Taker => LiquiditySide::Taker,
    }
}

#[must_use]
pub fn book_action_to_capnp(value: BookAction) -> enums_capnp::BookAction {
    match value {
        BookAction::Add => enums_capnp::BookAction::Add,
        BookAction::Update => enums_capnp::BookAction::Update,
        BookAction::Delete => enums_capnp::BookAction::Delete,
        BookAction::Clear => enums_capnp::BookAction::Clear,
    }
}

#[must_use]
pub fn book_action_from_capnp(value: enums_capnp::BookAction) -> BookAction {
    match value {
        enums_capnp::BookAction::Add => BookAction::Add,
        enums_capnp::BookAction::Update => BookAction::Update,
        enums_capnp::BookAction::Delete => BookAction::Delete,
        enums_capnp::BookAction::Clear => BookAction::Clear,
    }
}

#[must_use]
pub fn book_type_to_capnp(value: BookType) -> enums_capnp::BookType {
    match value {
        BookType::L1_MBP => enums_capnp::BookType::TopOfBookBidOffer,
        BookType::L2_MBP => enums_capnp::BookType::MarketByPrice,
        BookType::L3_MBO => enums_capnp::BookType::MarketByOrder,
    }
}

#[must_use]
pub fn book_type_from_capnp(value: enums_capnp::BookType) -> BookType {
    match value {
        enums_capnp::BookType::TopOfBookBidOffer => BookType::L1_MBP,
        enums_capnp::BookType::MarketByPrice => BookType::L2_MBP,
        enums_capnp::BookType::MarketByOrder => BookType::L3_MBO,
    }
}

#[must_use]
pub fn record_flag_to_capnp(value: RecordFlag) -> enums_capnp::RecordFlag {
    match value {
        RecordFlag::F_LAST => enums_capnp::RecordFlag::FLast,
        RecordFlag::F_TOB => enums_capnp::RecordFlag::FTob,
        RecordFlag::F_SNAPSHOT => enums_capnp::RecordFlag::FSnapshot,
        RecordFlag::F_MBP => enums_capnp::RecordFlag::FMbp,
        RecordFlag::RESERVED_2 => enums_capnp::RecordFlag::Reserved2,
        RecordFlag::RESERVED_1 => enums_capnp::RecordFlag::Reserved1,
    }
}

#[must_use]
pub fn record_flag_from_capnp(value: enums_capnp::RecordFlag) -> RecordFlag {
    match value {
        enums_capnp::RecordFlag::FLast => RecordFlag::F_LAST,
        enums_capnp::RecordFlag::FTob => RecordFlag::F_TOB,
        enums_capnp::RecordFlag::FSnapshot => RecordFlag::F_SNAPSHOT,
        enums_capnp::RecordFlag::FMbp => RecordFlag::F_MBP,
        enums_capnp::RecordFlag::Reserved2 => RecordFlag::RESERVED_2,
        enums_capnp::RecordFlag::Reserved1 => RecordFlag::RESERVED_1,
    }
}

#[must_use]
pub fn aggregation_source_to_capnp(value: AggregationSource) -> enums_capnp::AggregationSource {
    match value {
        AggregationSource::External => enums_capnp::AggregationSource::External,
        AggregationSource::Internal => enums_capnp::AggregationSource::Internal,
    }
}

#[must_use]
pub fn aggregation_source_from_capnp(value: enums_capnp::AggregationSource) -> AggregationSource {
    match value {
        enums_capnp::AggregationSource::External => AggregationSource::External,
        enums_capnp::AggregationSource::Internal => AggregationSource::Internal,
    }
}

#[must_use]
pub fn price_type_to_capnp(value: PriceType) -> enums_capnp::PriceType {
    match value {
        PriceType::Bid => enums_capnp::PriceType::Bid,
        PriceType::Ask => enums_capnp::PriceType::Ask,
        PriceType::Mid => enums_capnp::PriceType::Mid,
        PriceType::Last => enums_capnp::PriceType::Last,
        PriceType::Mark => enums_capnp::PriceType::Mark,
    }
}

#[must_use]
pub fn price_type_from_capnp(value: enums_capnp::PriceType) -> PriceType {
    match value {
        enums_capnp::PriceType::Bid => PriceType::Bid,
        enums_capnp::PriceType::Ask => PriceType::Ask,
        enums_capnp::PriceType::Mid => PriceType::Mid,
        enums_capnp::PriceType::Last => PriceType::Last,
        enums_capnp::PriceType::Mark => PriceType::Mark,
    }
}

#[must_use]
pub fn bar_aggregation_to_capnp(value: BarAggregation) -> enums_capnp::BarAggregation {
    match value {
        BarAggregation::Tick => enums_capnp::BarAggregation::Tick,
        BarAggregation::TickImbalance => enums_capnp::BarAggregation::TickImbalance,
        BarAggregation::TickRuns => enums_capnp::BarAggregation::TickRuns,
        BarAggregation::Volume => enums_capnp::BarAggregation::Volume,
        BarAggregation::VolumeImbalance => enums_capnp::BarAggregation::VolumeImbalance,
        BarAggregation::VolumeRuns => enums_capnp::BarAggregation::VolumeRuns,
        BarAggregation::Value => enums_capnp::BarAggregation::Value,
        BarAggregation::ValueImbalance => enums_capnp::BarAggregation::ValueImbalance,
        BarAggregation::ValueRuns => enums_capnp::BarAggregation::ValueRuns,
        BarAggregation::Millisecond => enums_capnp::BarAggregation::Millisecond,
        BarAggregation::Second => enums_capnp::BarAggregation::Second,
        BarAggregation::Minute => enums_capnp::BarAggregation::Minute,
        BarAggregation::Hour => enums_capnp::BarAggregation::Hour,
        BarAggregation::Day => enums_capnp::BarAggregation::Day,
        BarAggregation::Week => enums_capnp::BarAggregation::Week,
        BarAggregation::Month => enums_capnp::BarAggregation::Month,
        BarAggregation::Year => enums_capnp::BarAggregation::Year,
        BarAggregation::Renko => enums_capnp::BarAggregation::Renko,
    }
}

#[must_use]
pub fn bar_aggregation_from_capnp(value: enums_capnp::BarAggregation) -> BarAggregation {
    match value {
        enums_capnp::BarAggregation::Tick => BarAggregation::Tick,
        enums_capnp::BarAggregation::TickImbalance => BarAggregation::TickImbalance,
        enums_capnp::BarAggregation::TickRuns => BarAggregation::TickRuns,
        enums_capnp::BarAggregation::Volume => BarAggregation::Volume,
        enums_capnp::BarAggregation::VolumeImbalance => BarAggregation::VolumeImbalance,
        enums_capnp::BarAggregation::VolumeRuns => BarAggregation::VolumeRuns,
        enums_capnp::BarAggregation::Value => BarAggregation::Value,
        enums_capnp::BarAggregation::ValueImbalance => BarAggregation::ValueImbalance,
        enums_capnp::BarAggregation::ValueRuns => BarAggregation::ValueRuns,
        enums_capnp::BarAggregation::Millisecond => BarAggregation::Millisecond,
        enums_capnp::BarAggregation::Second => BarAggregation::Second,
        enums_capnp::BarAggregation::Minute => BarAggregation::Minute,
        enums_capnp::BarAggregation::Hour => BarAggregation::Hour,
        enums_capnp::BarAggregation::Day => BarAggregation::Day,
        enums_capnp::BarAggregation::Week => BarAggregation::Week,
        enums_capnp::BarAggregation::Month => BarAggregation::Month,
        enums_capnp::BarAggregation::Year => BarAggregation::Year,
        enums_capnp::BarAggregation::Renko => BarAggregation::Renko,
    }
}

#[must_use]
pub fn trailing_offset_type_to_capnp(value: TrailingOffsetType) -> enums_capnp::TrailingOffsetType {
    match value {
        TrailingOffsetType::NoTrailingOffset => enums_capnp::TrailingOffsetType::NoTrailingOffset,
        TrailingOffsetType::Price => enums_capnp::TrailingOffsetType::Price,
        TrailingOffsetType::BasisPoints => enums_capnp::TrailingOffsetType::BasisPoints,
        TrailingOffsetType::Ticks => enums_capnp::TrailingOffsetType::Ticks,
        TrailingOffsetType::PriceTier => enums_capnp::TrailingOffsetType::PriceTier,
    }
}

#[must_use]
pub fn trailing_offset_type_from_capnp(
    value: enums_capnp::TrailingOffsetType,
) -> TrailingOffsetType {
    match value {
        enums_capnp::TrailingOffsetType::NoTrailingOffset => TrailingOffsetType::NoTrailingOffset,
        enums_capnp::TrailingOffsetType::Price => TrailingOffsetType::Price,
        enums_capnp::TrailingOffsetType::BasisPoints => TrailingOffsetType::BasisPoints,
        enums_capnp::TrailingOffsetType::Ticks => TrailingOffsetType::Ticks,
        enums_capnp::TrailingOffsetType::PriceTier => TrailingOffsetType::PriceTier,
    }
}

#[must_use]
pub fn oms_type_to_capnp(value: OmsType) -> enums_capnp::OmsType {
    match value {
        OmsType::Unspecified => enums_capnp::OmsType::Unspecified,
        OmsType::Netting => enums_capnp::OmsType::Netting,
        OmsType::Hedging => enums_capnp::OmsType::Hedging,
    }
}

#[must_use]
pub fn oms_type_from_capnp(value: enums_capnp::OmsType) -> OmsType {
    match value {
        enums_capnp::OmsType::Unspecified => OmsType::Unspecified,
        enums_capnp::OmsType::Netting => OmsType::Netting,
        enums_capnp::OmsType::Hedging => OmsType::Hedging,
    }
}

#[must_use]
pub fn instrument_close_type_to_capnp(
    value: InstrumentCloseType,
) -> enums_capnp::InstrumentCloseType {
    match value {
        InstrumentCloseType::EndOfSession => enums_capnp::InstrumentCloseType::EndOfSession,
        InstrumentCloseType::ContractExpired => enums_capnp::InstrumentCloseType::ContractExpired,
    }
}

#[must_use]
pub fn instrument_close_type_from_capnp(
    value: enums_capnp::InstrumentCloseType,
) -> InstrumentCloseType {
    match value {
        enums_capnp::InstrumentCloseType::EndOfSession => InstrumentCloseType::EndOfSession,
        enums_capnp::InstrumentCloseType::ContractExpired => InstrumentCloseType::ContractExpired,
    }
}

#[must_use]
pub fn market_status_action_to_capnp(value: MarketStatusAction) -> enums_capnp::MarketStatusAction {
    match value {
        MarketStatusAction::None => enums_capnp::MarketStatusAction::None,
        MarketStatusAction::PreOpen => enums_capnp::MarketStatusAction::PreOpen,
        MarketStatusAction::PreCross => enums_capnp::MarketStatusAction::PreCross,
        MarketStatusAction::Quoting => enums_capnp::MarketStatusAction::Quoting,
        MarketStatusAction::Cross => enums_capnp::MarketStatusAction::Cross,
        MarketStatusAction::Rotation => enums_capnp::MarketStatusAction::Rotation,
        MarketStatusAction::NewPriceIndication => {
            enums_capnp::MarketStatusAction::NewPriceIndication
        }
        MarketStatusAction::Trading => enums_capnp::MarketStatusAction::Trading,
        MarketStatusAction::Halt => enums_capnp::MarketStatusAction::Halt,
        MarketStatusAction::Pause => enums_capnp::MarketStatusAction::Pause,
        MarketStatusAction::Suspend => enums_capnp::MarketStatusAction::Suspend,
        MarketStatusAction::PreClose => enums_capnp::MarketStatusAction::PreClose,
        MarketStatusAction::Close => enums_capnp::MarketStatusAction::Close,
        MarketStatusAction::PostClose => enums_capnp::MarketStatusAction::PostClose,
        MarketStatusAction::ShortSellRestrictionChange => {
            enums_capnp::MarketStatusAction::ShortSellRestrictionChange
        }
        MarketStatusAction::NotAvailableForTrading => {
            enums_capnp::MarketStatusAction::NotAvailableForTrading
        }
    }
}

#[must_use]
pub fn market_status_action_from_capnp(
    value: enums_capnp::MarketStatusAction,
) -> MarketStatusAction {
    match value {
        enums_capnp::MarketStatusAction::None => MarketStatusAction::None,
        enums_capnp::MarketStatusAction::PreOpen => MarketStatusAction::PreOpen,
        enums_capnp::MarketStatusAction::PreCross => MarketStatusAction::PreCross,
        enums_capnp::MarketStatusAction::Quoting => MarketStatusAction::Quoting,
        enums_capnp::MarketStatusAction::Cross => MarketStatusAction::Cross,
        enums_capnp::MarketStatusAction::Rotation => MarketStatusAction::Rotation,
        enums_capnp::MarketStatusAction::NewPriceIndication => {
            MarketStatusAction::NewPriceIndication
        }
        enums_capnp::MarketStatusAction::Trading => MarketStatusAction::Trading,
        enums_capnp::MarketStatusAction::Halt => MarketStatusAction::Halt,
        enums_capnp::MarketStatusAction::Pause => MarketStatusAction::Pause,
        enums_capnp::MarketStatusAction::Suspend => MarketStatusAction::Suspend,
        enums_capnp::MarketStatusAction::PreClose => MarketStatusAction::PreClose,
        enums_capnp::MarketStatusAction::Close => MarketStatusAction::Close,
        enums_capnp::MarketStatusAction::PostClose => MarketStatusAction::PostClose,
        enums_capnp::MarketStatusAction::ShortSellRestrictionChange => {
            MarketStatusAction::ShortSellRestrictionChange
        }
        enums_capnp::MarketStatusAction::NotAvailableForTrading => {
            MarketStatusAction::NotAvailableForTrading
        }
    }
}

impl<'a> ToCapnp<'a> for Currency {
    type Builder = types_capnp::currency::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_code(self.code);
        builder.set_precision(self.precision);
        builder.set_iso4217(self.iso4217);
        builder.set_name(self.name);
        builder.set_currency_type(currency_type_to_capnp(self.currency_type));
    }
}

impl<'a> FromCapnp<'a> for Currency {
    type Reader = types_capnp::currency::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let code = reader.get_code()?.to_str()?;
        let precision = reader.get_precision();
        let iso4217 = reader.get_iso4217();
        let name = reader.get_name()?.to_str()?;
        let currency_type = currency_type_from_capnp(reader.get_currency_type()?);

        Ok(Self::new(code, precision, iso4217, name, currency_type))
    }
}

impl<'a> ToCapnp<'a> for Money {
    type Builder = types_capnp::money::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let mut raw_builder = builder.reborrow().init_raw();

        #[cfg(not(feature = "high-precision"))]
        {
            let raw_i128 = self.raw as i128;
            raw_builder.set_lo(raw_i128 as u64);
            raw_builder.set_hi((raw_i128 >> 64) as u64);
        }

        #[cfg(feature = "high-precision")]
        {
            raw_builder.set_lo(self.raw as u64);
            raw_builder.set_hi((self.raw >> 64) as u64);
        }

        let currency_builder = builder.init_currency();
        self.currency.to_capnp(currency_builder);
    }
}

impl<'a> FromCapnp<'a> for Money {
    type Reader = types_capnp::money::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let raw_reader = reader.get_raw()?;
        let lo = raw_reader.get_lo();
        let hi = raw_reader.get_hi();

        // Cast through i64 to sign-extend and preserve two's complement for negative values
        let raw_i128 = ((hi as i64 as i128) << 64) | (lo as i128);

        let currency_reader = reader.get_currency()?;
        let currency = Currency::from_capnp(currency_reader)?;

        #[cfg(not(feature = "high-precision"))]
        {
            let raw = i64::try_from(raw_i128).map_err(|_| -> Box<dyn Error> {
                "Money value overflows i64 in standard precision mode".into()
            })?;
            Ok(Self::from_raw(raw.into(), currency))
        }

        #[cfg(feature = "high-precision")]
        {
            Ok(Self::from_raw(raw_i128, currency))
        }
    }
}

impl<'a> ToCapnp<'a> for AccountBalance {
    type Builder = types_capnp::account_balance::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let total_builder = builder.reborrow().init_total();
        self.total.to_capnp(total_builder);

        let locked_builder = builder.reborrow().init_locked();
        self.locked.to_capnp(locked_builder);

        let free_builder = builder.init_free();
        self.free.to_capnp(free_builder);
    }
}

impl<'a> FromCapnp<'a> for AccountBalance {
    type Reader = types_capnp::account_balance::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let total_reader = reader.get_total()?;
        let total = Money::from_capnp(total_reader)?;

        let locked_reader = reader.get_locked()?;
        let locked = Money::from_capnp(locked_reader)?;

        let free_reader = reader.get_free()?;
        let free = Money::from_capnp(free_reader)?;

        Ok(Self::new(total, locked, free))
    }
}

impl<'a> ToCapnp<'a> for MarginBalance {
    type Builder = types_capnp::margin_balance::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let initial_builder = builder.reborrow().init_initial();
        self.initial.to_capnp(initial_builder);

        let maintenance_builder = builder.reborrow().init_maintenance();
        self.maintenance.to_capnp(maintenance_builder);

        let instrument_builder = builder.init_instrument();
        self.instrument_id.to_capnp(instrument_builder);
    }
}

impl<'a> FromCapnp<'a> for MarginBalance {
    type Reader = types_capnp::margin_balance::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let initial_reader = reader.get_initial()?;
        let initial = Money::from_capnp(initial_reader)?;

        let maintenance_reader = reader.get_maintenance()?;
        let maintenance = Money::from_capnp(maintenance_reader)?;

        let instrument_reader = reader.get_instrument()?;
        let instrument_id = InstrumentId::from_capnp(instrument_reader)?;

        Ok(Self::new(initial, maintenance, instrument_id))
    }
}

pub fn serialize_instrument_id(id: &InstrumentId) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::instrument_id::Builder>();
    id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_instrument_id(bytes: &[u8]) -> Result<InstrumentId, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<identifiers_capnp::instrument_id::Reader>()?;
    InstrumentId::from_capnp(root)
}

pub fn serialize_price(price: &Price) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::price::Builder>();
    price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_price(bytes: &[u8]) -> Result<Price, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::price::Reader>()?;
    Price::from_capnp(root)
}

pub fn serialize_quantity(qty: &Quantity) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::quantity::Builder>();
    qty.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_quantity(bytes: &[u8]) -> Result<Quantity, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::quantity::Reader>()?;
    Quantity::from_capnp(root)
}

pub fn serialize_currency(currency: &Currency) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::currency::Builder>();
    currency.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_currency(bytes: &[u8]) -> Result<Currency, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::currency::Reader>()?;
    Currency::from_capnp(root)
}

pub fn serialize_money(money: &Money) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::money::Builder>();
    money.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_money(bytes: &[u8]) -> Result<Money, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::money::Reader>()?;
    Money::from_capnp(root)
}

pub fn serialize_account_balance(balance: &AccountBalance) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::account_balance::Builder>();
    balance.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_account_balance(bytes: &[u8]) -> Result<AccountBalance, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::account_balance::Reader>()?;
    AccountBalance::from_capnp(root)
}

pub fn serialize_margin_balance(balance: &MarginBalance) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::margin_balance::Builder>();
    balance.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

pub fn deserialize_margin_balance(bytes: &[u8]) -> Result<MarginBalance, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<types_capnp::margin_balance::Reader>()?;
    MarginBalance::from_capnp(root)
}

impl<'a> ToCapnp<'a> for QuoteTick {
    type Builder = market_capnp::quote_tick::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let bid_price_builder = builder.reborrow().init_bid_price();
        self.bid_price.to_capnp(bid_price_builder);

        let ask_price_builder = builder.reborrow().init_ask_price();
        self.ask_price.to_capnp(ask_price_builder);

        let bid_size_builder = builder.reborrow().init_bid_size();
        self.bid_size.to_capnp(bid_size_builder);

        let ask_size_builder = builder.reborrow().init_ask_size();
        self.ask_size.to_capnp(ask_size_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for QuoteTick {
    type Reader = market_capnp::quote_tick::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let bid_price_reader = reader.get_bid_price()?;
        let bid_price = Price::from_capnp(bid_price_reader)?;

        let ask_price_reader = reader.get_ask_price()?;
        let ask_price = Price::from_capnp(ask_price_reader)?;

        let bid_size_reader = reader.get_bid_size()?;
        let bid_size = Quantity::from_capnp(bid_size_reader)?;

        let ask_size_reader = reader.get_ask_size()?;
        let ask_size = Quantity::from_capnp(ask_size_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for TradeTick {
    type Builder = market_capnp::trade_tick::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let price_builder = builder.reborrow().init_price();
        self.price.to_capnp(price_builder);

        let size_builder = builder.reborrow().init_size();
        self.size.to_capnp(size_builder);

        builder.set_aggressor_side(aggressor_side_to_capnp(self.aggressor_side));

        let trade_id_builder = builder.reborrow().init_trade_id();
        self.trade_id.to_capnp(trade_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for TradeTick {
    type Reader = market_capnp::trade_tick::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let price_reader = reader.get_price()?;
        let price = Price::from_capnp(price_reader)?;

        let size_reader = reader.get_size()?;
        let size = Quantity::from_capnp(size_reader)?;

        let aggressor_side = aggressor_side_from_capnp(reader.get_aggressor_side()?);

        let trade_id_reader = reader.get_trade_id()?;
        let trade_id = TradeId::from_capnp(trade_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for MarkPriceUpdate {
    type Builder = market_capnp::mark_price_update::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let mark_price_builder = builder.reborrow().init_mark_price();
        self.value.to_capnp(mark_price_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for MarkPriceUpdate {
    type Reader = market_capnp::mark_price_update::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let mark_price_reader = reader.get_mark_price()?;
        let value = Price::from_capnp(mark_price_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            value,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for IndexPriceUpdate {
    type Builder = market_capnp::index_price_update::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let index_price_builder = builder.reborrow().init_index_price();
        self.value.to_capnp(index_price_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for IndexPriceUpdate {
    type Reader = market_capnp::index_price_update::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let index_price_reader = reader.get_index_price()?;
        let value = Price::from_capnp(index_price_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            value,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for FundingRateUpdate {
    type Builder = market_capnp::funding_rate_update::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let rate_builder = builder.reborrow().init_rate();
        self.rate.to_capnp(rate_builder);

        let mut next_funding_time_builder = builder.reborrow().init_next_funding_time();
        next_funding_time_builder.set_value(self.next_funding_ns.map_or(0, |ns| *ns));

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for FundingRateUpdate {
    type Reader = market_capnp::funding_rate_update::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let rate_reader = reader.get_rate()?;
        let rate = Decimal::from_capnp(rate_reader)?;

        let next_funding_time_reader = reader.get_next_funding_time()?;
        let next_funding_time_value = next_funding_time_reader.get_value();
        let next_funding_ns = if next_funding_time_value == 0 {
            None
        } else {
            Some(next_funding_time_value.into())
        };

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            rate,
            next_funding_ns,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for InstrumentClose {
    type Builder = market_capnp::instrument_close::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let close_price_builder = builder.reborrow().init_close_price();
        self.close_price.to_capnp(close_price_builder);

        builder.set_close_type(instrument_close_type_to_capnp(self.close_type));

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for InstrumentClose {
    type Reader = market_capnp::instrument_close::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let close_price_reader = reader.get_close_price()?;
        let close_price = Price::from_capnp(close_price_reader)?;

        let close_type = instrument_close_type_from_capnp(reader.get_close_type()?);

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            close_price,
            close_type,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for InstrumentStatus {
    type Builder = market_capnp::instrument_status::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        builder.set_action(market_status_action_to_capnp(self.action));
        builder.set_reason(self.reason.as_ref().map_or("", |s| s.as_str()));
        builder.set_trading_event(self.trading_event.as_ref().map_or("", |s| s.as_str()));
        builder.set_is_trading(self.is_trading.unwrap_or(false));
        builder.set_is_quoting(self.is_quoting.unwrap_or(false));
        builder.set_is_short_sell_restricted(self.is_short_sell_restricted.unwrap_or(false));

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for InstrumentStatus {
    type Reader = market_capnp::instrument_status::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let action = market_status_action_from_capnp(reader.get_action()?);

        let reason_str = reader.get_reason()?.to_str()?;
        let reason = if reason_str.is_empty() {
            None
        } else {
            Some(Ustr::from(reason_str))
        };

        let trading_event_str = reader.get_trading_event()?.to_str()?;
        let trading_event = if trading_event_str.is_empty() {
            None
        } else {
            Some(Ustr::from(trading_event_str))
        };

        let is_trading = Some(reader.get_is_trading());
        let is_quoting = Some(reader.get_is_quoting());
        let is_short_sell_restricted = Some(reader.get_is_short_sell_restricted());

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            action,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reason,
            trading_event,
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        })
    }
}

impl<'a> ToCapnp<'a> for BarSpecification {
    type Builder = market_capnp::bar_spec::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        builder.set_step(self.step.get() as u32);
        builder.set_aggregation(bar_aggregation_to_capnp(self.aggregation));
        builder.set_price_type(price_type_to_capnp(self.price_type));
    }
}

impl<'a> FromCapnp<'a> for BarSpecification {
    type Reader = market_capnp::bar_spec::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        use std::num::NonZero;

        let step = reader.get_step();
        let step = NonZero::new(step as usize).ok_or("BarSpecification step must be non-zero")?;

        let aggregation = bar_aggregation_from_capnp(reader.get_aggregation()?);
        let price_type = price_type_from_capnp(reader.get_price_type()?);

        Ok(Self {
            step,
            aggregation,
            price_type,
        })
    }
}

impl<'a> ToCapnp<'a> for BarType {
    type Builder = market_capnp::bar_type::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id().to_capnp(instrument_id_builder);

        let spec_builder = builder.reborrow().init_spec();
        self.spec().to_capnp(spec_builder);

        builder.set_aggregation_source(aggregation_source_to_capnp(self.aggregation_source()));
    }
}

impl<'a> FromCapnp<'a> for BarType {
    type Reader = market_capnp::bar_type::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let spec_reader = reader.get_spec()?;
        let spec = BarSpecification::from_capnp(spec_reader)?;

        let aggregation_source = aggregation_source_from_capnp(reader.get_aggregation_source()?);

        Ok(Self::new(instrument_id, spec, aggregation_source))
    }
}

impl<'a> ToCapnp<'a> for Bar {
    type Builder = market_capnp::bar::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let bar_type_builder = builder.reborrow().init_bar_type();
        self.bar_type.to_capnp(bar_type_builder);

        let open_builder = builder.reborrow().init_open();
        self.open.to_capnp(open_builder);

        let high_builder = builder.reborrow().init_high();
        self.high.to_capnp(high_builder);

        let low_builder = builder.reborrow().init_low();
        self.low.to_capnp(low_builder);

        let close_builder = builder.reborrow().init_close();
        self.close.to_capnp(close_builder);

        let volume_builder = builder.reborrow().init_volume();
        self.volume.to_capnp(volume_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for Bar {
    type Reader = market_capnp::bar::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let bar_type_reader = reader.get_bar_type()?;
        let bar_type = BarType::from_capnp(bar_type_reader)?;

        let open_reader = reader.get_open()?;
        let open = Price::from_capnp(open_reader)?;

        let high_reader = reader.get_high()?;
        let high = Price::from_capnp(high_reader)?;

        let low_reader = reader.get_low()?;
        let low = Price::from_capnp(low_reader)?;

        let close_reader = reader.get_close()?;
        let close = Price::from_capnp(close_reader)?;

        let volume_reader = reader.get_volume()?;
        let volume = Quantity::from_capnp(volume_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for BookOrder {
    type Builder = market_capnp::book_order::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let price_builder = builder.reborrow().init_price();
        self.price.to_capnp(price_builder);

        let size_builder = builder.reborrow().init_size();
        self.size.to_capnp(size_builder);

        builder.set_side(order_side_to_capnp(self.side));
        builder.set_order_id(self.order_id);
    }
}

impl<'a> FromCapnp<'a> for BookOrder {
    type Reader = market_capnp::book_order::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let price_reader = reader.get_price()?;
        let price = Price::from_capnp(price_reader)?;

        let size_reader = reader.get_size()?;
        let size = Quantity::from_capnp(size_reader)?;

        let side = order_side_from_capnp(reader.get_side()?);
        let order_id = reader.get_order_id();

        Ok(Self {
            side,
            price,
            size,
            order_id,
        })
    }
}

impl<'a> ToCapnp<'a> for OrderBookDelta {
    type Builder = market_capnp::order_book_delta::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        builder.set_action(book_action_to_capnp(self.action));

        let order_builder = builder.reborrow().init_order();
        self.order.to_capnp(order_builder);

        builder.set_flags(self.flags);
        builder.set_sequence(self.sequence);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderBookDelta {
    type Reader = market_capnp::order_book_delta::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let action = book_action_from_capnp(reader.get_action()?);

        let order_reader = reader.get_order()?;
        let order = BookOrder::from_capnp(order_reader)?;

        let flags = reader.get_flags();
        let sequence = reader.get_sequence();

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for OrderBookDeltas {
    type Builder = market_capnp::order_book_deltas::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let mut deltas_builder = builder.reborrow().init_deltas(self.deltas.len() as u32);
        for (i, delta) in self.deltas.iter().enumerate() {
            let delta_builder = deltas_builder.reborrow().get(i as u32);
            delta.to_capnp(delta_builder);
        }

        builder.set_flags(self.flags);
        builder.set_sequence(self.sequence);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderBookDeltas {
    type Reader = market_capnp::order_book_deltas::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let deltas_reader = reader.get_deltas()?;
        let mut deltas = Vec::with_capacity(deltas_reader.len() as usize);
        for delta_reader in deltas_reader.iter() {
            let delta = OrderBookDelta::from_capnp(delta_reader)?;
            deltas.push(delta);
        }

        let flags = reader.get_flags();
        let sequence = reader.get_sequence();

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            deltas,
            flags,
            sequence,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for OrderBookDepth10 {
    type Builder = market_capnp::order_book_depth10::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        // Convert bids (BookOrder array to BookLevel list)
        let mut bids_builder = builder.reborrow().init_bids(self.bids.len() as u32);
        for (i, bid) in self.bids.iter().enumerate() {
            let mut level_builder = bids_builder.reborrow().get(i as u32);
            let price_builder = level_builder.reborrow().init_price();
            bid.price.to_capnp(price_builder);
            let size_builder = level_builder.init_size();
            bid.size.to_capnp(size_builder);
        }

        // Convert asks (BookOrder array to BookLevel list)
        let mut asks_builder = builder.reborrow().init_asks(self.asks.len() as u32);
        for (i, ask) in self.asks.iter().enumerate() {
            let mut level_builder = asks_builder.reborrow().get(i as u32);
            let price_builder = level_builder.reborrow().init_price();
            ask.price.to_capnp(price_builder);
            let size_builder = level_builder.init_size();
            ask.size.to_capnp(size_builder);
        }

        // Convert counts
        let mut bid_counts_builder = builder
            .reborrow()
            .init_bid_counts(self.bid_counts.len() as u32);
        for (i, &count) in self.bid_counts.iter().enumerate() {
            bid_counts_builder.set(i as u32, count);
        }

        let mut ask_counts_builder = builder
            .reborrow()
            .init_ask_counts(self.ask_counts.len() as u32);
        for (i, &count) in self.ask_counts.iter().enumerate() {
            ask_counts_builder.set(i as u32, count);
        }

        builder.set_flags(self.flags);
        builder.set_sequence(self.sequence);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderBookDepth10 {
    type Reader = market_capnp::order_book_depth10::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        use nautilus_model::data::order::NULL_ORDER;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        // Convert bids (BookLevel list to BookOrder array)
        let bids_reader = reader.get_bids()?;
        let mut bids = [NULL_ORDER; 10];
        for (i, level_reader) in bids_reader.iter().enumerate().take(10) {
            let price_reader = level_reader.get_price()?;
            let price = Price::from_capnp(price_reader)?;

            let size_reader = level_reader.get_size()?;
            let size = Quantity::from_capnp(size_reader)?;

            bids[i] = BookOrder::new(OrderSide::Buy, price, size, 0);
        }

        // Convert asks (BookLevel list to BookOrder array)
        let asks_reader = reader.get_asks()?;
        let mut asks = [NULL_ORDER; 10];
        for (i, level_reader) in asks_reader.iter().enumerate().take(10) {
            let price_reader = level_reader.get_price()?;
            let price = Price::from_capnp(price_reader)?;

            let size_reader = level_reader.get_size()?;
            let size = Quantity::from_capnp(size_reader)?;

            asks[i] = BookOrder::new(OrderSide::Sell, price, size, 0);
        }

        // Convert counts
        let bid_counts_reader = reader.get_bid_counts()?;
        let mut bid_counts = [0u32; 10];
        for (i, count) in bid_counts_reader.iter().enumerate().take(10) {
            bid_counts[i] = count;
        }

        let ask_counts_reader = reader.get_ask_counts()?;
        let mut ask_counts = [0u32; 10];
        for (i, count) in ask_counts_reader.iter().enumerate().take(10) {
            ask_counts[i] = count;
        }

        let flags = reader.get_flags();
        let sequence = reader.get_sequence();

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

// ================================================================================================
// Order Events
// ================================================================================================

impl<'a> ToCapnp<'a> for OrderDenied {
    type Builder = order_capnp::order_denied::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        builder.set_reason(self.reason.as_str());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderDenied {
    type Reader = order_capnp::order_denied::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let reason = Ustr::from(reader.get_reason()?.to_str()?);

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event: ts_init.into(), // System event - ts_event = ts_init
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for OrderEmulated {
    type Builder = order_capnp::order_emulated::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderEmulated {
    type Reader = order_capnp::order_emulated::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event: ts_init.into(), // System event - ts_event = ts_init
            ts_init: ts_init.into(),
        })
    }
}

impl<'a> ToCapnp<'a> for OrderReleased {
    type Builder = order_capnp::order_released::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let released_price_builder = builder.reborrow().init_released_price();
        self.released_price.to_capnp(released_price_builder);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderReleased {
    type Reader = order_capnp::order_released::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let released_price_reader = reader.get_released_price()?;
        let released_price = Price::from_capnp(released_price_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            released_price,
            event_id,
            ts_event: ts_init.into(), // System event - ts_event = ts_init
            ts_init: ts_init.into(),
        })
    }
}

// OrderSubmitted
impl<'a> ToCapnp<'a> for OrderSubmitted {
    type Builder = order_capnp::order_submitted::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for OrderSubmitted {
    type Reader = order_capnp::order_submitted::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

// OrderAccepted
impl<'a> ToCapnp<'a> for OrderAccepted {
    type Builder = order_capnp::order_accepted::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.venue_order_id
            .write_capnp(|| builder.reborrow().init_venue_order_id());
        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderAccepted {
    type Reader = order_capnp::order_accepted::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id_reader = reader.get_venue_order_id()?;
        let venue_order_id = VenueOrderId::from_capnp(venue_order_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderRejected
impl<'a> ToCapnp<'a> for OrderRejected {
    type Builder = order_capnp::order_rejected::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        builder.set_reason(self.reason.as_str());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
        builder.set_due_post_only(self.due_post_only != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderRejected {
    type Reader = order_capnp::order_rejected::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let reason = Ustr::from(reader.get_reason()?.to_str()?);

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;
        let due_post_only = reader.get_due_post_only() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            reason,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
            due_post_only,
        })
    }
}

// OrderCanceled
impl<'a> ToCapnp<'a> for OrderCanceled {
    type Builder = order_capnp::order_canceled::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        if let Some(ref venue_order_id) = self.venue_order_id {
            let venue_order_id_builder = builder.reborrow().init_venue_order_id();
            venue_order_id.to_capnp(venue_order_id_builder);
        }

        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderCanceled {
    type Reader = order_capnp::order_canceled::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderExpired
impl<'a> ToCapnp<'a> for OrderExpired {
    type Builder = order_capnp::order_expired::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        if let Some(ref venue_order_id) = self.venue_order_id {
            let venue_order_id_builder = builder.reborrow().init_venue_order_id();
            venue_order_id.to_capnp(venue_order_id_builder);
        }

        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderExpired {
    type Reader = order_capnp::order_expired::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderTriggered
impl<'a> ToCapnp<'a> for OrderTriggered {
    type Builder = order_capnp::order_triggered::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        if let Some(ref venue_order_id) = self.venue_order_id {
            let venue_order_id_builder = builder.reborrow().init_venue_order_id();
            venue_order_id.to_capnp(venue_order_id_builder);
        }

        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderTriggered {
    type Reader = order_capnp::order_triggered::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderPendingUpdate
impl<'a> ToCapnp<'a> for OrderPendingUpdate {
    type Builder = order_capnp::order_pending_update::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        if let Some(ref venue_order_id) = self.venue_order_id {
            let venue_order_id_builder = builder.reborrow().init_venue_order_id();
            venue_order_id.to_capnp(venue_order_id_builder);
        }

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderPendingUpdate {
    type Reader = order_capnp::order_pending_update::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = if reader.has_venue_order_id() {
            let venue_order_id_reader = reader.get_venue_order_id()?;
            Some(VenueOrderId::from_capnp(venue_order_id_reader)?)
        } else {
            None
        };

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderPendingCancel
impl<'a> ToCapnp<'a> for OrderPendingCancel {
    type Builder = order_capnp::order_pending_cancel::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        if let Some(ref venue_order_id) = self.venue_order_id {
            let venue_order_id_builder = builder.reborrow().init_venue_order_id();
            venue_order_id.to_capnp(venue_order_id_builder);
        }

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderPendingCancel {
    type Reader = order_capnp::order_pending_cancel::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = if reader.has_venue_order_id() {
            let venue_order_id_reader = reader.get_venue_order_id()?;
            Some(VenueOrderId::from_capnp(venue_order_id_reader)?)
        } else {
            None
        };

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderModifyRejected
impl<'a> ToCapnp<'a> for OrderModifyRejected {
    type Builder = order_capnp::order_modify_rejected::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.venue_order_id
            .write_capnp(|| builder.reborrow().init_venue_order_id());
        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        builder.set_reason(self.reason.as_str());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderModifyRejected {
    type Reader = order_capnp::order_modify_rejected::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let reason = Ustr::from(reader.get_reason()?.to_str()?);

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            reason,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderCancelRejected
impl<'a> ToCapnp<'a> for OrderCancelRejected {
    type Builder = order_capnp::order_cancel_rejected::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.venue_order_id
            .write_capnp(|| builder.reborrow().init_venue_order_id());
        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        builder.set_reason(self.reason.as_str());

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderCancelRejected {
    type Reader = order_capnp::order_cancel_rejected::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let reason = Ustr::from(reader.get_reason()?.to_str()?);

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            reason,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderUpdated
impl<'a> ToCapnp<'a> for OrderUpdated {
    type Builder = order_capnp::order_updated::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        self.venue_order_id
            .write_capnp(|| builder.reborrow().init_venue_order_id());
        self.account_id
            .write_capnp(|| builder.reborrow().init_account_id());

        let quantity_builder = builder.reborrow().init_quantity();
        self.quantity.to_capnp(quantity_builder);

        if let Some(price) = self.price {
            let price_builder = builder.reborrow().init_price();
            price.to_capnp(price_builder);
        }

        if let Some(trigger_price) = self.trigger_price {
            let trigger_price_builder = builder.reborrow().init_trigger_price();
            trigger_price.to_capnp(trigger_price_builder);
        }

        if let Some(protection_price) = self.protection_price {
            let protection_price_builder = builder.reborrow().init_protection_price();
            protection_price.to_capnp(protection_price_builder);
        }

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation != 0);
    }
}

impl<'a> FromCapnp<'a> for OrderUpdated {
    type Reader = order_capnp::order_updated::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id = read_optional_from_capnp(
            || reader.has_venue_order_id(),
            || reader.get_venue_order_id(),
        )?;

        let account_id =
            read_optional_from_capnp(|| reader.has_account_id(), || reader.get_account_id())?;

        let quantity_reader = reader.get_quantity()?;
        let quantity = Quantity::from_capnp(quantity_reader)?;

        let price = if reader.has_price() {
            let price_reader = reader.get_price()?;
            Some(Price::from_capnp(price_reader)?)
        } else {
            None
        };

        let trigger_price = if reader.has_trigger_price() {
            let trigger_price_reader = reader.get_trigger_price()?;
            Some(Price::from_capnp(trigger_price_reader)?)
        } else {
            None
        };

        let protection_price = if reader.has_protection_price() {
            let protection_price_reader = reader.get_protection_price()?;
            Some(Price::from_capnp(protection_price_reader)?)
        } else {
            None
        };

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation() as u8;

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            quantity,
            price,
            trigger_price,
            protection_price,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
        })
    }
}

// OrderFilled
impl<'a> ToCapnp<'a> for OrderFilled {
    type Builder = order_capnp::order_filled::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let venue_order_id_builder = builder.reborrow().init_venue_order_id();
        self.venue_order_id.to_capnp(venue_order_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let trade_id_builder = builder.reborrow().init_trade_id();
        self.trade_id.to_capnp(trade_id_builder);

        builder.set_order_side(order_side_to_capnp(self.order_side));
        builder.set_order_type(order_type_to_capnp(self.order_type));

        let last_qty_builder = builder.reborrow().init_last_qty();
        self.last_qty.to_capnp(last_qty_builder);

        let last_px_builder = builder.reborrow().init_last_px();
        self.last_px.to_capnp(last_px_builder);

        let currency_builder = builder.reborrow().init_currency();
        self.currency.to_capnp(currency_builder);

        builder.set_liquidity_side(liquidity_side_to_capnp(self.liquidity_side));

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        builder.set_reconciliation(self.reconciliation);

        if let Some(position_id) = &self.position_id {
            let position_id_builder = builder.reborrow().init_position_id();
            position_id.to_capnp(position_id_builder);
        }

        if let Some(commission) = &self.commission {
            let commission_builder = builder.reborrow().init_commission();
            commission.to_capnp(commission_builder);
        }
    }
}

impl<'a> FromCapnp<'a> for OrderFilled {
    type Reader = order_capnp::order_filled::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let venue_order_id_reader = reader.get_venue_order_id()?;
        let venue_order_id = VenueOrderId::from_capnp(venue_order_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let trade_id_reader = reader.get_trade_id()?;
        let trade_id = TradeId::from_capnp(trade_id_reader)?;

        let order_side = order_side_from_capnp(reader.get_order_side()?);
        let order_type = order_type_from_capnp(reader.get_order_type()?);

        let last_qty_reader = reader.get_last_qty()?;
        let last_qty = Quantity::from_capnp(last_qty_reader)?;

        let last_px_reader = reader.get_last_px()?;
        let last_px = Price::from_capnp(last_px_reader)?;

        let currency_reader = reader.get_currency()?;
        let currency = Currency::from_capnp(currency_reader)?;

        let liquidity_side = liquidity_side_from_capnp(reader.get_liquidity_side()?);

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        let reconciliation = reader.get_reconciliation();

        let position_id = if reader.has_position_id() {
            let position_id_reader = reader.get_position_id()?;
            Some(PositionId::from_capnp(position_id_reader)?)
        } else {
            None
        };

        let commission = if reader.has_commission() {
            let commission_reader = reader.get_commission()?;
            Some(Money::from_capnp(commission_reader)?)
        } else {
            None
        };

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            currency,
            liquidity_side,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            reconciliation,
            position_id,
            commission,
        })
    }
}

// OrderInitialized - seed event
impl<'a> ToCapnp<'a> for OrderInitialized {
    type Builder = order_capnp::order_initialized::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        builder.set_order_side(order_side_to_capnp(self.order_side));
        builder.set_order_type(order_type_to_capnp(self.order_type));

        let quantity_builder = builder.reborrow().init_quantity();
        self.quantity.to_capnp(quantity_builder);

        builder.set_time_in_force(time_in_force_to_capnp(self.time_in_force));
        builder.set_post_only(self.post_only);
        builder.set_reduce_only(self.reduce_only);
        builder.set_quote_quantity(self.quote_quantity);
        builder.set_reconciliation(self.reconciliation);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);

        // Optional fields
        if let Some(price) = self.price {
            let price_builder = builder.reborrow().init_price();
            price.to_capnp(price_builder);
        }

        if let Some(trigger_price) = self.trigger_price {
            let trigger_price_builder = builder.reborrow().init_trigger_price();
            trigger_price.to_capnp(trigger_price_builder);
        }

        if let Some(trigger_type) = self.trigger_type {
            builder.set_trigger_type(trigger_type_to_capnp(trigger_type));
        }

        if let Some(limit_offset) = self.limit_offset {
            let limit_offset_builder = builder.reborrow().init_limit_offset();
            limit_offset.to_capnp(limit_offset_builder);
        }

        if let Some(trailing_offset) = self.trailing_offset {
            let trailing_offset_builder = builder.reborrow().init_trailing_offset();
            trailing_offset.to_capnp(trailing_offset_builder);
        }

        if let Some(trailing_offset_type) = self.trailing_offset_type {
            builder.set_trailing_offset_type(trailing_offset_type_to_capnp(trailing_offset_type));
        }

        if let Some(expire_time) = self.expire_time {
            let mut expire_time_builder = builder.reborrow().init_expire_time();
            expire_time_builder.set_value(*expire_time);
        }

        if let Some(display_qty) = self.display_qty {
            let display_qty_builder = builder.reborrow().init_display_qty();
            display_qty.to_capnp(display_qty_builder);
        }

        if let Some(emulation_trigger) = self.emulation_trigger {
            builder.set_emulation_trigger(trigger_type_to_capnp(emulation_trigger));
        }

        if let Some(trigger_instrument_id) = &self.trigger_instrument_id {
            let trigger_instrument_id_builder = builder.reborrow().init_trigger_instrument_id();
            trigger_instrument_id.to_capnp(trigger_instrument_id_builder);
        }

        if let Some(contingency_type) = self.contingency_type {
            builder.set_contingency_type(contingency_type_to_capnp(contingency_type));
        }

        if let Some(order_list_id) = &self.order_list_id {
            let order_list_id_builder = builder.reborrow().init_order_list_id();
            order_list_id.to_capnp(order_list_id_builder);
        }

        if let Some(linked_order_ids) = &self.linked_order_ids {
            let mut linked_order_ids_builder = builder
                .reborrow()
                .init_linked_order_ids(linked_order_ids.len() as u32);
            for (i, order_id) in linked_order_ids.iter().enumerate() {
                let order_id_builder = linked_order_ids_builder.reborrow().get(i as u32);
                order_id.to_capnp(order_id_builder);
            }
        }

        if let Some(parent_order_id) = &self.parent_order_id {
            let parent_order_id_builder = builder.reborrow().init_parent_order_id();
            parent_order_id.to_capnp(parent_order_id_builder);
        }

        if let Some(exec_algorithm_id) = &self.exec_algorithm_id {
            let exec_algorithm_id_builder = builder.reborrow().init_exec_algorithm_id();
            exec_algorithm_id.to_capnp(exec_algorithm_id_builder);
        }

        if let Some(exec_algorithm_params) = &self.exec_algorithm_params {
            let mut params_builder = builder.reborrow().init_exec_algorithm_params();
            let mut entries_builder = params_builder
                .reborrow()
                .init_entries(exec_algorithm_params.len() as u32);
            for (i, (key, value)) in exec_algorithm_params.iter().enumerate() {
                let mut entry_builder = entries_builder.reborrow().get(i as u32);
                entry_builder.set_key(key.as_str());
                entry_builder.set_value(value.as_str());
            }
        }

        if let Some(exec_spawn_id) = &self.exec_spawn_id {
            let exec_spawn_id_builder = builder.reborrow().init_exec_spawn_id();
            exec_spawn_id.to_capnp(exec_spawn_id_builder);
        }

        if let Some(tags) = &self.tags {
            let mut tags_builder = builder.reborrow().init_tags(tags.len() as u32);
            for (i, tag) in tags.iter().enumerate() {
                tags_builder.set(i as u32, tag.as_str());
            }
        }
    }
}

impl<'a> FromCapnp<'a> for OrderInitialized {
    type Reader = order_capnp::order_initialized::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let client_order_id_reader = reader.get_client_order_id()?;
        let client_order_id = ClientOrderId::from_capnp(client_order_id_reader)?;

        let order_side = order_side_from_capnp(reader.get_order_side()?);
        let order_type = order_type_from_capnp(reader.get_order_type()?);

        let quantity_reader = reader.get_quantity()?;
        let quantity = Quantity::from_capnp(quantity_reader)?;

        let time_in_force = time_in_force_from_capnp(reader.get_time_in_force()?);
        let post_only = reader.get_post_only();
        let reduce_only = reader.get_reduce_only();
        let quote_quantity = reader.get_quote_quantity();
        let reconciliation = reader.get_reconciliation();

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        // Optional fields
        let price = if reader.has_price() {
            let price_reader = reader.get_price()?;
            Some(Price::from_capnp(price_reader)?)
        } else {
            None
        };

        let trigger_price = if reader.has_trigger_price() {
            let trigger_price_reader = reader.get_trigger_price()?;
            Some(Price::from_capnp(trigger_price_reader)?)
        } else {
            None
        };

        let trigger_type = match reader.get_trigger_type()? {
            enums_capnp::TriggerType::NoTrigger => None,
            other => Some(trigger_type_from_capnp(other)),
        };

        let limit_offset = if reader.has_limit_offset() {
            let limit_offset_reader = reader.get_limit_offset()?;
            Some(Decimal::from_capnp(limit_offset_reader)?)
        } else {
            None
        };

        let trailing_offset = if reader.has_trailing_offset() {
            let trailing_offset_reader = reader.get_trailing_offset()?;
            Some(Decimal::from_capnp(trailing_offset_reader)?)
        } else {
            None
        };

        let trailing_offset_type = match reader.get_trailing_offset_type()? {
            enums_capnp::TrailingOffsetType::NoTrailingOffset => None,
            other => Some(trailing_offset_type_from_capnp(other)),
        };

        let expire_time = if reader.has_expire_time() {
            let expire_time_reader = reader.get_expire_time()?;
            let value = expire_time_reader.get_value();
            Some(value.into())
        } else {
            None
        };

        let display_qty = if reader.has_display_qty() {
            let display_qty_reader = reader.get_display_qty()?;
            Some(Quantity::from_capnp(display_qty_reader)?)
        } else {
            None
        };

        let emulation_trigger = match reader.get_emulation_trigger()? {
            enums_capnp::TriggerType::NoTrigger => None,
            other => Some(trigger_type_from_capnp(other)),
        };

        let trigger_instrument_id = if reader.has_trigger_instrument_id() {
            let trigger_instrument_id_reader = reader.get_trigger_instrument_id()?;
            Some(InstrumentId::from_capnp(trigger_instrument_id_reader)?)
        } else {
            None
        };

        let contingency_type = match reader.get_contingency_type()? {
            enums_capnp::ContingencyType::NoContingency => None,
            other => Some(contingency_type_from_capnp(other)),
        };

        let order_list_id = if reader.has_order_list_id() {
            let order_list_id_reader = reader.get_order_list_id()?;
            Some(OrderListId::from_capnp(order_list_id_reader)?)
        } else {
            None
        };

        let linked_order_ids = if reader.has_linked_order_ids() {
            let linked_order_ids_reader = reader.get_linked_order_ids()?;
            let mut linked_order_ids = Vec::new();
            for order_id_reader in linked_order_ids_reader.iter() {
                linked_order_ids.push(ClientOrderId::from_capnp(order_id_reader)?);
            }
            Some(linked_order_ids)
        } else {
            None
        };

        let parent_order_id = if reader.has_parent_order_id() {
            let parent_order_id_reader = reader.get_parent_order_id()?;
            Some(ClientOrderId::from_capnp(parent_order_id_reader)?)
        } else {
            None
        };

        let exec_algorithm_id = if reader.has_exec_algorithm_id() {
            let exec_algorithm_id_reader = reader.get_exec_algorithm_id()?;
            Some(ExecAlgorithmId::from_capnp(exec_algorithm_id_reader)?)
        } else {
            None
        };

        let exec_algorithm_params = if reader.has_exec_algorithm_params() {
            let params_reader = reader.get_exec_algorithm_params()?;
            let entries_reader = params_reader.get_entries()?;
            let mut params = IndexMap::new();
            for entry_reader in entries_reader.iter() {
                let key = Ustr::from(entry_reader.get_key()?.to_str()?);
                let value = Ustr::from(entry_reader.get_value()?.to_str()?);
                params.insert(key, value);
            }
            Some(params)
        } else {
            None
        };

        let exec_spawn_id = if reader.has_exec_spawn_id() {
            let exec_spawn_id_reader = reader.get_exec_spawn_id()?;
            Some(ClientOrderId::from_capnp(exec_spawn_id_reader)?)
        } else {
            None
        };

        let tags = if reader.has_tags() {
            let tags_reader = reader.get_tags()?;
            let mut tags = Vec::new();
            for tag in tags_reader.iter() {
                tags.push(Ustr::from(tag?.to_str()?));
            }
            Some(tags)
        } else {
            None
        };

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            reconciliation,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        })
    }
}

// ================================================================================================
// Position Events
// ================================================================================================

// PositionOpened
impl<'a> ToCapnp<'a> for PositionOpened {
    type Builder = position_capnp::position_opened::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let position_id_builder = builder.reborrow().init_position_id();
        self.position_id.to_capnp(position_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let opening_order_id_builder = builder.reborrow().init_opening_order_id();
        self.opening_order_id.to_capnp(opening_order_id_builder);

        builder.set_entry(order_side_to_capnp(self.entry));
        builder.set_side(position_side_to_capnp(self.side));
        builder.set_signed_qty(self.signed_qty);

        let quantity_builder = builder.reborrow().init_quantity();
        self.quantity.to_capnp(quantity_builder);

        let last_qty_builder = builder.reborrow().init_last_qty();
        self.last_qty.to_capnp(last_qty_builder);

        let last_px_builder = builder.reborrow().init_last_px();
        self.last_px.to_capnp(last_px_builder);

        let currency_builder = builder.reborrow().init_currency();
        self.currency.to_capnp(currency_builder);

        builder.set_avg_px_open(self.avg_px_open);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for PositionOpened {
    type Reader = position_capnp::position_opened::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let position_id_reader = reader.get_position_id()?;
        let position_id = PositionId::from_capnp(position_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let opening_order_id_reader = reader.get_opening_order_id()?;
        let opening_order_id = ClientOrderId::from_capnp(opening_order_id_reader)?;

        let entry = order_side_from_capnp(reader.get_entry()?);
        let side = position_side_from_capnp(reader.get_side()?);
        let signed_qty = reader.get_signed_qty();

        let quantity_reader = reader.get_quantity()?;
        let quantity = Quantity::from_capnp(quantity_reader)?;

        let last_qty_reader = reader.get_last_qty()?;
        let last_qty = Quantity::from_capnp(last_qty_reader)?;

        let last_px_reader = reader.get_last_px()?;
        let last_px = Price::from_capnp(last_px_reader)?;

        let currency_reader = reader.get_currency()?;
        let currency = Currency::from_capnp(currency_reader)?;

        let avg_px_open = reader.get_avg_px_open();

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            entry,
            side,
            signed_qty,
            quantity,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

// PositionChanged
impl<'a> ToCapnp<'a> for PositionChanged {
    type Builder = position_capnp::position_changed::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let position_id_builder = builder.reborrow().init_position_id();
        self.position_id.to_capnp(position_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let opening_order_id_builder = builder.reborrow().init_opening_order_id();
        self.opening_order_id.to_capnp(opening_order_id_builder);

        builder.set_entry(order_side_to_capnp(self.entry));
        builder.set_side(position_side_to_capnp(self.side));
        builder.set_signed_qty(self.signed_qty);

        let quantity_builder = builder.reborrow().init_quantity();
        self.quantity.to_capnp(quantity_builder);

        let peak_quantity_builder = builder.reborrow().init_peak_quantity();
        self.peak_quantity.to_capnp(peak_quantity_builder);

        let last_qty_builder = builder.reborrow().init_last_qty();
        self.last_qty.to_capnp(last_qty_builder);

        let last_px_builder = builder.reborrow().init_last_px();
        self.last_px.to_capnp(last_px_builder);

        let currency_builder = builder.reborrow().init_currency();
        self.currency.to_capnp(currency_builder);

        builder.set_avg_px_open(self.avg_px_open);
        builder.set_avg_px_close(self.avg_px_close.unwrap_or(f64::NAN));
        builder.set_realized_return(self.realized_return);

        self.realized_pnl
            .write_capnp(|| builder.reborrow().init_realized_pnl());

        let unrealized_pnl_builder = builder.reborrow().init_unrealized_pnl();
        self.unrealized_pnl.to_capnp(unrealized_pnl_builder);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_opened_builder = builder.reborrow().init_ts_opened();
        ts_opened_builder.set_value(*self.ts_opened);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for PositionChanged {
    type Reader = position_capnp::position_changed::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let position_id_reader = reader.get_position_id()?;
        let position_id = PositionId::from_capnp(position_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let opening_order_id_reader = reader.get_opening_order_id()?;
        let opening_order_id = ClientOrderId::from_capnp(opening_order_id_reader)?;

        let entry = order_side_from_capnp(reader.get_entry()?);
        let side = position_side_from_capnp(reader.get_side()?);
        let signed_qty = reader.get_signed_qty();

        let quantity_reader = reader.get_quantity()?;
        let quantity = Quantity::from_capnp(quantity_reader)?;

        let peak_quantity_reader = reader.get_peak_quantity()?;
        let peak_quantity = Quantity::from_capnp(peak_quantity_reader)?;

        let last_qty_reader = reader.get_last_qty()?;
        let last_qty = Quantity::from_capnp(last_qty_reader)?;

        let last_px_reader = reader.get_last_px()?;
        let last_px = Price::from_capnp(last_px_reader)?;

        let currency_reader = reader.get_currency()?;
        let currency = Currency::from_capnp(currency_reader)?;

        let avg_px_open = reader.get_avg_px_open();
        let avg_px_close = {
            let value = reader.get_avg_px_close();
            if value.is_nan() { None } else { Some(value) }
        };
        let realized_return = reader.get_realized_return();

        let realized_pnl = if reader.has_realized_pnl() {
            let realized_pnl_reader = reader.get_realized_pnl()?;
            Some(Money::from_capnp(realized_pnl_reader)?)
        } else {
            None
        };

        let unrealized_pnl_reader = reader.get_unrealized_pnl()?;
        let unrealized_pnl = Money::from_capnp(unrealized_pnl_reader)?;

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_opened_reader = reader.get_ts_opened()?;
        let ts_opened = ts_opened_reader.get_value();

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            entry,
            side,
            signed_qty,
            quantity,
            peak_quantity,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            avg_px_close,
            realized_return,
            realized_pnl,
            unrealized_pnl,
            event_id,
            ts_opened: ts_opened.into(),
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

// PositionClosed
impl<'a> ToCapnp<'a> for PositionClosed {
    type Builder = position_capnp::position_closed::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let position_id_builder = builder.reborrow().init_position_id();
        self.position_id.to_capnp(position_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let opening_order_id_builder = builder.reborrow().init_opening_order_id();
        self.opening_order_id.to_capnp(opening_order_id_builder);

        self.closing_order_id
            .write_capnp(|| builder.reborrow().init_closing_order_id());

        builder.set_entry(order_side_to_capnp(self.entry));
        builder.set_side(position_side_to_capnp(self.side));
        builder.set_signed_qty(self.signed_qty);

        let quantity_builder = builder.reborrow().init_quantity();
        self.quantity.to_capnp(quantity_builder);

        let peak_quantity_builder = builder.reborrow().init_peak_quantity();
        self.peak_quantity.to_capnp(peak_quantity_builder);

        let last_qty_builder = builder.reborrow().init_last_qty();
        self.last_qty.to_capnp(last_qty_builder);

        let last_px_builder = builder.reborrow().init_last_px();
        self.last_px.to_capnp(last_px_builder);

        let currency_builder = builder.reborrow().init_currency();
        self.currency.to_capnp(currency_builder);

        builder.set_avg_px_open(self.avg_px_open);
        builder.set_avg_px_close(self.avg_px_close.unwrap_or(f64::NAN));
        builder.set_realized_return(self.realized_return);

        self.realized_pnl
            .write_capnp(|| builder.reborrow().init_realized_pnl());

        let unrealized_pnl_builder = builder.reborrow().init_unrealized_pnl();
        self.unrealized_pnl.to_capnp(unrealized_pnl_builder);

        builder.set_duration(self.duration);

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_opened_builder = builder.reborrow().init_ts_opened();
        ts_opened_builder.set_value(*self.ts_opened);

        if let Some(ts_closed) = self.ts_closed {
            let mut ts_closed_builder = builder.reborrow().init_ts_closed();
            ts_closed_builder.set_value(*ts_closed);
        }

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for PositionClosed {
    type Reader = position_capnp::position_closed::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let position_id_reader = reader.get_position_id()?;
        let position_id = PositionId::from_capnp(position_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let opening_order_id_reader = reader.get_opening_order_id()?;
        let opening_order_id = ClientOrderId::from_capnp(opening_order_id_reader)?;

        let closing_order_id = read_optional_from_capnp(
            || reader.has_closing_order_id(),
            || reader.get_closing_order_id(),
        )?;

        let entry = order_side_from_capnp(reader.get_entry()?);
        let side = position_side_from_capnp(reader.get_side()?);
        let signed_qty = reader.get_signed_qty();

        let quantity_reader = reader.get_quantity()?;
        let quantity = Quantity::from_capnp(quantity_reader)?;

        let peak_quantity_reader = reader.get_peak_quantity()?;
        let peak_quantity = Quantity::from_capnp(peak_quantity_reader)?;

        let last_qty_reader = reader.get_last_qty()?;
        let last_qty = Quantity::from_capnp(last_qty_reader)?;

        let last_px_reader = reader.get_last_px()?;
        let last_px = Price::from_capnp(last_px_reader)?;

        let currency_reader = reader.get_currency()?;
        let currency = Currency::from_capnp(currency_reader)?;

        let avg_px_open = reader.get_avg_px_open();
        let avg_px_close = {
            let value = reader.get_avg_px_close();
            if value.is_nan() { None } else { Some(value) }
        };
        let realized_return = reader.get_realized_return();

        let realized_pnl = if reader.has_realized_pnl() {
            let realized_pnl_reader = reader.get_realized_pnl()?;
            Some(Money::from_capnp(realized_pnl_reader)?)
        } else {
            None
        };

        let unrealized_pnl_reader = reader.get_unrealized_pnl()?;
        let unrealized_pnl = Money::from_capnp(unrealized_pnl_reader)?;

        let duration = reader.get_duration();

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_opened_reader = reader.get_ts_opened()?;
        let ts_opened = ts_opened_reader.get_value();

        let ts_closed = if reader.has_ts_closed() {
            let ts_closed_reader = reader.get_ts_closed()?;
            Some(ts_closed_reader.get_value().into())
        } else {
            None
        };

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            closing_order_id,
            entry,
            side,
            signed_qty,
            quantity,
            peak_quantity,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            avg_px_close,
            realized_return,
            realized_pnl,
            unrealized_pnl,
            duration,
            event_id,
            ts_opened: ts_opened.into(),
            ts_closed,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

// PositionAdjusted
impl<'a> ToCapnp<'a> for PositionAdjusted {
    type Builder = position_capnp::position_adjusted::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let strategy_id_builder = builder.reborrow().init_strategy_id();
        self.strategy_id.to_capnp(strategy_id_builder);

        let instrument_id_builder = builder.reborrow().init_instrument_id();
        self.instrument_id.to_capnp(instrument_id_builder);

        let position_id_builder = builder.reborrow().init_position_id();
        self.position_id.to_capnp(position_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        builder.set_adjustment_type(position_adjustment_type_to_capnp(self.adjustment_type));

        if let Some(qty_change) = self.quantity_change {
            let (lo, mid, hi, flags) = decimal_to_parts(&qty_change);
            let mut qty_change_builder = builder.reborrow().init_quantity_change();
            qty_change_builder.set_lo(lo);
            qty_change_builder.set_mid(mid);
            qty_change_builder.set_hi(hi);
            qty_change_builder.set_flags(flags);
        }

        if let Some(ref pnl) = self.pnl_change {
            let pnl_change_builder = builder.reborrow().init_pnl_change();
            pnl.to_capnp(pnl_change_builder);
        }

        if let Some(reason) = self.reason {
            builder.set_reason(reason.as_str());
        }

        let event_id_builder = builder.reborrow().init_event_id();
        self.event_id.to_capnp(event_id_builder);

        let mut ts_event_builder = builder.reborrow().init_ts_event();
        ts_event_builder.set_value(*self.ts_event);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> FromCapnp<'a> for PositionAdjusted {
    type Reader = position_capnp::position_adjusted::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let trader_id_reader = reader.get_trader_id()?;
        let trader_id = TraderId::from_capnp(trader_id_reader)?;

        let strategy_id_reader = reader.get_strategy_id()?;
        let strategy_id = StrategyId::from_capnp(strategy_id_reader)?;

        let instrument_id_reader = reader.get_instrument_id()?;
        let instrument_id = InstrumentId::from_capnp(instrument_id_reader)?;

        let position_id_reader = reader.get_position_id()?;
        let position_id = PositionId::from_capnp(position_id_reader)?;

        let account_id_reader = reader.get_account_id()?;
        let account_id = AccountId::from_capnp(account_id_reader)?;

        let adjustment_type = position_adjustment_type_from_capnp(reader.get_adjustment_type()?);

        let quantity_change = if reader.has_quantity_change() {
            let qty_change_reader = reader.get_quantity_change()?;
            Some(decimal_from_parts(
                qty_change_reader.get_lo(),
                qty_change_reader.get_mid(),
                qty_change_reader.get_hi(),
                qty_change_reader.get_flags(),
            ))
        } else {
            None
        };

        let pnl_change = if reader.has_pnl_change() {
            let pnl_change_reader = reader.get_pnl_change()?;
            Some(Money::from_capnp(pnl_change_reader)?)
        } else {
            None
        };

        let reason = if reader.has_reason() {
            let reason_reader = reader.get_reason()?;
            let text = reason_reader.to_str()?;
            if text.is_empty() {
                None
            } else {
                Some(Ustr::from(text))
            }
        } else {
            None
        };

        let event_id_reader = reader.get_event_id()?;
        let event_id = nautilus_core::UUID4::from_capnp(event_id_reader)?;

        let ts_event_reader = reader.get_ts_event()?;
        let ts_event = ts_event_reader.get_value();

        let ts_init_reader = reader.get_ts_init()?;
        let ts_init = ts_init_reader.get_value();

        Ok(Self {
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            adjustment_type,
            quantity_change,
            pnl_change,
            reason,
            event_id,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use capnp::message::Builder;
    use nautilus_core::UnixNanos;
    use nautilus_model::{data::stubs::*, events::order::stubs::*};
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;

    macro_rules! assert_capnp_roundtrip {
        ($value:expr, $builder:path, $reader:path, $ty:ty) => {{
            let expected: $ty = $value;
            let mut message = Builder::new_default();
            {
                let builder = message.init_root::<$builder>();
                expected.to_capnp(builder);
            }
            let reader = message
                .get_root_as_reader::<$reader>()
                .expect("capnp reader");
            let decoded = <$ty>::from_capnp(reader).expect("capnp decode");
            assert_eq!(expected, decoded);
        }};
    }

    macro_rules! capnp_simple_roundtrip_test {
        ($name:ident, $value:expr, $builder:path, $reader:path, $ty:ty) => {
            #[rstest]
            fn $name() {
                assert_capnp_roundtrip!($value, $builder, $reader, $ty);
            }
        };
    }

    macro_rules! order_fixture_roundtrip_test {
        ($name:ident, $fixture:ident, $ty:ty, $builder:path, $reader:path) => {
            #[rstest]
            fn $name($fixture: $ty) {
                assert_capnp_roundtrip!($fixture, $builder, $reader, $ty);
            }
        };
    }

    #[rstest]
    fn test_instrument_id_roundtrip() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let bytes = serialize_instrument_id(&instrument_id).unwrap();
        let decoded = deserialize_instrument_id(&bytes).unwrap();
        assert_eq!(instrument_id, decoded);
    }

    #[rstest]
    fn test_price_roundtrip() {
        let price = Price::from("123.45");
        let bytes = serialize_price(&price).unwrap();
        let decoded = deserialize_price(&bytes).unwrap();
        assert_eq!(price, decoded);
    }

    #[rstest]
    fn test_quantity_roundtrip() {
        let qty = Quantity::from("100.5");
        let bytes = serialize_quantity(&qty).unwrap();
        let decoded = deserialize_quantity(&bytes).unwrap();
        assert_eq!(qty, decoded);
    }

    #[rstest]
    fn test_currency_roundtrip() {
        let currency = Currency::USD();
        let bytes = serialize_currency(&currency).unwrap();
        let decoded = deserialize_currency(&bytes).unwrap();
        assert_eq!(currency, decoded);
    }

    #[rstest]
    fn test_currency_crypto_roundtrip() {
        let currency = Currency::BTC();
        let bytes = serialize_currency(&currency).unwrap();
        let decoded = deserialize_currency(&bytes).unwrap();
        assert_eq!(currency, decoded);
    }

    #[rstest]
    fn test_money_roundtrip() {
        let money = Money::from_raw(100_000_000, Currency::USD());
        let bytes = serialize_money(&money).unwrap();
        let decoded = deserialize_money(&bytes).unwrap();
        assert_eq!(money, decoded);
    }

    #[rstest]
    fn test_money_negative() {
        let money = Money::from_raw(-50_000_000, Currency::USD());
        let bytes = serialize_money(&money).unwrap();
        let decoded = deserialize_money(&bytes).unwrap();
        assert_eq!(money, decoded);
    }

    #[rstest]
    fn test_money_zero() {
        let money = Money::from_raw(0, Currency::USD());
        let bytes = serialize_money(&money).unwrap();
        let decoded = deserialize_money(&bytes).unwrap();
        assert_eq!(money, decoded);
    }

    #[rstest]
    fn test_decimal_serialization_layout() {
        let decimal = Decimal::from_parts(
            0x89ab_cdef,
            0x0123_4567,
            0x0fed_cba9,
            true, // negative to ensure sign bit set
            6,    // add some scale metadata
        );
        let mut message = capnp::message::Builder::new_default();
        {
            let builder = message.init_root::<types_capnp::decimal::Builder>();
            decimal.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<types_capnp::decimal::Reader>()
            .expect("reader");

        let serialized = decimal.serialize();
        let expected_flags = u32::from_le_bytes(serialized[0..4].try_into().expect("flags slice"));

        assert_eq!(reader.get_flags(), expected_flags);
        assert_eq!(reader.get_lo(), 0x89ab_cdef);
        assert_eq!(reader.get_mid(), 0x0123_4567);
        assert_eq!(reader.get_hi(), 0x0fed_cba9);
    }

    #[rstest]
    fn test_decimal_roundtrip_preserves_scale_and_sign() {
        let decimal = Decimal::from_parts(0xffff_ffff, 0x7fff_ffff, 0x0000_00ff, false, 9);

        let mut message = capnp::message::Builder::new_default();
        {
            let builder = message.init_root::<types_capnp::decimal::Builder>();
            decimal.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<types_capnp::decimal::Reader>()
            .expect("reader");
        let decoded = Decimal::from_capnp(reader).expect("decoded decimal");
        assert_eq!(decimal, decoded);
    }

    #[rstest]
    fn test_account_balance_roundtrip() {
        let total = Money::from_raw(1000_00, Currency::USD());
        let locked = Money::from_raw(100_00, Currency::USD());
        let free = Money::from_raw(900_00, Currency::USD());
        let balance = AccountBalance::new(total, locked, free);
        let bytes = serialize_account_balance(&balance).unwrap();
        let decoded = deserialize_account_balance(&bytes).unwrap();
        assert_eq!(balance, decoded);
    }

    #[rstest]
    fn test_margin_balance_roundtrip() {
        let initial = Money::from_raw(5000_00, Currency::USD());
        let maintenance = Money::from_raw(2500_00, Currency::USD());
        let instrument_id = InstrumentId::from("BTC-USD-PERP.BINANCE");
        let balance = MarginBalance::new(initial, maintenance, instrument_id);
        let bytes = serialize_margin_balance(&balance).unwrap();
        let decoded = deserialize_margin_balance(&bytes).unwrap();
        assert_eq!(balance, decoded);
    }

    // Identifier round-trip coverage
    capnp_simple_roundtrip_test!(
        trader_id_capnp_roundtrip,
        TraderId::from("TRADER-CAPNP"),
        identifiers_capnp::trader_id::Builder,
        identifiers_capnp::trader_id::Reader,
        TraderId
    );
    capnp_simple_roundtrip_test!(
        strategy_id_capnp_roundtrip,
        StrategyId::from("STRATEGY-CAPNP"),
        identifiers_capnp::strategy_id::Builder,
        identifiers_capnp::strategy_id::Reader,
        StrategyId
    );
    capnp_simple_roundtrip_test!(
        actor_id_capnp_roundtrip,
        ActorId::from("ACTOR-CAPNP"),
        identifiers_capnp::actor_id::Builder,
        identifiers_capnp::actor_id::Reader,
        ActorId
    );
    capnp_simple_roundtrip_test!(
        account_id_capnp_roundtrip,
        AccountId::from("ACCOUNT-CAPNP"),
        identifiers_capnp::account_id::Builder,
        identifiers_capnp::account_id::Reader,
        AccountId
    );
    capnp_simple_roundtrip_test!(
        client_id_capnp_roundtrip,
        ClientId::from("CLIENT-CAPNP"),
        identifiers_capnp::client_id::Builder,
        identifiers_capnp::client_id::Reader,
        ClientId
    );
    capnp_simple_roundtrip_test!(
        client_order_id_capnp_roundtrip,
        ClientOrderId::from("O-20240101-000000-001-001-1"),
        identifiers_capnp::client_order_id::Builder,
        identifiers_capnp::client_order_id::Reader,
        ClientOrderId
    );
    capnp_simple_roundtrip_test!(
        venue_order_id_capnp_roundtrip,
        VenueOrderId::from("V-ORDER-1"),
        identifiers_capnp::venue_order_id::Builder,
        identifiers_capnp::venue_order_id::Reader,
        VenueOrderId
    );
    capnp_simple_roundtrip_test!(
        trade_id_capnp_roundtrip,
        TradeId::from("TRADE-12345"),
        identifiers_capnp::trade_id::Builder,
        identifiers_capnp::trade_id::Reader,
        TradeId
    );
    capnp_simple_roundtrip_test!(
        position_id_capnp_roundtrip,
        PositionId::from("POSITION-1"),
        identifiers_capnp::position_id::Builder,
        identifiers_capnp::position_id::Reader,
        PositionId
    );
    capnp_simple_roundtrip_test!(
        exec_algorithm_id_capnp_roundtrip,
        ExecAlgorithmId::from("EXEC-1"),
        identifiers_capnp::exec_algorithm_id::Builder,
        identifiers_capnp::exec_algorithm_id::Reader,
        ExecAlgorithmId
    );
    capnp_simple_roundtrip_test!(
        component_id_capnp_roundtrip,
        ComponentId::from("COMPONENT-RISK"),
        identifiers_capnp::component_id::Builder,
        identifiers_capnp::component_id::Reader,
        ComponentId
    );
    capnp_simple_roundtrip_test!(
        order_list_id_capnp_roundtrip,
        OrderListId::from("LIST-1"),
        identifiers_capnp::order_list_id::Builder,
        identifiers_capnp::order_list_id::Reader,
        OrderListId
    );
    capnp_simple_roundtrip_test!(
        symbol_capnp_roundtrip,
        Symbol::from("ETH-PERP"),
        identifiers_capnp::symbol::Builder,
        identifiers_capnp::symbol::Reader,
        Symbol
    );
    capnp_simple_roundtrip_test!(
        venue_capnp_roundtrip,
        Venue::from("BINANCE"),
        identifiers_capnp::venue::Builder,
        identifiers_capnp::venue::Reader,
        Venue
    );
    capnp_simple_roundtrip_test!(
        uuid4_capnp_roundtrip,
        uuid4(),
        base_capnp::u_u_i_d4::Builder,
        base_capnp::u_u_i_d4::Reader,
        nautilus_core::UUID4
    );

    // Market data structures
    capnp_simple_roundtrip_test!(
        quote_tick_capnp_roundtrip,
        quote_ethusdt_binance(),
        market_capnp::quote_tick::Builder,
        market_capnp::quote_tick::Reader,
        QuoteTick
    );
    capnp_simple_roundtrip_test!(
        trade_tick_capnp_roundtrip,
        stub_trade_ethusdt_buyer(),
        market_capnp::trade_tick::Builder,
        market_capnp::trade_tick::Reader,
        TradeTick
    );
    capnp_simple_roundtrip_test!(
        bar_specification_capnp_roundtrip,
        sample_bar_specification(),
        market_capnp::bar_spec::Builder,
        market_capnp::bar_spec::Reader,
        BarSpecification
    );
    capnp_simple_roundtrip_test!(
        bar_type_standard_capnp_roundtrip,
        sample_bar_type_standard(),
        market_capnp::bar_type::Builder,
        market_capnp::bar_type::Reader,
        BarType
    );
    capnp_simple_roundtrip_test!(
        bar_capnp_roundtrip,
        stub_bar(),
        market_capnp::bar::Builder,
        market_capnp::bar::Reader,
        Bar
    );
    capnp_simple_roundtrip_test!(
        book_order_capnp_roundtrip,
        stub_book_order(),
        market_capnp::book_order::Builder,
        market_capnp::book_order::Reader,
        BookOrder
    );
    capnp_simple_roundtrip_test!(
        order_book_delta_capnp_roundtrip,
        stub_delta(),
        market_capnp::order_book_delta::Builder,
        market_capnp::order_book_delta::Reader,
        OrderBookDelta
    );
    capnp_simple_roundtrip_test!(
        order_book_deltas_capnp_roundtrip,
        stub_deltas(),
        market_capnp::order_book_deltas::Builder,
        market_capnp::order_book_deltas::Reader,
        OrderBookDeltas
    );
    capnp_simple_roundtrip_test!(
        order_book_depth10_capnp_roundtrip,
        sample_order_book_depth10(),
        market_capnp::order_book_depth10::Builder,
        market_capnp::order_book_depth10::Reader,
        OrderBookDepth10
    );
    capnp_simple_roundtrip_test!(
        mark_price_update_capnp_roundtrip,
        sample_mark_price_update(),
        market_capnp::mark_price_update::Builder,
        market_capnp::mark_price_update::Reader,
        MarkPriceUpdate
    );
    capnp_simple_roundtrip_test!(
        index_price_update_capnp_roundtrip,
        sample_index_price_update(),
        market_capnp::index_price_update::Builder,
        market_capnp::index_price_update::Reader,
        IndexPriceUpdate
    );
    capnp_simple_roundtrip_test!(
        funding_rate_update_capnp_roundtrip,
        sample_funding_rate_update(),
        market_capnp::funding_rate_update::Builder,
        market_capnp::funding_rate_update::Reader,
        FundingRateUpdate
    );
    capnp_simple_roundtrip_test!(
        instrument_close_capnp_roundtrip,
        stub_instrument_close(),
        market_capnp::instrument_close::Builder,
        market_capnp::instrument_close::Reader,
        InstrumentClose
    );
    capnp_simple_roundtrip_test!(
        instrument_status_capnp_roundtrip,
        sample_instrument_status_event(),
        market_capnp::instrument_status::Builder,
        market_capnp::instrument_status::Reader,
        InstrumentStatus
    );

    // Order-event coverage through fixtures
    order_fixture_roundtrip_test!(
        order_filled_capnp_roundtrip,
        order_filled,
        OrderFilled,
        order_capnp::order_filled::Builder,
        order_capnp::order_filled::Reader
    );
    order_fixture_roundtrip_test!(
        order_denied_capnp_roundtrip,
        order_denied_max_submitted_rate,
        OrderDenied,
        order_capnp::order_denied::Builder,
        order_capnp::order_denied::Reader
    );
    order_fixture_roundtrip_test!(
        order_rejected_capnp_roundtrip,
        order_rejected_insufficient_margin,
        OrderRejected,
        order_capnp::order_rejected::Builder,
        order_capnp::order_rejected::Reader
    );
    order_fixture_roundtrip_test!(
        order_initialized_capnp_roundtrip,
        order_initialized_buy_limit,
        OrderInitialized,
        order_capnp::order_initialized::Builder,
        order_capnp::order_initialized::Reader
    );
    order_fixture_roundtrip_test!(
        order_submitted_capnp_roundtrip,
        order_submitted,
        OrderSubmitted,
        order_capnp::order_submitted::Builder,
        order_capnp::order_submitted::Reader
    );
    order_fixture_roundtrip_test!(
        order_triggered_capnp_roundtrip,
        order_triggered,
        OrderTriggered,
        order_capnp::order_triggered::Builder,
        order_capnp::order_triggered::Reader
    );
    order_fixture_roundtrip_test!(
        order_emulated_capnp_roundtrip,
        order_emulated,
        OrderEmulated,
        order_capnp::order_emulated::Builder,
        order_capnp::order_emulated::Reader
    );
    order_fixture_roundtrip_test!(
        order_released_capnp_roundtrip,
        order_released,
        OrderReleased,
        order_capnp::order_released::Builder,
        order_capnp::order_released::Reader
    );
    order_fixture_roundtrip_test!(
        order_updated_capnp_roundtrip,
        order_updated,
        OrderUpdated,
        order_capnp::order_updated::Builder,
        order_capnp::order_updated::Reader
    );
    order_fixture_roundtrip_test!(
        order_pending_update_capnp_roundtrip,
        order_pending_update,
        OrderPendingUpdate,
        order_capnp::order_pending_update::Builder,
        order_capnp::order_pending_update::Reader
    );
    order_fixture_roundtrip_test!(
        order_pending_cancel_capnp_roundtrip,
        order_pending_cancel,
        OrderPendingCancel,
        order_capnp::order_pending_cancel::Builder,
        order_capnp::order_pending_cancel::Reader
    );
    order_fixture_roundtrip_test!(
        order_modify_rejected_capnp_roundtrip,
        order_modify_rejected,
        OrderModifyRejected,
        order_capnp::order_modify_rejected::Builder,
        order_capnp::order_modify_rejected::Reader
    );
    order_fixture_roundtrip_test!(
        order_accepted_capnp_roundtrip,
        order_accepted,
        OrderAccepted,
        order_capnp::order_accepted::Builder,
        order_capnp::order_accepted::Reader
    );
    order_fixture_roundtrip_test!(
        order_cancel_rejected_capnp_roundtrip,
        order_cancel_rejected,
        OrderCancelRejected,
        order_capnp::order_cancel_rejected::Builder,
        order_capnp::order_cancel_rejected::Reader
    );
    order_fixture_roundtrip_test!(
        order_expired_capnp_roundtrip,
        order_expired,
        OrderExpired,
        order_capnp::order_expired::Builder,
        order_capnp::order_expired::Reader
    );
    #[rstest]
    fn order_canceled_capnp_roundtrip() {
        assert_capnp_roundtrip!(
            sample_order_canceled(),
            order_capnp::order_canceled::Builder,
            order_capnp::order_canceled::Reader,
            OrderCanceled
        );
    }

    // Position event coverage
    #[rstest]
    fn position_opened_capnp_roundtrip() {
        assert_capnp_roundtrip!(
            sample_position_opened(),
            position_capnp::position_opened::Builder,
            position_capnp::position_opened::Reader,
            PositionOpened
        );
    }

    #[rstest]
    fn position_changed_capnp_roundtrip() {
        assert_capnp_roundtrip!(
            sample_position_changed(),
            position_capnp::position_changed::Builder,
            position_capnp::position_changed::Reader,
            PositionChanged
        );
    }

    #[rstest]
    fn position_closed_capnp_roundtrip() {
        assert_capnp_roundtrip!(
            sample_position_closed(),
            position_capnp::position_closed::Builder,
            position_capnp::position_closed::Reader,
            PositionClosed
        );
    }

    #[rstest]
    fn position_adjusted_capnp_roundtrip() {
        assert_capnp_roundtrip!(
            sample_position_adjusted(),
            position_capnp::position_adjusted::Builder,
            position_capnp::position_adjusted::Reader,
            PositionAdjusted
        );
    }

    fn sample_bar_specification() -> BarSpecification {
        BarSpecification::new(5, BarAggregation::Minute, PriceType::Last)
    }

    fn sample_bar_type_standard() -> BarType {
        BarType::new(
            InstrumentId::from("AUDUSD.SIM"),
            sample_bar_specification(),
            AggregationSource::External,
        )
    }

    fn sample_mark_price_update() -> MarkPriceUpdate {
        MarkPriceUpdate::new(
            InstrumentId::from("BTCUSD-PERP.BINANCE"),
            Price::from("42000.123"),
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
    }

    fn sample_index_price_update() -> IndexPriceUpdate {
        IndexPriceUpdate::new(
            InstrumentId::from("BTCUSD-PERP.BINANCE"),
            Price::from("41950.500"),
            UnixNanos::from(3),
            UnixNanos::from(4),
        )
    }

    fn sample_funding_rate_update() -> FundingRateUpdate {
        FundingRateUpdate::new(
            InstrumentId::from("BTCUSD-PERP.BINANCE"),
            dec!(0.0001),
            Some(UnixNanos::from(1_000_000)),
            UnixNanos::from(5),
            UnixNanos::from(6),
        )
    }

    fn sample_instrument_status_event() -> InstrumentStatus {
        InstrumentStatus::new(
            InstrumentId::from("MSFT.XNAS"),
            MarketStatusAction::Trading,
            UnixNanos::from(1),
            UnixNanos::from(2),
            Some(Ustr::from("Normal trading")),
            Some(Ustr::from("MARKET_OPEN")),
            Some(true),
            Some(true),
            Some(false),
        )
    }

    fn sample_order_canceled() -> OrderCanceled {
        OrderCanceled::new(
            trader_id(),
            strategy_id_ema_cross(),
            instrument_id_btc_usdt(),
            client_order_id(),
            uuid4(),
            UnixNanos::from(7),
            UnixNanos::from(8),
            true,
            Some(venue_order_id()),
            Some(account_id()),
        )
    }

    fn sample_order_book_depth10() -> OrderBookDepth10 {
        const LEVELS: usize = 10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut bids = [BookOrder::default(); LEVELS];
        let mut asks = [BookOrder::default(); LEVELS];
        for i in 0..LEVELS {
            bids[i] = BookOrder::new(
                OrderSide::Buy,
                Price::new(100.0 - i as f64, 2),
                Quantity::new(1.0 + i as f64, 2),
                0,
            );
            asks[i] = BookOrder::new(
                OrderSide::Sell,
                Price::new(101.0 + i as f64, 2),
                Quantity::new(1.0 + i as f64, 2),
                0,
            );
        }
        let bid_counts = [1_u32; LEVELS];
        let ask_counts = [1_u32; LEVELS];
        OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            0,
            1,
            UnixNanos::from(9),
            UnixNanos::from(10),
        )
    }

    fn sample_position_opened() -> PositionOpened {
        PositionOpened {
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_btc_usdt(),
            position_id: PositionId::from("P-OPEN"),
            account_id: account_id(),
            opening_order_id: client_order_id(),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 100.0,
            quantity: Quantity::from("100"),
            last_qty: Quantity::from("100"),
            last_px: Price::from("20000"),
            currency: Currency::USD(),
            avg_px_open: 20000.0,
            event_id: uuid4(),
            ts_event: UnixNanos::from(9),
            ts_init: UnixNanos::from(10),
        }
    }

    fn sample_position_changed() -> PositionChanged {
        PositionChanged {
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_btc_usdt(),
            position_id: PositionId::from("P-CHANGED"),
            account_id: account_id(),
            opening_order_id: client_order_id(),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 150.0,
            quantity: Quantity::from("150"),
            peak_quantity: Quantity::from("175"),
            last_qty: Quantity::from("50"),
            last_px: Price::from("20100"),
            currency: Currency::USD(),
            avg_px_open: 19950.0,
            avg_px_close: Some(20100.0),
            realized_return: 0.01,
            realized_pnl: Some(Money::new(150.0, Currency::USD())),
            unrealized_pnl: Money::new(75.0, Currency::USD()),
            event_id: uuid4(),
            ts_opened: UnixNanos::from(11),
            ts_event: UnixNanos::from(12),
            ts_init: UnixNanos::from(13),
        }
    }

    fn sample_position_closed() -> PositionClosed {
        PositionClosed {
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_btc_usdt(),
            position_id: PositionId::from("P-CLOSED"),
            account_id: account_id(),
            opening_order_id: client_order_id(),
            closing_order_id: Some(ClientOrderId::from("O-19700101-000000-001-001-9")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("200"),
            last_qty: Quantity::from("200"),
            last_px: Price::from("20500"),
            currency: Currency::USD(),
            avg_px_open: 20000.0,
            avg_px_close: Some(20500.0),
            realized_return: 0.025,
            realized_pnl: Some(Money::new(1000.0, Currency::USD())),
            unrealized_pnl: Money::new(0.0, Currency::USD()),
            duration: 1_000_000,
            event_id: uuid4(),
            ts_opened: UnixNanos::from(14),
            ts_closed: Some(UnixNanos::from(15)),
            ts_event: UnixNanos::from(15),
            ts_init: UnixNanos::from(16),
        }
    }

    fn sample_position_adjusted() -> PositionAdjusted {
        PositionAdjusted::new(
            trader_id(),
            strategy_id_ema_cross(),
            instrument_id_btc_usdt(),
            PositionId::from("P-ADJUST"),
            account_id(),
            PositionAdjustmentType::Funding,
            Some(dec!(-0.001)),
            Some(Money::new(-5.5, Currency::USD())),
            Some(Ustr::from("funding_2024-01-15")),
            uuid4(),
            UnixNanos::from(17),
            UnixNanos::from(18),
        )
    }
}
