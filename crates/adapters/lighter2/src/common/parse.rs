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

//! Parsing functions to convert Lighter data to Nautilus types.

use std::str::FromStr;

use anyhow::{Context, Result};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::{
        AggressorSide, AssetClass, CurrencyType, InstrumentClass, OptionKind, OrderSide,
        OrderStatus, OrderType, TimeInForce,
    },
    identifiers::{InstrumentId, Symbol, TradeId, Venue, VenueOrderId},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    consts::LIGHTER,
    enums::{LighterInstrumentType, LighterOrderSide, LighterOrderStatus, LighterOrderType},
    models::LighterMarket,
};

/// Parses a Lighter order side to Nautilus `OrderSide`.
pub fn parse_order_side(side: LighterOrderSide) -> OrderSide {
    match side {
        LighterOrderSide::Buy => OrderSide::Buy,
        LighterOrderSide::Sell => OrderSide::Sell,
    }
}

/// Parses a Nautilus `OrderSide` to Lighter order side.
pub fn to_lighter_order_side(side: OrderSide) -> LighterOrderSide {
    match side {
        OrderSide::Buy => LighterOrderSide::Buy,
        OrderSide::Sell => LighterOrderSide::Sell,
        OrderSide::NoOrderSide => LighterOrderSide::Buy, // Default to buy
    }
}

/// Parses a Lighter order type to Nautilus `OrderType`.
pub fn parse_order_type(order_type: LighterOrderType) -> OrderType {
    match order_type {
        LighterOrderType::Limit => OrderType::Limit,
        LighterOrderType::Market => OrderType::Market,
        LighterOrderType::StopLoss => OrderType::StopMarket,
        LighterOrderType::StopLossLimit => OrderType::StopLimit,
        LighterOrderType::TakeProfit => OrderType::MarketIfTouched,
        LighterOrderType::TakeProfitLimit => OrderType::LimitIfTouched,
        LighterOrderType::Twap => OrderType::Limit, // Map TWAP to Limit
    }
}

/// Parses a Lighter order status to Nautilus `OrderStatus`.
pub fn parse_order_status(status: LighterOrderStatus) -> OrderStatus {
    match status {
        LighterOrderStatus::Pending => OrderStatus::Submitted,
        LighterOrderStatus::Open => OrderStatus::Accepted,
        LighterOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        LighterOrderStatus::Filled => OrderStatus::Filled,
        LighterOrderStatus::Canceled => OrderStatus::Canceled,
        LighterOrderStatus::Rejected => OrderStatus::Rejected,
        LighterOrderStatus::Expired => OrderStatus::Expired,
    }
}

/// Parses a Lighter market to a Nautilus `InstrumentAny`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_instrument(market: &LighterMarket) -> Result<InstrumentAny> {
    let venue = Venue::from(LIGHTER);
    let symbol = Symbol::from(market.symbol.as_str());
    let instrument_id = InstrumentId::new(symbol, venue);

    let base_currency = Currency::from(market.base_currency.as_str());
    let quote_currency = Currency::from(market.quote_currency.as_str());

    let price_precision = market.price_precision;
    let size_precision = market.size_precision;

    let price_increment = Price::from(format!("0.{:0>width$}1", "", width = (price_precision as usize).saturating_sub(1)).as_str());
    let size_increment = Quantity::from(format!("0.{:0>width$}1", "", width = (size_precision as usize).saturating_sub(1)).as_str());

    let min_quantity = Some(Quantity::from_str(&market.min_order_size.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to parse min_order_size: {}", e))?);
    let max_quantity = Some(Quantity::from_str(&market.max_order_size.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to parse max_order_size: {}", e))?);

    let ts_init = UnixNanos::default();

    match market.instrument_type {
        LighterInstrumentType::Spot => {
            let instrument = CurrencyPair::new(
                instrument_id,
                symbol,
                base_currency,
                quote_currency,
                price_precision,
                size_precision,
                price_increment,
                size_increment,
                None, // multiplier
                None, // lot_size
                max_quantity,
                min_quantity,
                None, // max_notional
                None, // min_notional
                None, // max_price
                None, // min_price
                None, // margin_init
                None, // margin_maint
                None, // maker_fee
                None, // taker_fee
                ts_init,
                ts_init,
            );
            Ok(InstrumentAny::CurrencyPair(instrument))
        }
        LighterInstrumentType::Perp => {
            let instrument = CryptoPerpetual::new(
                instrument_id,
                symbol,
                base_currency,
                quote_currency,
                base_currency, // settlement_currency
                false,         // is_inverse
                price_precision,
                size_precision,
                price_increment,
                size_increment,
                None, // multiplier
                None, // lot_size
                max_quantity,
                min_quantity,
                None, // max_notional
                None, // min_notional
                None, // max_price
                None, // min_price
                None, // margin_init
                None, // margin_maint
                None, // maker_fee
                None, // taker_fee
                ts_init,
                ts_init,
            );
            Ok(InstrumentAny::CryptoPerpetual(instrument))
        }
    }
}

/// Parses a decimal string to `Price`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_price(value: &str) -> Result<Price> {
    Price::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse price: {}", e))
}

/// Parses a decimal string to `Quantity`.
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_quantity(value: &str) -> Result<Quantity> {
    Quantity::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse quantity: {}", e))
}

/// Parses a decimal to `Price`.
///
/// # Errors
///
/// Returns an error if conversion fails.
pub fn decimal_to_price(value: Decimal) -> Result<Price> {
    parse_price(&value.to_string())
}

/// Parses a decimal to `Quantity`.
///
/// # Errors
///
/// Returns an error if conversion fails.
pub fn decimal_to_quantity(value: Decimal) -> Result<Quantity> {
    parse_quantity(&value.to_string())
}

/// Creates a `VenueOrderId` from a string.
pub fn parse_venue_order_id(id: &str) -> VenueOrderId {
    VenueOrderId::new(id)
}

/// Creates a `TradeId` from a string.
pub fn parse_trade_id(id: &str) -> TradeId {
    TradeId::new(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side(LighterOrderSide::Buy), OrderSide::Buy);
        assert_eq!(parse_order_side(LighterOrderSide::Sell), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_type() {
        assert_eq!(parse_order_type(LighterOrderType::Limit), OrderType::Limit);
        assert_eq!(parse_order_type(LighterOrderType::Market), OrderType::Market);
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(
            parse_order_status(LighterOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            parse_order_status(LighterOrderStatus::Filled),
            OrderStatus::Filled
        );
    }
}
