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

//! Parsing utilities for Rithmic market data messages.

use crate::common::parse::parse_timestamp_nanos;
use crate::error::{Result, RithmicError};

use super::client::{QuoteTick, TradeTick};

/// Parses a quote tick from Rithmic BBO data.
///
/// This function converts raw Rithmic protocol buffer data into
/// a normalized QuoteTick struct.
#[allow(dead_code)] // Scaffolding for future implementation
pub fn parse_quote_tick(
    symbol: &str,
    exchange: &str,
    bid_price: Option<f64>,
    bid_size: Option<f64>,
    ask_price: Option<f64>,
    ask_size: Option<f64>,
    timestamp_secs: f64,
) -> Result<QuoteTick> {
    // Validate we have at least bid or ask
    if bid_price.is_none() && ask_price.is_none() {
        return Err(RithmicError::Parse(
            "Quote must have at least bid or ask price".to_string(),
        ));
    }

    let ts_nanos = parse_timestamp_nanos(timestamp_secs);

    Ok(QuoteTick {
        symbol: symbol.to_string(),
        exchange: exchange.to_string(),
        bid_price: bid_price.unwrap_or(0.0),
        ask_price: ask_price.unwrap_or(0.0),
        bid_size: bid_size.unwrap_or(0.0),
        ask_size: ask_size.unwrap_or(0.0),
        ts_event: ts_nanos,
        ts_init: ts_nanos,
    })
}

/// Parses a trade tick from Rithmic last trade data.
#[allow(dead_code)] // Scaffolding for future implementation
pub fn parse_trade_tick(
    symbol: &str,
    exchange: &str,
    price: f64,
    size: f64,
    aggressor_side: Option<&str>,
    trade_id: Option<&str>,
    timestamp_secs: f64,
) -> Result<TradeTick> {
    if price <= 0.0 {
        return Err(RithmicError::Parse(format!("Invalid trade price: {price}")));
    }

    if size <= 0.0 {
        return Err(RithmicError::Parse(format!("Invalid trade size: {size}")));
    }

    let ts_nanos = parse_timestamp_nanos(timestamp_secs);

    Ok(TradeTick {
        symbol: symbol.to_string(),
        exchange: exchange.to_string(),
        price,
        size,
        aggressor_side: aggressor_side.unwrap_or("UNKNOWN").to_string(),
        trade_id: trade_id.unwrap_or("").to_string(),
        ts_event: ts_nanos,
        ts_init: ts_nanos,
    })
}

/// Determines aggressor side from Rithmic trade data.
///
/// Returns "BUY" if trade was at ask, "SELL" if at bid, "UNKNOWN" otherwise.
#[allow(dead_code)] // Scaffolding for future implementation
pub fn determine_aggressor_side(
    trade_price: f64,
    bid_price: Option<f64>,
    ask_price: Option<f64>,
) -> &'static str {
    match (bid_price, ask_price) {
        (Some(bid), _) if (trade_price - bid).abs() < f64::EPSILON => "SELL",
        (_, Some(ask)) if (trade_price - ask).abs() < f64::EPSILON => "BUY",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_parse_quote_tick() {
        let quote = parse_quote_tick(
            "ESZ4",
            "CME",
            Some(4500.25),
            Some(10.0),
            Some(4500.50),
            Some(15.0),
            1234567890.0,
        )
        .unwrap();

        assert_eq!(quote.symbol, "ESZ4");
        assert_eq!(quote.bid_price, 4500.25);
        assert_eq!(quote.ask_price, 4500.50);
    }

    #[rstest::rstest]
    fn test_parse_trade_tick() {
        let trade = parse_trade_tick(
            "ESZ4",
            "CME",
            4500.25,
            5.0,
            Some("BUY"),
            Some("12345"),
            1234567890.0,
        )
        .unwrap();

        assert_eq!(trade.symbol, "ESZ4");
        assert_eq!(trade.price, 4500.25);
    }

    #[rstest::rstest]
    fn test_determine_aggressor_side() {
        assert_eq!(
            determine_aggressor_side(100.0, Some(100.0), Some(101.0)),
            "SELL"
        );
        assert_eq!(
            determine_aggressor_side(101.0, Some(100.0), Some(101.0)),
            "BUY"
        );
        assert_eq!(
            determine_aggressor_side(100.5, Some(100.0), Some(101.0)),
            "UNKNOWN"
        );
    }
}
