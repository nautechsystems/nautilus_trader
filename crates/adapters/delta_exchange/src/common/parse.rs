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

//! Parsing utilities for Delta Exchange data structures.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{Symbol, Venue},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use ustr::Ustr;

use super::enums::{
    DeltaExchangeOrderType, DeltaExchangeProductType, DeltaExchangeSide, DeltaExchangeTimeInForce,
};

/// Parse a string as a Decimal, handling empty strings as None.
pub fn parse_optional_decimal<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(ref s) if s.is_empty() => Ok(None),
        Some(s) => Decimal::from_str(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Parse a string as a Decimal, handling empty strings as zero.
pub fn parse_decimal_or_zero<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(ref s) if s.is_empty() => Ok(Decimal::ZERO),
        Some(s) => Decimal::from_str(&s).map_err(serde::de::Error::custom),
        None => Ok(Decimal::ZERO),
    }
}

/// Parse an empty string as None for optional fields.
pub fn parse_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// Parse an optional datetime from string, handling empty strings.
pub fn parse_optional_datetime<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(ref s) if s.is_empty() => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(&s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Convert Delta Exchange timestamp (microseconds) to UnixNanos.
pub fn parse_timestamp_us(timestamp_us: u64) -> UnixNanos {
    UnixNanos::from(timestamp_us * 1_000) // Convert microseconds to nanoseconds
}

/// Convert Delta Exchange timestamp (milliseconds) to UnixNanos.
pub fn parse_timestamp_ms(timestamp_ms: u64) -> UnixNanos {
    UnixNanos::from(timestamp_ms * 1_000_000) // Convert milliseconds to nanoseconds
}

/// Parse Delta Exchange symbol to Nautilus Symbol.
pub fn parse_symbol(symbol: &str) -> Result<Symbol, String> {
    if symbol.is_empty() {
        return Err("Symbol cannot be empty".to_string());
    }
    Ok(Symbol::new(symbol).map_err(|e| e.to_string())?)
}

/// Parse Delta Exchange venue.
pub fn parse_venue() -> Venue {
    Venue::new("DELTA_EXCHANGE").expect("Valid venue")
}

/// Convert Delta Exchange side to Nautilus OrderSide.
pub fn parse_order_side(side: DeltaExchangeSide) -> OrderSide {
    match side {
        DeltaExchangeSide::Buy => OrderSide::Buy,
        DeltaExchangeSide::Sell => OrderSide::Sell,
    }
}

/// Convert Delta Exchange order type to Nautilus OrderType.
pub fn parse_order_type(order_type: DeltaExchangeOrderType) -> OrderType {
    match order_type {
        DeltaExchangeOrderType::LimitOrder => OrderType::Limit,
        DeltaExchangeOrderType::MarketOrder => OrderType::Market,
        DeltaExchangeOrderType::StopLossOrder => OrderType::StopLimit,
        DeltaExchangeOrderType::TakeProfitOrder => OrderType::LimitIfTouched,
    }
}

/// Convert Delta Exchange time in force to Nautilus TimeInForce.
pub fn parse_time_in_force(tif: DeltaExchangeTimeInForce) -> TimeInForce {
    match tif {
        DeltaExchangeTimeInForce::Gtc => TimeInForce::Gtc,
        DeltaExchangeTimeInForce::Ioc => TimeInForce::Ioc,
    }
}

/// Convert Nautilus OrderSide to Delta Exchange side.
pub fn order_side_to_delta_exchange(side: OrderSide) -> DeltaExchangeSide {
    match side {
        OrderSide::Buy => DeltaExchangeSide::Buy,
        OrderSide::Sell => DeltaExchangeSide::Sell,
        OrderSide::NoOrderSide => DeltaExchangeSide::Buy, // Default fallback
    }
}

/// Convert Nautilus OrderType to Delta Exchange order type.
pub fn order_type_to_delta_exchange(order_type: OrderType) -> DeltaExchangeOrderType {
    match order_type {
        OrderType::Market => DeltaExchangeOrderType::MarketOrder,
        OrderType::Limit => DeltaExchangeOrderType::LimitOrder,
        OrderType::StopLimit => DeltaExchangeOrderType::StopLossOrder,
        OrderType::LimitIfTouched => DeltaExchangeOrderType::TakeProfitOrder,
        _ => DeltaExchangeOrderType::LimitOrder, // Default fallback
    }
}

/// Convert Nautilus TimeInForce to Delta Exchange time in force.
pub fn time_in_force_to_delta_exchange(tif: TimeInForce) -> DeltaExchangeTimeInForce {
    match tif {
        TimeInForce::Gtc => DeltaExchangeTimeInForce::Gtc,
        TimeInForce::Ioc => DeltaExchangeTimeInForce::Ioc,
        _ => DeltaExchangeTimeInForce::Gtc, // Default fallback
    }
}

/// Parse a price from string.
pub fn parse_price(price_str: &str, precision: u8) -> Result<Price, String> {
    if price_str.is_empty() {
        return Err("Price string cannot be empty".to_string());
    }
    
    let decimal = Decimal::from_str(price_str)
        .map_err(|e| format!("Failed to parse price '{}': {}", price_str, e))?;
    
    Price::new(decimal, precision).map_err(|e| e.to_string())
}

/// Parse a quantity from string.
pub fn parse_quantity(quantity_str: &str, precision: u8) -> Result<Quantity, String> {
    if quantity_str.is_empty() {
        return Err("Quantity string cannot be empty".to_string());
    }
    
    let decimal = Decimal::from_str(quantity_str)
        .map_err(|e| format!("Failed to parse quantity '{}': {}", quantity_str, e))?;
    
    Quantity::new(decimal, precision).map_err(|e| e.to_string())
}

/// Parse Delta Exchange product symbol to determine if it's an option.
pub fn is_option_symbol(symbol: &str) -> bool {
    symbol.starts_with("C-") || symbol.starts_with("P-")
}

/// Parse Delta Exchange option symbol to extract components.
pub fn parse_option_symbol(symbol: &str) -> Result<(String, String, Decimal, String), String> {
    // Format: C-BTC-90000-310125 or P-BTC-38100-230124
    let parts: Vec<&str> = symbol.split('-').collect();
    if parts.len() != 4 {
        return Err(format!("Invalid option symbol format: {}", symbol));
    }
    
    let option_type = parts[0].to_string(); // C or P
    let underlying = parts[1].to_string(); // BTC, ETH, etc.
    let strike = Decimal::from_str(parts[2])
        .map_err(|e| format!("Invalid strike price '{}': {}", parts[2], e))?;
    let expiry = parts[3].to_string(); // ddMMYY format
    
    Ok((option_type, underlying, strike, expiry))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_symbol() {
        let symbol = parse_symbol("BTCUSD").unwrap();
        assert_eq!(symbol.as_str(), "BTCUSD");
    }

    #[test]
    fn test_parse_symbol_empty() {
        let result = parse_symbol("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_venue() {
        let venue = parse_venue();
        assert_eq!(venue.as_str(), "DELTA_EXCHANGE");
    }

    #[test]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side(DeltaExchangeSide::Buy), OrderSide::Buy);
        assert_eq!(parse_order_side(DeltaExchangeSide::Sell), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_type() {
        assert_eq!(parse_order_type(DeltaExchangeOrderType::LimitOrder), OrderType::Limit);
        assert_eq!(parse_order_type(DeltaExchangeOrderType::MarketOrder), OrderType::Market);
    }

    #[test]
    fn test_parse_time_in_force() {
        assert_eq!(parse_time_in_force(DeltaExchangeTimeInForce::Gtc), TimeInForce::Gtc);
        assert_eq!(parse_time_in_force(DeltaExchangeTimeInForce::Ioc), TimeInForce::Ioc);
    }

    #[test]
    fn test_parse_price() {
        let price = parse_price("50000.50", 2).unwrap();
        assert_eq!(price.as_f64(), 50000.50);
    }

    #[test]
    fn test_parse_quantity() {
        let quantity = parse_quantity("1.5", 1).unwrap();
        assert_eq!(quantity.as_f64(), 1.5);
    }

    #[test]
    fn test_is_option_symbol() {
        assert!(is_option_symbol("C-BTC-90000-310125"));
        assert!(is_option_symbol("P-BTC-38100-230124"));
        assert!(!is_option_symbol("BTCUSD"));
        assert!(!is_option_symbol("ETHUSD"));
    }

    #[test]
    fn test_parse_option_symbol() {
        let (option_type, underlying, strike, expiry) = 
            parse_option_symbol("C-BTC-90000-310125").unwrap();
        
        assert_eq!(option_type, "C");
        assert_eq!(underlying, "BTC");
        assert_eq!(strike, Decimal::from(90000));
        assert_eq!(expiry, "310125");
    }

    #[test]
    fn test_parse_option_symbol_invalid() {
        let result = parse_option_symbol("INVALID");
        assert!(result.is_err());
    }

    #[test]
    fn test_timestamp_conversion() {
        let timestamp_us = 1641890400000000; // 2022-01-11T00:00:00Z in microseconds
        let unix_nanos = parse_timestamp_us(timestamp_us);
        assert_eq!(unix_nanos.as_u64(), 1641890400000000000); // nanoseconds
        
        let timestamp_ms = 1641890400000; // 2022-01-11T00:00:00Z in milliseconds
        let unix_nanos = parse_timestamp_ms(timestamp_ms);
        assert_eq!(unix_nanos.as_u64(), 1641890400000000000); // nanoseconds
    }
}
