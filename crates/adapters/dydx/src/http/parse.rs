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

//! Parsing utilities for converting dYdX v4 Indexer API responses into Nautilus domain models.
//!
//! This module contains functions that transform raw JSON data structures
//! from the dYdX Indexer API into strongly-typed Nautilus data types such as
//! instruments, trades, bars, account states, etc.
//!
//! # Design Principles
//!
//! - **Validation First**: All inputs are validated before parsing
//! - **Contextual Errors**: All errors include context about what was being parsed
//! - **Zero-Copy When Possible**: Uses references and borrows to minimize allocations
//! - **Type Safety**: Leverages Rust's type system to prevent invalid states
//!
//! # Error Handling
//!
//! All parsing functions return `anyhow::Result<T>` with descriptive error messages
//! that include context about the field being parsed and the value that failed.
//! This makes debugging API changes or data issues much easier.
//!
//!

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::Symbol,
    instruments::{CryptoPerpetual, InstrumentAny},
};

use super::models::PerpetualMarket;
use crate::common::{
    enums::DydxMarketStatus,
    parse::{get_currency, parse_decimal, parse_instrument_id, parse_price, parse_quantity},
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
        anyhow::bail!(
            "Invalid ticker format '{}', expected 'BASE-QUOTE' (e.g., 'BTC-USD')",
            ticker
        );
    }
    if parts[0].is_empty() || parts[1].is_empty() {
        anyhow::bail!(
            "Invalid ticker format '{}', base and quote cannot be empty",
            ticker
        );
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

/// Validates that a market is active and tradable.
///
/// # Errors
///
/// Returns an error if the market status is not Active.
pub fn validate_market_active(ticker: &str, status: &DydxMarketStatus) -> anyhow::Result<()> {
    if *status != DydxMarketStatus::Active {
        anyhow::bail!(
            "Market '{}' is not active (status: {:?}). Only active markets can be parsed.",
            ticker,
            status
        );
    }
    Ok(())
}

/// Parses a dYdX perpetual market into a Nautilus [`InstrumentAny`].
///
/// dYdX v4 only supports perpetual markets, so this function creates a
/// [`CryptoPerpetual`] instrument with the appropriate fields mapped from
/// the dYdX market definition.
///
/// # Returns
///
/// Returns an [`InstrumentAny::CryptoPerpetual`] on success.
///
/// # Errors
///
/// Returns an error if:
/// - Market status is not Active.
/// - Ticker format is invalid (not BASE-QUOTE).
/// - Required fields are missing or invalid.
/// - Price or quantity values cannot be parsed.
/// - Currency parsing fails.
/// - Margin fractions are out of valid range.
///
pub fn parse_instrument_any(
    definition: &PerpetualMarket,
    maker_fee: Option<rust_decimal::Decimal>,
    taker_fee: Option<rust_decimal::Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Validate market status - only parse active markets
    validate_market_active(&definition.ticker, &definition.status)?;

    // Parse instrument ID with Nautilus perpetual suffix and keep raw symbol as venue ticker
    let instrument_id = parse_instrument_id(&definition.ticker);
    let raw_symbol = Symbol::from(definition.ticker.as_str());

    // Parse currencies from ticker using helper function
    let (base_str, quote_str) = parse_ticker_currencies(&definition.ticker)
        .context(format!("Failed to parse ticker '{}'", definition.ticker))?;

    let base_currency = get_currency(base_str);
    let quote_currency = get_currency(quote_str);
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

    // Parse min order size with context
    let min_quantity = Some(
        parse_quantity(&definition.min_order_size.to_string(), "min_order_size").context(
            format!(
                "Failed to parse min_order_size '{}' for market '{}'",
                definition.min_order_size, definition.ticker
            ),
        )?,
    );

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::Utc;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::common::enums::DydxTickerType;

    fn create_test_market() -> PerpetualMarket {
        PerpetualMarket {
            clob_pair_id: 1,
            ticker: "BTC-USD".to_string(),
            status: DydxMarketStatus::Active,
            base_asset: "BTC".to_string(),
            quote_asset: "USD".to_string(),
            step_size: Decimal::from_str("0.001").unwrap(),
            tick_size: Decimal::from_str("1").unwrap(),
            index_price: Decimal::from_str("50000").unwrap(),
            oracle_price: Decimal::from_str("50000").unwrap(),
            price_change_24h: Decimal::ZERO,
            next_funding_rate: Decimal::ZERO,
            next_funding_at: Utc::now(),
            min_order_size: Decimal::from_str("0.001").unwrap(),
            market_type: DydxTickerType::Perpetual,
            initial_margin_fraction: Decimal::from_str("0.05").unwrap(),
            maintenance_margin_fraction: Decimal::from_str("0.03").unwrap(),
            base_position_notional: Decimal::from_str("10000").unwrap(),
            incremental_position_size: Decimal::from_str("10000").unwrap(),
            incremental_initial_margin_fraction: Decimal::from_str("0.01").unwrap(),
            max_position_size: Decimal::from_str("100").unwrap(),
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
    fn test_parse_instrument_any_inactive_market() {
        let mut market = create_test_market();
        market.status = DydxMarketStatus::Paused;

        let result = parse_instrument_any(&market, None, None, UnixNanos::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not active"));
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
            "Expected ticker format error, was: {}",
            error_msg
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
    fn test_validate_market_active() {
        assert!(validate_market_active("BTC-USD", &DydxMarketStatus::Active).is_ok());

        assert!(validate_market_active("BTC-USD", &DydxMarketStatus::Paused).is_err());
        assert!(validate_market_active("BTC-USD", &DydxMarketStatus::CancelOnly).is_err());
        assert!(validate_market_active("BTC-USD", &DydxMarketStatus::PostOnly).is_err());
    }
}
