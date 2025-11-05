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

//! Type parsers for converting Hyperliquid types to Nautilus types.

use super::{
    consts::{HYPERLIQUID, HYPERLIQUID_DEFAULT_PRICE_PRECISION},
    enums::{HyperliquidOrderSide, HyperliquidOrderStatus, HyperliquidTimeInForce},
    types::HyperliquidAsset,
};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, TimeInForce},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};

/// Parses a Hyperliquid asset into a Nautilus CryptoPerpetual instrument
pub fn parse_instrument(asset: &HyperliquidAsset) -> anyhow::Result<InstrumentAny> {
    let venue = Venue::from(HYPERLIQUID);
    let symbol = Symbol::from(asset.name.as_str());
    let instrument_id = InstrumentId::new(symbol, venue);

    let base_currency = Currency::from(asset.name.as_str());
    let quote_currency = Currency::from("USD");

    let price_precision: u8 = HYPERLIQUID_DEFAULT_PRICE_PRECISION;
    let size_precision: u8 = asset.sz_decimals;

    // Calculate increments
    let price_increment = Price::from(format!("0.{:0>width$}1", "", width = (price_precision - 1) as usize).as_str());
    let size_increment = Quantity::from(format!("0.{:0>width$}1", "", width = (size_precision - 1) as usize).as_str());

    let instrument = CryptoPerpetual::new(
        instrument_id,
        symbol,
        base_currency,
        quote_currency,
        base_currency, // settlement currency
        false,         // is_inverse
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,          // multiplier
        None,          // lot_size
        None,          // max_quantity
        Some(size_increment), // min_quantity
        None,          // max_notional
        None,          // min_notional
        None,          // max_price
        None,          // min_price
        None,          // margin_init
        None,          // margin_maint
        None,          // maker_fee
        None,          // taker_fee
        0.into(),      // ts_event
        0.into(),      // ts_init
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Converts Hyperliquid order side to Nautilus OrderSide
pub fn parse_order_side(side: &HyperliquidOrderSide) -> OrderSide {
    match side {
        HyperliquidOrderSide::Buy => OrderSide::Buy,
        HyperliquidOrderSide::Sell => OrderSide::Sell,
    }
}

/// Converts string order side to Nautilus OrderSide
pub fn parse_order_side_str(side: &str) -> OrderSide {
    match side {
        "A" => OrderSide::Buy,
        "B" => OrderSide::Sell,
        _ => OrderSide::NoOrderSide,
    }
}

/// Converts Hyperliquid order status to Nautilus OrderStatus
pub fn parse_order_status(status: &HyperliquidOrderStatus) -> OrderStatus {
    match status {
        HyperliquidOrderStatus::Open => OrderStatus::Accepted,
        HyperliquidOrderStatus::Filled => OrderStatus::Filled,
        HyperliquidOrderStatus::Canceled => OrderStatus::Canceled,
        HyperliquidOrderStatus::Rejected => OrderStatus::Rejected,
        HyperliquidOrderStatus::Triggered => OrderStatus::Triggered,
    }
}

/// Converts Hyperliquid time in force to Nautilus TimeInForce
pub fn parse_time_in_force(tif: &HyperliquidTimeInForce) -> TimeInForce {
    match tif {
        HyperliquidTimeInForce::Gtc => TimeInForce::Gtc,
        HyperliquidTimeInForce::Ioc => TimeInForce::Ioc,
        HyperliquidTimeInForce::Alo => TimeInForce::Gtc, // ALO (add liquidity only) maps to GTC post-only
    }
}

/// Converts Nautilus OrderSide to Hyperliquid boolean (is_buy)
pub fn to_hyperliquid_is_buy(side: OrderSide) -> bool {
    match side {
        OrderSide::Buy => true,
        OrderSide::Sell => false,
        _ => false,
    }
}

/// Converts Nautilus TimeInForce to Hyperliquid TimeInForce
pub fn to_hyperliquid_time_in_force(tif: TimeInForce) -> HyperliquidTimeInForce {
    match tif {
        TimeInForce::Gtc => HyperliquidTimeInForce::Gtc,
        TimeInForce::Ioc => HyperliquidTimeInForce::Ioc,
        TimeInForce::Fok => HyperliquidTimeInForce::Ioc, // FOK not directly supported
        _ => HyperliquidTimeInForce::Gtc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_instrument() {
        let asset = HyperliquidAsset {
            name: "BTC".to_string(),
            sz_decimals: 5,
            max_leverage: Some(50),
            only_isolated: Some(false),
        };

        let instrument = parse_instrument(&asset).unwrap();
        assert!(matches!(instrument, InstrumentAny::CryptoPerpetual(_)));

        if let InstrumentAny::CryptoPerpetual(perp) = instrument {
            assert_eq!(perp.id.symbol.as_str(), "BTC");
            assert_eq!(perp.id.venue.as_str(), "HYPERLIQUID");
            assert_eq!(perp.size_precision, 5);
        }
    }

    #[test]
    fn test_parse_order_side() {
        assert_eq!(
            parse_order_side(&HyperliquidOrderSide::Buy),
            OrderSide::Buy
        );
        assert_eq!(
            parse_order_side(&HyperliquidOrderSide::Sell),
            OrderSide::Sell
        );
    }

    #[test]
    fn test_parse_order_side_str() {
        assert_eq!(parse_order_side_str("A"), OrderSide::Buy);
        assert_eq!(parse_order_side_str("B"), OrderSide::Sell);
        assert_eq!(parse_order_side_str("X"), OrderSide::NoOrderSide);
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(
            parse_order_status(&HyperliquidOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            parse_order_status(&HyperliquidOrderStatus::Filled),
            OrderStatus::Filled
        );
        assert_eq!(
            parse_order_status(&HyperliquidOrderStatus::Canceled),
            OrderStatus::Canceled
        );
    }

    #[test]
    fn test_to_hyperliquid_is_buy() {
        assert_eq!(to_hyperliquid_is_buy(OrderSide::Buy), true);
        assert_eq!(to_hyperliquid_is_buy(OrderSide::Sell), false);
    }
}
