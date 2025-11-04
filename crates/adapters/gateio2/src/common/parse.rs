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

//! Type conversions from Gate.io types to Nautilus types.

use anyhow::{anyhow, Result};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use std::str::FromStr;
use ustr::Ustr;

use crate::common::{
    consts::GATEIO,
    enums::{GateioOrderSide, GateioOrderStatus, GateioOrderType},
    models::{GateioFuturesContract, GateioSpotCurrencyPair},
};

/// Parses a Gate.io spot currency pair into a Nautilus `InstrumentAny`.
pub fn parse_spot_instrument(pair: &GateioSpotCurrencyPair) -> Result<InstrumentAny> {
    let venue = Venue::from(GATEIO);
    let symbol = Symbol::from(pair.id.as_str());
    let instrument_id = InstrumentId::new(symbol, venue);

    let base_currency = Currency::from(pair.base.as_str());
    let quote_currency = Currency::from(pair.quote.as_str());

    let price_precision = pair.precision;
    let size_precision = pair.amount_precision;
    let price_increment = Price::from(format!("0.{:0width$}1", 0, width = price_precision as usize - 1));
    let size_increment = Quantity::from(format!("0.{:0width$}1", 0, width = size_precision as usize - 1));

    let min_quantity = if !pair.min_base_amount.is_empty() {
        Quantity::from(pair.min_base_amount.as_str())
    } else {
        size_increment
    };

    let min_notional = if !pair.min_quote_amount.is_empty() {
        Some(Quantity::from(pair.min_quote_amount.as_str()))
    } else {
        None
    };

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
        None, // max_quantity
        Some(min_quantity),
        None, // max_notional
        None, // min_notional (use None for now, conversion issue)
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        0.into(),    // ts_event
        0.into(),    // ts_init
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Gate.io futures contract into a Nautilus `InstrumentAny`.
pub fn parse_futures_instrument(contract: &GateioFuturesContract) -> Result<InstrumentAny> {
    let venue = Venue::from(GATEIO);
    let symbol = Symbol::from(contract.name.as_str());
    let instrument_id = InstrumentId::new(symbol, venue);

    // Parse underlying and quote currency from contract name
    // Example: BTC_USDT -> underlying: BTC, quote: USDT
    let parts: Vec<&str> = contract.name.split('_').collect();
    let underlying = if parts.len() >= 2 {
        Currency::from(parts[0])
    } else {
        Currency::from("BTC")
    };
    let quote_currency = if parts.len() >= 2 {
        Currency::from(parts[1])
    } else {
        Currency::from("USDT")
    };
    let settlement_currency = quote_currency;

    // Parse precision from rounding strings
    let price_precision = parse_precision(&contract.order_price_round)?;
    let size_precision = 0; // Gate.io uses integer contract sizes

    let price_increment = Price::from(contract.order_price_round.as_str());
    let size_increment = Quantity::from("1"); // Contracts are in whole numbers

    let min_quantity = Quantity::from(contract.order_size_min.to_string().as_str());
    let max_quantity = Some(Quantity::from(contract.order_size_max.to_string().as_str()));

    let maker_fee = if !contract.maker_fee_rate.is_empty() {
        Some(Decimal::from_str(&contract.maker_fee_rate).unwrap_or(Decimal::ZERO))
    } else {
        None
    };

    let taker_fee = if !contract.taker_fee_rate.is_empty() {
        Some(Decimal::from_str(&contract.taker_fee_rate).unwrap_or(Decimal::ZERO))
    } else {
        None
    };

    let instrument = CryptoPerpetual::new(
        instrument_id,
        symbol,
        underlying,
        quote_currency,
        settlement_currency,
        false, // is_inverse (USDT-margined is not inverse)
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        max_quantity,
        Some(min_quantity),
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        maker_fee,
        taker_fee,
        0.into(), // ts_event
        0.into(), // ts_init
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses precision from a rounding string (e.g., "0.01" -> 2)
fn parse_precision(rounding: &str) -> Result<u8> {
    if rounding == "1" || rounding.is_empty() {
        return Ok(0);
    }

    let decimal = Decimal::from_str(rounding)
        .map_err(|e| anyhow!("Failed to parse rounding '{}': {}", rounding, e))?;

    // Count decimal places
    let s = decimal.to_string();
    if let Some(dot_pos) = s.find('.') {
        Ok((s.len() - dot_pos - 1) as u8)
    } else {
        Ok(0)
    }
}

/// Converts Gate.io order side to Nautilus `OrderSide`.
#[must_use]
pub fn parse_order_side(side: &GateioOrderSide) -> OrderSide {
    match side {
        GateioOrderSide::Buy => OrderSide::Buy,
        GateioOrderSide::Sell => OrderSide::Sell,
    }
}

/// Converts Gate.io order type to Nautilus `OrderType`.
#[must_use]
pub fn parse_order_type(order_type: &GateioOrderType) -> OrderType {
    match order_type {
        GateioOrderType::Limit => OrderType::Limit,
        GateioOrderType::Market => OrderType::Market,
    }
}

/// Converts Gate.io order status string to Nautilus `OrderStatus`.
pub fn parse_order_status(status: &str) -> Result<OrderStatus> {
    match status {
        "open" => Ok(OrderStatus::Accepted),
        "closed" => Ok(OrderStatus::Filled),
        "cancelled" => Ok(OrderStatus::Canceled),
        _ => Err(anyhow!("Unknown order status: {}", status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side(&GateioOrderSide::Buy), OrderSide::Buy);
        assert_eq!(parse_order_side(&GateioOrderSide::Sell), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_type() {
        assert_eq!(
            parse_order_type(&GateioOrderType::Limit),
            OrderType::Limit
        );
        assert_eq!(
            parse_order_type(&GateioOrderType::Market),
            OrderType::Market
        );
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(parse_order_status("open").unwrap(), OrderStatus::Accepted);
        assert_eq!(parse_order_status("closed").unwrap(), OrderStatus::Filled);
        assert_eq!(
            parse_order_status("cancelled").unwrap(),
            OrderStatus::Canceled
        );
        assert!(parse_order_status("unknown").is_err());
    }

    #[test]
    fn test_parse_precision() {
        assert_eq!(parse_precision("1").unwrap(), 0);
        assert_eq!(parse_precision("0.1").unwrap(), 1);
        assert_eq!(parse_precision("0.01").unwrap(), 2);
        assert_eq!(parse_precision("0.001").unwrap(), 3);
    }
}
