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

//! Parsing utilities for converting dYdX v4 Indexer API responses into Nautilus domain models.
//!
//! This module contains functions that transform raw JSON data structures
//! from the dYdX Indexer API into strongly-typed Nautilus data types such as
//! instruments, trades, bars, account states, etc.
//!
//! # Design Principles
//!
//! - **Validation First**: All inputs are validated before parsing.
//! - **Contextual Errors**: All errors include context about what was being parsed.
//! - **Zero-Copy When Possible**: Uses references and borrows to minimize allocations.
//! - **Type Safety**: Leverages Rust's type system to prevent invalid states.
//!
//! # Error Handling
//!
//! All parsing functions return `anyhow::Result<T>` with descriptive error messages
//! that include context about the field being parsed and the value that failed.
//! This makes debugging API changes or data issues much easier.

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    events::AccountState,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::Currency,
};
use rust_decimal::Decimal;

use super::models::PerpetualMarket;
#[cfg(test)]
use crate::common::enums::DydxTransferType;
use crate::{
    common::{
        enums::{DydxMarketStatus, DydxOrderExecution, DydxOrderType, DydxTimeInForce},
        parse::{parse_decimal, parse_instrument_id, parse_price, parse_quantity},
    },
    websocket::messages::DydxSubaccountInfo,
};

/// Validates that a ticker has the correct format (BASE-QUOTE).
///
/// # Errors
///
/// Returns an error if the ticker is not in the format "BASE-QUOTE".
///
pub fn validate_ticker_format(ticker: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = ticker.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid ticker format '{ticker}', expected 'BASE-QUOTE' (e.g., 'BTC-USD')");
    }
    if parts[0].is_empty() || parts[1].is_empty() {
        anyhow::bail!("Invalid ticker format '{ticker}', base and quote cannot be empty");
    }
    Ok(())
}

/// Parses base and quote currency codes from a ticker.
///
/// # Errors
///
/// Returns an error if the ticker format is invalid.
///
pub fn parse_ticker_currencies(ticker: &str) -> anyhow::Result<(&str, &str)> {
    validate_ticker_format(ticker)?;
    let parts: Vec<&str> = ticker.split('-').collect();
    Ok((parts[0], parts[1]))
}

/// Returns true if the market status is Active.
#[must_use]
pub const fn is_market_active(status: &DydxMarketStatus) -> bool {
    matches!(status, DydxMarketStatus::Active)
}

/// Calculate time-in-force for conditional orders.
///
/// # Errors
///
/// Returns an error if the combination of parameters is invalid.
pub fn calculate_time_in_force(
    order_type: DydxOrderType,
    base_tif: DydxTimeInForce,
    post_only: bool,
    execution: Option<DydxOrderExecution>,
) -> anyhow::Result<TimeInForce> {
    match order_type {
        DydxOrderType::Market => Ok(TimeInForce::Ioc),
        DydxOrderType::Limit if post_only => Ok(TimeInForce::Gtc), // Post-only is GTC with post_only flag
        DydxOrderType::Limit => match base_tif {
            DydxTimeInForce::Gtt => Ok(TimeInForce::Gtc),
            DydxTimeInForce::Fok => Ok(TimeInForce::Fok),
            DydxTimeInForce::Ioc => Ok(TimeInForce::Ioc),
        },

        DydxOrderType::StopLimit | DydxOrderType::TakeProfitLimit => match execution {
            Some(DydxOrderExecution::PostOnly) => Ok(TimeInForce::Gtc), // Post-only is GTC with post_only flag
            Some(DydxOrderExecution::Fok) => Ok(TimeInForce::Fok),
            Some(DydxOrderExecution::Ioc) => Ok(TimeInForce::Ioc),
            Some(DydxOrderExecution::Default) | None => Ok(TimeInForce::Gtc), // Default for conditional limit
        },

        DydxOrderType::StopMarket | DydxOrderType::TakeProfitMarket => match execution {
            Some(DydxOrderExecution::Fok) => Ok(TimeInForce::Fok),
            Some(DydxOrderExecution::Ioc | DydxOrderExecution::Default) | None => {
                Ok(TimeInForce::Ioc)
            }
            Some(DydxOrderExecution::PostOnly) => {
                anyhow::bail!("Execution PostOnly not supported for {order_type:?}")
            }
        },

        DydxOrderType::TrailingStop => Ok(TimeInForce::Gtc),
    }
}

/// Validate conditional order parameters.
///
/// Ensures that trigger prices are set correctly relative to limit prices
/// based on order type and side.
///
/// # Errors
///
/// Returns an error if:
/// - Conditional order is missing trigger price.
/// - Trigger price is on wrong side of limit price for the order type.
pub fn validate_conditional_order(
    order_type: DydxOrderType,
    trigger_price: Option<Decimal>,
    price: Decimal,
    side: OrderSide,
) -> anyhow::Result<()> {
    if !order_type.is_conditional() {
        return Ok(());
    }

    let trigger_price = trigger_price
        .ok_or_else(|| anyhow::anyhow!("trigger_price required for {order_type:?}"))?;

    // Validate trigger price relative to limit price
    match order_type {
        DydxOrderType::StopLimit | DydxOrderType::StopMarket => {
            // Stop orders: trigger when price falls (sell) or rises (buy)
            match side {
                OrderSide::Buy if trigger_price < price => {
                    anyhow::bail!(
                        "Stop buy trigger_price ({trigger_price}) must be >= limit price ({price})"
                    );
                }
                OrderSide::Sell if trigger_price > price => {
                    anyhow::bail!(
                        "Stop sell trigger_price ({trigger_price}) must be <= limit price ({price})"
                    );
                }
                _ => {}
            }
        }
        DydxOrderType::TakeProfitLimit | DydxOrderType::TakeProfitMarket => {
            // Take profit: trigger when price rises (sell) or falls (buy)
            match side {
                OrderSide::Buy if trigger_price > price => {
                    anyhow::bail!(
                        "Take profit buy trigger_price ({trigger_price}) must be <= limit price ({price})"
                    );
                }
                OrderSide::Sell if trigger_price < price => {
                    anyhow::bail!(
                        "Take profit sell trigger_price ({trigger_price}) must be >= limit price ({price})"
                    );
                }
                _ => {}
            }
        }
        _ => {}
    }

    Ok(())
}

/// Parses a dYdX perpetual market into a Nautilus [`InstrumentAny`].
///
/// dYdX v4 only supports perpetual markets, so this function creates a
/// [`CryptoPerpetual`] instrument with the appropriate fields mapped from
/// the dYdX market definition.
///
/// # Errors
///
/// Returns an error if:
/// - Ticker format is invalid (not BASE-QUOTE).
/// - Required fields are missing or invalid.
/// - Price or quantity values cannot be parsed.
/// - Currency parsing fails.
/// - Margin fractions are out of valid range.
///
/// Note: Callers should pre-filter inactive markets using [`is_market_active`].
pub fn parse_instrument_any(
    definition: &PerpetualMarket,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Parse instrument ID with Nautilus perpetual suffix and keep raw symbol as venue ticker
    let instrument_id = parse_instrument_id(&definition.ticker);
    let raw_symbol = Symbol::from(definition.ticker.as_str());

    // Parse currencies from ticker using helper function
    let (base_str, quote_str) = parse_ticker_currencies(&definition.ticker)
        .context(format!("Failed to parse ticker '{}'", definition.ticker))?;

    let base_currency = Currency::get_or_create_crypto_with_context(base_str, None);
    let quote_currency = Currency::get_or_create_crypto_with_context(quote_str, None);
    let settlement_currency = quote_currency; // dYdX perpetuals settle in quote currency

    // Parse price and size increments with context
    let price_increment =
        parse_price(&definition.tick_size.to_string(), "tick_size").context(format!(
            "Failed to parse tick_size '{}' for market '{}'",
            definition.tick_size, definition.ticker
        ))?;

    let size_increment =
        parse_quantity(&definition.step_size.to_string(), "step_size").context(format!(
            "Failed to parse step_size '{}' for market '{}'",
            definition.step_size, definition.ticker
        ))?;

    // Parse min order size with context (use step_size as fallback if not provided)
    let min_quantity = Some(if let Some(min_size) = &definition.min_order_size {
        parse_quantity(&min_size.to_string(), "min_order_size").context(format!(
            "Failed to parse min_order_size '{}' for market '{}'",
            min_size, definition.ticker
        ))?
    } else {
        // Use step_size as minimum quantity if min_order_size not provided
        parse_quantity(&definition.step_size.to_string(), "step_size").context(format!(
            "Failed to parse step_size as min_quantity for market '{}'",
            definition.ticker
        ))?
    });

    // Parse margin fractions with validation
    let margin_init = Some(
        parse_decimal(
            &definition.initial_margin_fraction.to_string(),
            "initial_margin_fraction",
        )
        .context(format!(
            "Failed to parse initial_margin_fraction '{}' for market '{}'",
            definition.initial_margin_fraction, definition.ticker
        ))?,
    );

    let margin_maint = Some(
        parse_decimal(
            &definition.maintenance_margin_fraction.to_string(),
            "maintenance_margin_fraction",
        )
        .context(format!(
            "Failed to parse maintenance_margin_fraction '{}' for market '{}'",
            definition.maintenance_margin_fraction, definition.ticker
        ))?,
    );

    // Create the perpetual instrument
    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // dYdX perpetuals are not inverse
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,                 // multiplier: not applicable for dYdX
        Some(size_increment), // lot_size: same as size_increment
        None,                 // max_quantity: not specified by dYdX
        min_quantity,
        None, // max_notional: not specified by dYdX
        None, // min_notional: not specified by dYdX
        None, // max_price: not specified by dYdX
        None, // min_price: not specified by dYdX
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::Utc;
    use nautilus_model::{enums::OrderSide, instruments::Instrument};
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::{
            enums::{DydxOrderExecution, DydxOrderType, DydxTickerType, DydxTimeInForce},
            testing::load_json_result_fixture,
        },
        http::models::{
            CandlesResponse, FillsResponse, MarketsResponse, Order, OrderbookResponse,
            SubaccountResponse, TradesResponse, TransfersResponse,
        },
    };

    fn create_test_market() -> PerpetualMarket {
        PerpetualMarket {
            clob_pair_id: 1,
            ticker: "BTC-USD".to_string(),
            status: DydxMarketStatus::Active,
            base_asset: Some("BTC".to_string()),
            quote_asset: Some("USD".to_string()),
            step_size: Decimal::from_str("0.001").unwrap(),
            tick_size: Decimal::from_str("1").unwrap(),
            index_price: Some(Decimal::from_str("50000").unwrap()),
            oracle_price: Decimal::from_str("50000").unwrap(),
            price_change_24h: Decimal::ZERO,
            next_funding_rate: Decimal::ZERO,
            next_funding_at: Some(Utc::now()),
            min_order_size: Some(Decimal::from_str("0.001").unwrap()),
            market_type: Some(DydxTickerType::Perpetual),
            initial_margin_fraction: Decimal::from_str("0.05").unwrap(),
            maintenance_margin_fraction: Decimal::from_str("0.03").unwrap(),
            base_position_notional: Some(Decimal::from_str("10000").unwrap()),
            incremental_position_size: Some(Decimal::from_str("10000").unwrap()),
            incremental_initial_margin_fraction: Some(Decimal::from_str("0.01").unwrap()),
            max_position_size: Some(Decimal::from_str("100").unwrap()),
            open_interest: Decimal::from_str("1000000").unwrap(),
            atomic_resolution: -10,
            quantum_conversion_exponent: -10,
            subticks_per_tick: 100,
            step_base_quantums: 1000,
            is_reduce_only: false,
        }
    }

    #[rstest]
    fn test_parse_instrument_any_valid() {
        let market = create_test_market();
        let maker_fee = Some(Decimal::from_str("0.0002").unwrap());
        let taker_fee = Some(Decimal::from_str("0.0005").unwrap());
        let ts_init = UnixNanos::default();

        let result = parse_instrument_any(&market, maker_fee, taker_fee, ts_init);
        assert!(result.is_ok());

        let instrument = result.unwrap();
        if let InstrumentAny::CryptoPerpetual(perp) = instrument {
            assert_eq!(perp.id.symbol.as_str(), "BTC-USD-PERP");
            assert_eq!(perp.base_currency.code.as_str(), "BTC");
            assert_eq!(perp.quote_currency.code.as_str(), "USD");
            assert!(!perp.is_inverse);
            assert_eq!(perp.price_increment.to_string(), "1");
            assert_eq!(perp.size_increment.to_string(), "0.001");
        } else {
            panic!("Expected CryptoPerpetual instrument");
        }
    }

    #[rstest]
    fn test_is_market_active() {
        assert!(is_market_active(&DydxMarketStatus::Active));
        assert!(!is_market_active(&DydxMarketStatus::Paused));
        assert!(!is_market_active(&DydxMarketStatus::CancelOnly));
        assert!(!is_market_active(&DydxMarketStatus::PostOnly));
        assert!(!is_market_active(&DydxMarketStatus::Initializing));
        assert!(!is_market_active(&DydxMarketStatus::FinalSettlement));
    }

    #[rstest]
    fn test_parse_instrument_any_invalid_ticker() {
        let mut market = create_test_market();
        market.ticker = "INVALID".to_string();

        let result = parse_instrument_any(&market, None, None, UnixNanos::default());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        // The error message includes context, so check for key parts
        assert!(
            error_msg.contains("Invalid ticker format")
                || error_msg.contains("Failed to parse ticker"),
            "Expected ticker format error, was: {error_msg}"
        );
    }

    #[rstest]
    fn test_validate_ticker_format_valid() {
        assert!(validate_ticker_format("BTC-USD").is_ok());
        assert!(validate_ticker_format("ETH-USD").is_ok());
        assert!(validate_ticker_format("ATOM-USD").is_ok());
    }

    #[rstest]
    fn test_validate_ticker_format_invalid() {
        // Missing hyphen
        assert!(validate_ticker_format("BTCUSD").is_err());

        // Too many parts
        assert!(validate_ticker_format("BTC-USD-PERP").is_err());

        // Empty base
        assert!(validate_ticker_format("-USD").is_err());

        // Empty quote
        assert!(validate_ticker_format("BTC-").is_err());

        // Just hyphen
        assert!(validate_ticker_format("-").is_err());
    }

    #[rstest]
    fn test_parse_ticker_currencies_valid() {
        let (base, quote) = parse_ticker_currencies("BTC-USD").unwrap();
        assert_eq!(base, "BTC");
        assert_eq!(quote, "USD");

        let (base, quote) = parse_ticker_currencies("ETH-USDC").unwrap();
        assert_eq!(base, "ETH");
        assert_eq!(quote, "USDC");
    }

    #[rstest]
    fn test_parse_ticker_currencies_invalid() {
        assert!(parse_ticker_currencies("INVALID").is_err());
        assert!(parse_ticker_currencies("BTC-USD-PERP").is_err());
    }

    #[rstest]
    fn test_validate_stop_limit_buy_valid() {
        let result = validate_conditional_order(
            DydxOrderType::StopLimit,
            Some(dec!(51000)), // trigger
            dec!(50000),       // limit price
            OrderSide::Buy,
        );
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_stop_limit_buy_invalid() {
        // Invalid: trigger below limit
        let result = validate_conditional_order(
            DydxOrderType::StopLimit,
            Some(dec!(49000)),
            dec!(50000),
            OrderSide::Buy,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be >= limit price")
        );
    }

    #[rstest]
    fn test_validate_stop_limit_sell_valid() {
        let result = validate_conditional_order(
            DydxOrderType::StopLimit,
            Some(dec!(49000)), // trigger
            dec!(50000),       // limit price
            OrderSide::Sell,
        );
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_stop_limit_sell_invalid() {
        // Invalid: trigger above limit
        let result = validate_conditional_order(
            DydxOrderType::StopLimit,
            Some(dec!(51000)),
            dec!(50000),
            OrderSide::Sell,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be <= limit price")
        );
    }

    #[rstest]
    fn test_validate_take_profit_sell_valid() {
        let result = validate_conditional_order(
            DydxOrderType::TakeProfitLimit,
            Some(dec!(51000)), // trigger
            dec!(50000),       // limit price
            OrderSide::Sell,
        );
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_take_profit_buy_valid() {
        let result = validate_conditional_order(
            DydxOrderType::TakeProfitLimit,
            Some(dec!(49000)), // trigger
            dec!(50000),       // limit price
            OrderSide::Buy,
        );
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_missing_trigger_price() {
        let result =
            validate_conditional_order(DydxOrderType::StopLimit, None, dec!(50000), OrderSide::Buy);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("trigger_price required")
        );
    }

    #[rstest]
    fn test_validate_non_conditional_order() {
        // Should pass for non-conditional orders
        let result =
            validate_conditional_order(DydxOrderType::Limit, None, dec!(50000), OrderSide::Buy);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_calculate_tif_market() {
        let tif = calculate_time_in_force(DydxOrderType::Market, DydxTimeInForce::Gtt, false, None)
            .unwrap();
        assert_eq!(tif, TimeInForce::Ioc);
    }

    #[rstest]
    fn test_calculate_tif_limit_post_only() {
        let tif = calculate_time_in_force(DydxOrderType::Limit, DydxTimeInForce::Gtt, true, None)
            .unwrap();
        assert_eq!(tif, TimeInForce::Gtc); // Post-only uses GTC with post_only flag
    }

    #[rstest]
    fn test_calculate_tif_limit_gtc() {
        let tif = calculate_time_in_force(DydxOrderType::Limit, DydxTimeInForce::Gtt, false, None)
            .unwrap();
        assert_eq!(tif, TimeInForce::Gtc);
    }

    #[rstest]
    fn test_calculate_tif_stop_market_ioc() {
        let tif = calculate_time_in_force(
            DydxOrderType::StopMarket,
            DydxTimeInForce::Gtt,
            false,
            Some(DydxOrderExecution::Ioc),
        )
        .unwrap();
        assert_eq!(tif, TimeInForce::Ioc);
    }

    #[rstest]
    fn test_calculate_tif_stop_limit_post_only() {
        let tif = calculate_time_in_force(
            DydxOrderType::StopLimit,
            DydxTimeInForce::Gtt,
            false,
            Some(DydxOrderExecution::PostOnly),
        )
        .unwrap();
        assert_eq!(tif, TimeInForce::Gtc); // Post-only uses GTC with post_only flag
    }

    #[rstest]
    fn test_calculate_tif_stop_limit_gtc() {
        let tif =
            calculate_time_in_force(DydxOrderType::StopLimit, DydxTimeInForce::Gtt, false, None)
                .unwrap();
        assert_eq!(tif, TimeInForce::Gtc);
    }

    #[rstest]
    fn test_calculate_tif_stop_market_invalid_post_only() {
        let result = calculate_time_in_force(
            DydxOrderType::StopMarket,
            DydxTimeInForce::Gtt,
            false,
            Some(DydxOrderExecution::PostOnly),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("PostOnly not supported")
        );
    }

    #[rstest]
    fn test_calculate_tif_trailing_stop() {
        let tif = calculate_time_in_force(
            DydxOrderType::TrailingStop,
            DydxTimeInForce::Gtt,
            false,
            None,
        )
        .unwrap();
        assert_eq!(tif, TimeInForce::Gtc);
    }

    #[rstest]
    fn test_parse_perpetual_markets() {
        let json = load_json_result_fixture("http_get_perpetual_markets.json");
        let response: MarketsResponse =
            serde_json::from_value(json).expect("Failed to parse markets");

        assert_eq!(response.markets.len(), 3);
        assert!(response.markets.contains_key("BTC-USD"));
        assert!(response.markets.contains_key("ETH-USD"));
        assert!(response.markets.contains_key("SOL-USD"));

        let btc = response.markets.get("BTC-USD").unwrap();
        assert_eq!(btc.ticker, "BTC-USD");
        assert_eq!(btc.clob_pair_id, 0);
        assert_eq!(btc.atomic_resolution, -10);
    }

    #[rstest]
    fn test_parse_instrument_from_market() {
        let json = load_json_result_fixture("http_get_perpetual_markets.json");
        let response: MarketsResponse =
            serde_json::from_value(json).expect("Failed to parse markets");
        let btc = response.markets.get("BTC-USD").unwrap();

        let ts_init = UnixNanos::default();
        let instrument =
            parse_instrument_any(btc, None, None, ts_init).expect("Failed to parse instrument");

        assert_eq!(instrument.id().symbol.as_str(), "BTC-USD-PERP");
        assert_eq!(instrument.id().venue.as_str(), "DYDX");
    }

    #[rstest]
    fn test_parse_orderbook_response() {
        let json = load_json_result_fixture("http_get_orderbook.json");
        let response: OrderbookResponse =
            serde_json::from_value(json).expect("Failed to parse orderbook");

        assert_eq!(response.bids.len(), 5);
        assert_eq!(response.asks.len(), 5);

        let best_bid = &response.bids[0];
        assert_eq!(best_bid.price.to_string(), "89947");
        assert_eq!(best_bid.size.to_string(), "0.0002");

        let best_ask = &response.asks[0];
        assert_eq!(best_ask.price.to_string(), "89958");
        assert_eq!(best_ask.size.to_string(), "0.1177");
    }

    #[rstest]
    fn test_parse_trades_response() {
        let json = load_json_result_fixture("http_get_trades.json");
        let response: TradesResponse =
            serde_json::from_value(json).expect("Failed to parse trades");

        assert_eq!(response.trades.len(), 3);

        let first_trade = &response.trades[0];
        assert_eq!(first_trade.id, "03f89a550000000200000002");
        assert_eq!(first_trade.side, OrderSide::Buy);
        assert_eq!(first_trade.price.to_string(), "89942");
        assert_eq!(first_trade.size.to_string(), "0.0001");
    }

    #[rstest]
    fn test_parse_candles_response() {
        let json = load_json_result_fixture("http_get_candles.json");
        let response: CandlesResponse =
            serde_json::from_value(json).expect("Failed to parse candles");

        assert_eq!(response.candles.len(), 3);

        let first_candle = &response.candles[0];
        assert_eq!(first_candle.ticker, "BTC-USD");
        assert_eq!(first_candle.open.to_string(), "89934");
        assert_eq!(first_candle.high.to_string(), "89970");
        assert_eq!(first_candle.low.to_string(), "89911");
        assert_eq!(first_candle.close.to_string(), "89941");
    }

    #[rstest]
    fn test_parse_subaccount_response() {
        let json = load_json_result_fixture("http_get_subaccount.json");
        let response: SubaccountResponse =
            serde_json::from_value(json).expect("Failed to parse subaccount");

        let subaccount = &response.subaccount;
        assert_eq!(subaccount.subaccount_number, 0);
        assert_eq!(subaccount.equity.to_string(), "45.201296");
        assert_eq!(subaccount.free_collateral.to_string(), "45.201296");
        assert!(subaccount.margin_enabled);
        assert_eq!(subaccount.open_perpetual_positions.len(), 0);
    }

    #[rstest]
    fn test_parse_orders_response() {
        let json = load_json_result_fixture("http_get_orders.json");
        let response: Vec<Order> = serde_json::from_value(json).expect("Failed to parse orders");

        assert_eq!(response.len(), 3);

        let first_order = &response[0];
        assert_eq!(first_order.id, "0f0981cb-152e-57d3-bea9-4d8e0dd5ed35");
        assert_eq!(first_order.side, OrderSide::Buy);
        assert_eq!(first_order.order_type, DydxOrderType::Limit);
        assert!(first_order.reduce_only);

        let second_order = &response[1];
        assert_eq!(second_order.side, OrderSide::Sell);
        assert!(!second_order.reduce_only);
    }

    #[rstest]
    fn test_parse_fills_response() {
        let json = load_json_result_fixture("http_get_fills.json");
        let response: FillsResponse = serde_json::from_value(json).expect("Failed to parse fills");

        assert_eq!(response.fills.len(), 3);

        let first_fill = &response.fills[0];
        assert_eq!(first_fill.id, "6450e369-1dc3-5229-8dc2-fb3b5d1cf2ab");
        assert_eq!(first_fill.side, OrderSide::Buy);
        assert_eq!(first_fill.market, "BTC-USD");
        assert_eq!(first_fill.price.to_string(), "105117");
    }

    #[rstest]
    fn test_parse_transfers_response() {
        let json = load_json_result_fixture("http_get_transfers.json");
        let response: TransfersResponse =
            serde_json::from_value(json).expect("Failed to parse transfers");

        assert_eq!(response.transfers.len(), 1);

        let deposit = &response.transfers[0];
        assert_eq!(deposit.transfer_type, DydxTransferType::Deposit);
        assert_eq!(deposit.asset, "USDC");
        assert_eq!(deposit.amount.to_string(), "45.334703");
    }

    #[rstest]
    fn test_transfer_type_enum_serde() {
        // Test all transfer type variants serialize/deserialize correctly
        let test_cases = vec![
            (DydxTransferType::Deposit, "\"DEPOSIT\""),
            (DydxTransferType::Withdrawal, "\"WITHDRAWAL\""),
            (DydxTransferType::TransferIn, "\"TRANSFER_IN\""),
            (DydxTransferType::TransferOut, "\"TRANSFER_OUT\""),
        ];

        for (variant, expected_json) in test_cases {
            // Test serialization
            let serialized = serde_json::to_string(&variant).expect("Failed to serialize");
            assert_eq!(
                serialized, expected_json,
                "Serialization failed for {variant:?}"
            );

            // Test deserialization
            let deserialized: DydxTransferType =
                serde_json::from_str(&serialized).expect("Failed to deserialize");
            assert_eq!(
                deserialized, variant,
                "Deserialization failed for {variant:?}"
            );
        }
    }
}

use std::str::FromStr;

use nautilus_core::UUID4;
use nautilus_model::{
    enums::{LiquiditySide, OrderStatus, PositionSide, TriggerType},
    identifiers::{AccountId, ClientOrderId, PositionId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Price, Quantity},
};

use super::models::{Fill, Order, PerpetualPosition};
use crate::common::enums::{DydxLiquidity, DydxOrderStatus};

/// Map dYdX order status to Nautilus OrderStatus.
fn parse_order_status(status: &DydxOrderStatus) -> OrderStatus {
    match status {
        DydxOrderStatus::Open => OrderStatus::Accepted,
        DydxOrderStatus::Filled => OrderStatus::Filled,
        DydxOrderStatus::Canceled => OrderStatus::Canceled,
        DydxOrderStatus::BestEffortCanceled => OrderStatus::Canceled,
        DydxOrderStatus::Untriggered => OrderStatus::Accepted, // Conditional orders waiting for trigger
        DydxOrderStatus::BestEffortOpened => OrderStatus::Accepted,
        DydxOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
    }
}

/// Parse a dYdX Order into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_order_status_report(
    order: &Order,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.id);
    let client_order_id = if order.client_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(&order.client_id))
    };

    let order_type = order.order_type.into();

    let execution = order.execution.or({
        // Infer execution type from post_only flag if not explicitly set
        if order.post_only {
            Some(DydxOrderExecution::PostOnly)
        } else {
            Some(DydxOrderExecution::Default)
        }
    });
    let time_in_force = calculate_time_in_force(
        order.order_type,
        order.time_in_force,
        order.reduce_only,
        execution,
    )?;

    let order_side = order.side;
    let order_status = parse_order_status(&order.status);

    let size_precision = instrument.size_precision();
    let quantity = Quantity::from_decimal_dp(order.size, size_precision)
        .context("failed to parse order size")?;
    let filled_qty = Quantity::from_decimal_dp(order.total_filled, size_precision)
        .context("failed to parse total_filled")?;

    let price_precision = instrument.price_precision();
    let price = Price::from_decimal_dp(order.price, price_precision)
        .context("failed to parse order price")?;

    // Use updated_at for both ts_accepted and ts_last (not good_til_block_time which is the expiry)
    let ts_accepted = order.updated_at.map_or(ts_init, |dt| {
        UnixNanos::from(dt.timestamp_millis() as u64 * 1_000_000)
    });
    let ts_last = ts_accepted;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(UUID4::new()),
    );

    report = report.with_price(price);

    if let Some(trigger_price_dec) = order.trigger_price {
        let trigger_price = Price::from_decimal_dp(trigger_price_dec, instrument.price_precision())
            .context("failed to parse trigger_price")?;
        report = report.with_trigger_price(trigger_price);

        if let Some(condition_type) = order.condition_type {
            let trigger_type = match condition_type {
                crate::common::enums::DydxConditionType::StopLoss => TriggerType::LastPrice,
                crate::common::enums::DydxConditionType::TakeProfit => TriggerType::LastPrice,
                crate::common::enums::DydxConditionType::Unspecified => TriggerType::Default,
            };
            report = report.with_trigger_type(trigger_type);
        }
    }

    Ok(report)
}

/// Parse a dYdX Fill into a Nautilus FillReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_fill_report(
    fill: &Fill,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new(&fill.id);
    let order_side = fill.side;

    let size_precision = instrument.size_precision();
    let price_precision = instrument.price_precision();

    let last_qty = Quantity::from_decimal_dp(fill.size, size_precision)
        .context("failed to parse fill size")?;
    let last_px = Price::from_decimal_dp(fill.price, price_precision)
        .context("failed to parse fill price")?;

    // Parse commission (fee)
    //
    // Negate dYdX fee to match Nautilus conventions:
    // - dYdX: negative fee = rebate, positive fee = cost
    // - Nautilus: positive commission = rebate, negative commission = cost
    // Reference: OKX and Bybit adapters also negate venue fees
    let commission = Money::from_decimal(-fill.fee, instrument.quote_currency())
        .context("failed to parse fee")?;

    let liquidity_side = match fill.liquidity {
        DydxLiquidity::Maker => LiquiditySide::Maker,
        DydxLiquidity::Taker => LiquiditySide::Taker,
    };

    let ts_event = UnixNanos::from(fill.created_at.timestamp_millis() as u64 * 1_000_000);

    let report = FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        None, // client_order_id - will be linked by execution engine
        None, // venue_position_id
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    Ok(report)
}

/// Parse a dYdX PerpetualPosition into a Nautilus PositionStatusReport.
///
/// # Errors
///
/// Returns an error if required fields are missing or invalid.
pub fn parse_position_status_report(
    position: &PerpetualPosition,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Determine position side based on size (negative for short)
    let position_side = if position.size.is_zero() {
        PositionSide::Flat
    } else if position.size.is_sign_positive() {
        PositionSide::Long
    } else {
        PositionSide::Short
    };

    // Create quantity (always positive)
    let quantity = Quantity::from_decimal_dp(position.size.abs(), instrument.size_precision())
        .context("failed to parse position size")?;

    let avg_px_open = position.entry_price;
    let ts_last = UnixNanos::from(position.created_at.timestamp_millis() as u64 * 1_000_000);

    let venue_position_id = Some(PositionId::new(format!(
        "{}_{}",
        account_id, position.market
    )));

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side.as_specified(),
        quantity,
        ts_last,
        ts_init,
        Some(UUID4::new()),
        venue_position_id,
        Some(avg_px_open),
    ))
}

/// Parse a dYdX subaccount info into a Nautilus AccountState.
///
/// dYdX provides account-level balances with:
/// - `equity`: Total account value (total balance)
/// - `freeCollateral`: Available for new orders (free balance)
/// - `locked`: equity - freeCollateral (calculated)
///
/// Margin calculations per position:
/// - `initial_margin = margin_init * abs(position_size) * oracle_price`
/// - `maintenance_margin = margin_maint * abs(position_size) * oracle_price`
///
/// # Errors
///
/// Returns an error if balance fields cannot be parsed.
pub fn parse_account_state(
    subaccount: &DydxSubaccountInfo,
    account_id: AccountId,
    instruments: &std::collections::HashMap<InstrumentId, InstrumentAny>,
    oracle_prices: &std::collections::HashMap<InstrumentId, Decimal>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    use std::collections::HashMap;

    use nautilus_model::{
        enums::AccountType,
        events::AccountState,
        types::{AccountBalance, MarginBalance},
    };

    let mut balances = Vec::new();

    // Parse equity (total) and freeCollateral (free)
    let equity: Decimal = subaccount
        .equity
        .parse()
        .context(format!("Failed to parse equity '{}'", subaccount.equity))?;

    let free_collateral: Decimal = subaccount.free_collateral.parse().context(format!(
        "Failed to parse freeCollateral '{}'",
        subaccount.free_collateral
    ))?;

    // dYdX uses USDC as the settlement currency
    let currency = Currency::get_or_create_crypto_with_context("USDC", None);

    let total = Money::from_decimal(equity, currency).context("failed to parse equity")?;
    let free = Money::from_decimal(free_collateral, currency)
        .context("failed to parse free collateral")?;
    let locked = total - free;

    let balance = AccountBalance::new_checked(total, locked, free)
        .context("Failed to create AccountBalance from subaccount data")?;
    balances.push(balance);

    // Calculate margin balances from open positions
    let mut margins = Vec::new();
    let mut initial_margins: HashMap<Currency, Decimal> = HashMap::new();
    let mut maintenance_margins: HashMap<Currency, Decimal> = HashMap::new();

    if let Some(ref positions) = subaccount.open_perpetual_positions {
        for position in positions.values() {
            // Parse instrument ID from market symbol (e.g., "BTC-USD" -> "BTC-USD-PERP")
            let market_str = position.market.as_str();
            let instrument_id = parse_instrument_id(market_str);

            // Get instrument to access margin parameters
            let instrument = match instruments.get(&instrument_id) {
                Some(inst) => inst,
                None => {
                    log::warn!(
                        "Cannot calculate margin for position {market_str}: instrument not found"
                    );
                    continue;
                }
            };

            // Get margin parameters from instrument
            let (margin_init, margin_maint) = match instrument {
                InstrumentAny::CryptoPerpetual(perp) => (perp.margin_init, perp.margin_maint),
                _ => {
                    log::warn!(
                        "Instrument {instrument_id} is not a CryptoPerpetual, skipping margin calculation"
                    );
                    continue;
                }
            };

            // Parse position size
            let position_size = match Decimal::from_str(&position.size) {
                Ok(size) => size.abs(),
                Err(e) => {
                    log::warn!(
                        "Failed to parse position size '{}' for {}: {}",
                        position.size,
                        market_str,
                        e
                    );
                    continue;
                }
            };

            // Skip closed positions
            if position_size.is_zero() {
                continue;
            }

            // Get oracle price, fallback to entry price
            let oracle_price = oracle_prices
                .get(&instrument_id)
                .copied()
                .or_else(|| Decimal::from_str(&position.entry_price).ok())
                .unwrap_or(Decimal::ZERO);

            if oracle_price.is_zero() {
                log::warn!("No valid price for position {market_str}, skipping margin calculation");
                continue;
            }

            // Calculate margins: margin_fraction * abs(size) * oracle_price
            let initial_margin = margin_init * position_size * oracle_price;

            let maintenance_margin = margin_maint * position_size * oracle_price;

            // Aggregate margins by currency
            let quote_currency = instrument.quote_currency();
            *initial_margins
                .entry(quote_currency)
                .or_insert(Decimal::ZERO) += initial_margin;
            *maintenance_margins
                .entry(quote_currency)
                .or_insert(Decimal::ZERO) += maintenance_margin;
        }
    }

    // Create MarginBalance objects from aggregated margins
    for (currency, initial_margin) in initial_margins {
        let maintenance_margin = maintenance_margins
            .get(&currency)
            .copied()
            .unwrap_or(Decimal::ZERO);

        let initial_money = Money::from_decimal(initial_margin, currency).context(format!(
            "Failed to create initial margin Money for {currency}"
        ))?;
        let maintenance_money = Money::from_decimal(maintenance_margin, currency).context(
            format!("Failed to create maintenance margin Money for {currency}"),
        )?;

        // Create synthetic instrument ID for account-level margin
        // Format: ACCOUNT.DYDX (similar to OKX pattern)
        let margin_instrument_id = InstrumentId::new(Symbol::new("ACCOUNT"), Venue::new("DYDX"));

        let margin_balance =
            MarginBalance::new(initial_money, maintenance_money, margin_instrument_id);
        margins.push(margin_balance);
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Margin, // dYdX uses cross-margin
        balances,
        margins,
        true, // is_reported - comes from venue
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency - dYdX settles in USDC
    ))
}

/// Parses an account state from HTTP subaccount response.
///
/// Unlike `parse_account_state` which handles WebSocket types with string fields,
/// this function handles HTTP API types with pre-parsed Decimal fields.
///
/// # Errors
///
/// Returns an error if balance fields cannot be parsed.
pub fn parse_http_account_state(
    subaccount: &crate::http::models::Subaccount,
    account_id: AccountId,
    instruments: &std::collections::HashMap<InstrumentId, InstrumentAny>,
    oracle_prices: &std::collections::HashMap<InstrumentId, Decimal>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    use std::collections::HashMap;

    use nautilus_model::{
        enums::AccountType,
        events::AccountState,
        types::{AccountBalance, MarginBalance},
    };

    let mut balances = Vec::new();
    let equity = subaccount.equity;
    let free_collateral = subaccount.free_collateral;

    // dYdX uses USDC as the settlement currency
    let currency = Currency::get_or_create_crypto_with_context("USDC", None);

    let total = Money::from_decimal(equity, currency).context("failed to parse equity")?;
    let free = Money::from_decimal(free_collateral, currency)
        .context("failed to parse free collateral")?;
    let locked = total - free;

    let balance = AccountBalance::new_checked(total, locked, free)
        .context("Failed to create AccountBalance from subaccount data")?;
    balances.push(balance);

    let mut margins = Vec::new();
    let mut initial_margins: HashMap<Currency, Decimal> = HashMap::new();
    let mut maintenance_margins: HashMap<Currency, Decimal> = HashMap::new();

    for (market_str, position) in &subaccount.open_perpetual_positions {
        // Transform market symbol (e.g., "BTC-USD" -> "BTC-USD-PERP")
        let instrument_id = parse_instrument_id(market_str);

        let instrument = match instruments.get(&instrument_id) {
            Some(inst) => inst,
            None => {
                log::warn!(
                    "Cannot calculate margin for position {market_str}: instrument not found"
                );
                continue;
            }
        };

        let (margin_init, margin_maint) = match instrument {
            InstrumentAny::CryptoPerpetual(perp) => (perp.margin_init, perp.margin_maint),
            _ => {
                log::warn!(
                    "Instrument {instrument_id} is not a CryptoPerpetual, skipping margin calculation"
                );
                continue;
            }
        };

        // HTTP position.size is already Decimal (unlike WS which uses strings)
        let position_size = position.size.abs();
        if position_size.is_zero() {
            continue;
        }

        // Fallback to entry price if oracle price not available
        let oracle_price = oracle_prices
            .get(&instrument_id)
            .copied()
            .unwrap_or(position.entry_price);

        if oracle_price.is_zero() {
            log::warn!("No valid price for position {market_str}, skipping margin calculation");
            continue;
        }

        // margin = margin_fraction * abs(size) * oracle_price
        let initial_margin = margin_init * position_size * oracle_price;
        let maintenance_margin = margin_maint * position_size * oracle_price;

        let quote_currency = instrument.quote_currency();
        *initial_margins
            .entry(quote_currency)
            .or_insert(Decimal::ZERO) += initial_margin;
        *maintenance_margins
            .entry(quote_currency)
            .or_insert(Decimal::ZERO) += maintenance_margin;
    }

    for (currency, initial_margin) in initial_margins {
        let maintenance_margin = maintenance_margins
            .get(&currency)
            .copied()
            .unwrap_or(Decimal::ZERO);

        let initial_money = Money::from_decimal(initial_margin, currency).context(format!(
            "Failed to create initial margin Money for {currency}"
        ))?;
        let maintenance_money = Money::from_decimal(maintenance_margin, currency).context(
            format!("Failed to create maintenance margin Money for {currency}"),
        )?;

        // Synthetic instrument ID for account-level margin (not a real instrument)
        let margin_instrument_id = InstrumentId::new(Symbol::new("ACCOUNT"), Venue::new("DYDX"));

        let margin_balance =
            MarginBalance::new(initial_money, maintenance_money, margin_instrument_id);
        margins.push(margin_balance);
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Margin, // dYdX uses cross-margin
        balances,
        margins,
        true, // is_reported - comes from venue
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency - dYdX settles in USDC
    ))
}

#[cfg(test)]
mod reconciliation_tests {
    use chrono::Utc;
    use nautilus_model::{
        enums::{OrderSide, OrderStatus, TimeInForce},
        identifiers::{AccountId, InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, Instrument},
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    use super::*;

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), Venue::new("DYDX"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            instrument_id.symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,                                // price_precision
            8,                                // size_precision
            Price::new(0.01, 2),              // price_increment
            Quantity::new(0.001, 8),          // size_increment
            Some(Quantity::new(1.0, 0)),      // multiplier
            Some(Quantity::new(0.001, 8)),    // lot_size
            Some(Quantity::new(100000.0, 8)), // max_quantity
            Some(Quantity::new(0.001, 8)),    // min_quantity
            None,                             // max_notional
            None,                             // min_notional
            Some(Price::new(1000000.0, 2)),   // max_price
            Some(Price::new(0.01, 2)),        // min_price
            Some(dec!(0.05)),                 // margin_init
            Some(dec!(0.03)),                 // margin_maint
            Some(dec!(0.0002)),               // maker_fee
            Some(dec!(0.0005)),               // taker_fee
            UnixNanos::default(),             // ts_event
            UnixNanos::default(),             // ts_init
        ))
    }

    #[rstest]
    fn test_parse_order_status() {
        assert_eq!(
            parse_order_status(&DydxOrderStatus::Open),
            OrderStatus::Accepted
        );
        assert_eq!(
            parse_order_status(&DydxOrderStatus::Filled),
            OrderStatus::Filled
        );
        assert_eq!(
            parse_order_status(&DydxOrderStatus::Canceled),
            OrderStatus::Canceled
        );
        assert_eq!(
            parse_order_status(&DydxOrderStatus::PartiallyFilled),
            OrderStatus::PartiallyFilled
        );
        assert_eq!(
            parse_order_status(&DydxOrderStatus::Untriggered),
            OrderStatus::Accepted
        );
    }

    #[rstest]
    fn test_parse_order_status_report_basic() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let order = Order {
            id: "order123".to_string(),
            subaccount_id: "subacct1".to_string(),
            client_id: "client1".to_string(),
            clob_pair_id: 1,
            side: OrderSide::Buy,
            size: dec!(1.5),
            total_filled: dec!(1.0),
            price: dec!(50000.0),
            status: DydxOrderStatus::PartiallyFilled,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            reduce_only: false,
            post_only: false,
            order_flags: 0,
            good_til_block: None,
            good_til_block_time: Some(Utc::now()),
            created_at_height: Some(1000),
            client_metadata: 0,
            trigger_price: None,
            condition_type: None,
            conditional_order_trigger_subticks: None,
            execution: None,
            updated_at: Some(Utc::now()),
            updated_at_height: Some(1001),
            ticker: None,
            subaccount_number: 0,
            order_router_address: None,
        };

        let result = parse_order_status_report(&order, &instrument, account_id, ts_init);
        if let Err(ref e) = result {
            eprintln!("Parse error: {e:?}");
        }
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
    }

    #[rstest]
    fn test_parse_order_status_report_conditional() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let order = Order {
            id: "order456".to_string(),
            subaccount_id: "subacct1".to_string(),
            client_id: String::new(), // Empty client ID
            clob_pair_id: 1,
            side: OrderSide::Sell,
            size: dec!(2.0),
            total_filled: dec!(0.0),
            price: dec!(51000.0),
            status: DydxOrderStatus::Untriggered,
            order_type: DydxOrderType::StopLimit,
            time_in_force: DydxTimeInForce::Gtt,
            reduce_only: true,
            post_only: false,
            order_flags: 0,
            good_til_block: None,
            good_til_block_time: Some(Utc::now()),
            created_at_height: Some(1000),
            client_metadata: 0,
            trigger_price: Some(dec!(49000.0)),
            condition_type: Some(crate::common::enums::DydxConditionType::StopLoss),
            conditional_order_trigger_subticks: Some(490000),
            execution: None,
            updated_at: Some(Utc::now()),
            updated_at_height: Some(1001),
            ticker: None,
            subaccount_number: 0,
            order_router_address: None,
        };

        let result = parse_order_status_report(&order, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.client_order_id, None);
        assert!(report.trigger_price.is_some());
        assert_eq!(report.trigger_price.unwrap().as_f64(), 49000.0);
    }

    #[rstest]
    fn test_parse_fill_report() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let fill = Fill {
            id: "fill789".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Taker,
            fill_type: crate::common::enums::DydxFillType::Limit,
            market: "BTC-USD".to_string(),
            market_type: crate::common::enums::DydxTickerType::Perpetual,
            price: dec!(50100.0),
            size: dec!(1.0),
            fee: dec!(-5.01),
            created_at: Utc::now(),
            created_at_height: 1000,
            order_id: "order123".to_string(),
            client_metadata: 0,
        };

        let result = parse_fill_report(&fill, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.last_px.as_f64(), 50100.0);
        assert_eq!(report.commission.as_f64(), 5.01);
    }

    #[rstest]
    fn test_parse_position_status_report_long() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let position = PerpetualPosition {
            market: "BTC-USD".to_string(),
            status: crate::common::enums::DydxPositionStatus::Open,
            side: OrderSide::Buy,
            size: dec!(2.5),
            max_size: dec!(3.0),
            entry_price: dec!(49500.0),
            exit_price: None,
            realized_pnl: dec!(100.0),
            created_at_height: 1000,
            created_at: Utc::now(),
            sum_open: dec!(2.5),
            sum_close: dec!(0.0),
            net_funding: dec!(-2.5),
            unrealized_pnl: dec!(250.0),
            closed_at: None,
        };

        let result = parse_position_status_report(&position, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.position_side, PositionSide::Long.as_specified());
        assert_eq!(report.quantity.as_f64(), 2.5);
        assert_eq!(report.avg_px_open.unwrap().to_f64().unwrap(), 49500.0);
    }

    #[rstest]
    fn test_parse_position_status_report_short() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let position = PerpetualPosition {
            market: "BTC-USD".to_string(),
            status: crate::common::enums::DydxPositionStatus::Open,
            side: OrderSide::Sell,
            size: dec!(-1.5),
            max_size: dec!(1.5),
            entry_price: dec!(51000.0),
            exit_price: None,
            realized_pnl: dec!(0.0),
            created_at_height: 1000,
            created_at: Utc::now(),
            sum_open: dec!(1.5),
            sum_close: dec!(0.0),
            net_funding: dec!(1.2),
            unrealized_pnl: dec!(-150.0),
            closed_at: None,
        };

        let result = parse_position_status_report(&position, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.position_side, PositionSide::Short.as_specified());
        assert_eq!(report.quantity.as_f64(), 1.5);
    }

    #[rstest]
    fn test_parse_position_status_report_flat() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let position = PerpetualPosition {
            market: "BTC-USD".to_string(),
            status: crate::common::enums::DydxPositionStatus::Closed,
            side: OrderSide::Buy,
            size: dec!(0.0),
            max_size: dec!(2.0),
            entry_price: dec!(50000.0),
            exit_price: Some(dec!(51000.0)),
            realized_pnl: dec!(500.0),
            created_at_height: 1000,
            created_at: Utc::now(),
            sum_open: dec!(2.0),
            sum_close: dec!(2.0),
            net_funding: dec!(-5.0),
            unrealized_pnl: dec!(0.0),
            closed_at: Some(Utc::now()),
        };

        let result = parse_position_status_report(&position, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.position_side, PositionSide::Flat.as_specified());
        assert_eq!(report.quantity.as_f64(), 0.0);
    }

    /// Test external order detection (orders not created by this client)
    #[rstest]
    fn test_parse_order_external_detection() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        // External order: created by different client (e.g., web UI)
        let order = Order {
            id: "external-order-123".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "99999".to_string(),
            clob_pair_id: 1,
            side: OrderSide::Buy,
            size: dec!(0.5),
            total_filled: dec!(0.0),
            price: dec!(50000.0),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            reduce_only: false,
            post_only: false,
            order_flags: 0,
            good_til_block: Some(1000),
            good_til_block_time: None,
            created_at_height: Some(900),
            client_metadata: 0,
            trigger_price: None,
            condition_type: None,
            conditional_order_trigger_subticks: None,
            execution: None,
            updated_at: Some(Utc::now()),
            updated_at_height: Some(900),
            ticker: None,
            subaccount_number: 0,
            order_router_address: None,
        };

        let result = parse_order_status_report(&order, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        // External orders should still be reconciled correctly
        assert_eq!(report.filled_qty.as_f64(), 0.0);
    }

    /// Test order reconciliation with partial fills
    #[rstest]
    fn test_parse_order_partial_fill_reconciliation() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let order = Order {
            id: "partial-order-123".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "12345".to_string(),
            clob_pair_id: 1,
            side: OrderSide::Buy,
            size: dec!(2.0),
            total_filled: dec!(0.75),
            price: dec!(50000.0),
            status: DydxOrderStatus::PartiallyFilled,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            reduce_only: false,
            post_only: false,
            order_flags: 0,
            good_til_block: Some(2000),
            good_til_block_time: None,
            created_at_height: Some(1500),
            client_metadata: 0,
            trigger_price: None,
            condition_type: None,
            conditional_order_trigger_subticks: None,
            execution: None,
            updated_at: Some(Utc::now()),
            updated_at_height: Some(1600),
            ticker: None,
            subaccount_number: 0,
            order_router_address: None,
        };

        let result = parse_order_status_report(&order, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.filled_qty.as_f64(), 0.75);
        assert_eq!(report.quantity.as_f64(), 2.0);
    }

    /// Test reconciliation with multiple positions (long and short)
    #[rstest]
    fn test_parse_multiple_positions() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        // Position 1: Long position
        let long_position = PerpetualPosition {
            market: "BTC-USD".to_string(),
            status: crate::common::enums::DydxPositionStatus::Open,
            side: OrderSide::Buy,
            size: dec!(1.5),
            max_size: dec!(1.5),
            entry_price: dec!(49000.0),
            exit_price: None,
            realized_pnl: dec!(0.0),
            created_at_height: 1000,
            created_at: Utc::now(),
            sum_open: dec!(1.5),
            sum_close: dec!(0.0),
            net_funding: dec!(-1.0),
            unrealized_pnl: dec!(150.0),
            closed_at: None,
        };

        let result1 =
            parse_position_status_report(&long_position, &instrument, account_id, ts_init);
        assert!(result1.is_ok());
        let report1 = result1.unwrap();
        assert_eq!(report1.position_side, PositionSide::Long.as_specified());

        // Position 2: Short position (should be handled separately if from different market)
        let short_position = PerpetualPosition {
            market: "BTC-USD".to_string(),
            status: crate::common::enums::DydxPositionStatus::Open,
            side: OrderSide::Sell,
            size: dec!(-2.0),
            max_size: dec!(2.0),
            entry_price: dec!(51000.0),
            exit_price: None,
            realized_pnl: dec!(0.0),
            created_at_height: 1100,
            created_at: Utc::now(),
            sum_open: dec!(2.0),
            sum_close: dec!(0.0),
            net_funding: dec!(0.5),
            unrealized_pnl: dec!(-200.0),
            closed_at: None,
        };

        let result2 =
            parse_position_status_report(&short_position, &instrument, account_id, ts_init);
        assert!(result2.is_ok());
        let report2 = result2.unwrap();
        assert_eq!(report2.position_side, PositionSide::Short.as_specified());
    }

    /// Test fill reconciliation with zero fee
    #[rstest]
    fn test_parse_fill_zero_fee() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let fill = Fill {
            id: "fill-zero-fee".to_string(),
            side: OrderSide::Sell,
            liquidity: DydxLiquidity::Maker,
            fill_type: crate::common::enums::DydxFillType::Limit,
            market: "BTC-USD".to_string(),
            market_type: crate::common::enums::DydxTickerType::Perpetual,
            price: dec!(50000.0),
            size: dec!(0.1),
            fee: dec!(0.0), // Zero fee (e.g., fee rebate or promotional period)
            created_at: Utc::now(),
            created_at_height: 1000,
            order_id: "order-zero-fee".to_string(),
            client_metadata: 0,
        };

        let result = parse_fill_report(&fill, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.commission.as_f64(), 0.0);
    }

    /// Test fill reconciliation with maker rebate (negative fee)
    #[rstest]
    fn test_parse_fill_maker_rebate() {
        let instrument = create_test_instrument();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let fill = Fill {
            id: "fill-maker-rebate".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: crate::common::enums::DydxFillType::Limit,
            market: "BTC-USD".to_string(),
            market_type: crate::common::enums::DydxTickerType::Perpetual,
            price: dec!(50000.0),
            size: dec!(1.0),
            fee: dec!(-2.5), // Negative fee = rebate
            created_at: Utc::now(),
            created_at_height: 1000,
            order_id: "order-maker-rebate".to_string(),
            client_metadata: 0,
        };

        let result = parse_fill_report(&fill, &instrument, account_id, ts_init);
        assert!(result.is_ok());

        let report = result.unwrap();
        // Commission should be negated: -(-2.5) = 2.5 (positive = rebate)
        assert_eq!(report.commission.as_f64(), 2.5);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }
}
