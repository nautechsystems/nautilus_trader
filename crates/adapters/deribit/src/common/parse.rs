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

//! Parsing functions for Deribit API responses into Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{datetime::NANOSECONDS_IN_MICROSECOND, nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::TradeTick,
    enums::{AccountType, AggressorSide, AssetClass, CurrencyType, OptionKind},
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol, TradeId, Venue},
    instruments::{
        CryptoFuture, CryptoPerpetual, CurrencyPair, OptionContract, any::InstrumentAny,
    },
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::consts::DERIBIT_VENUE,
    http::models::{
        DeribitAccountSummary, DeribitInstrument, DeribitInstrumentKind, DeribitOptionType,
        DeribitPublicTrade,
    },
};

/// Extracts server timestamp from response and converts to UnixNanos.
///
/// # Errors
///
/// Returns an error if the server timestamp (us_out) is missing from the response.
pub fn extract_server_timestamp(us_out: Option<u64>) -> anyhow::Result<UnixNanos> {
    let us_out =
        us_out.ok_or_else(|| anyhow::anyhow!("Missing server timestamp (us_out) in response"))?;
    Ok(UnixNanos::from(us_out * NANOSECONDS_IN_MICROSECOND))
}

/// Parses a Deribit instrument into a Nautilus [`InstrumentAny`].
///
/// Returns `Ok(None)` for unsupported instrument types (e.g., combos).
///
/// # Errors
///
/// Returns an error if:
/// - Required fields are missing (e.g., strike price for options)
/// - Timestamp conversion fails
/// - Decimal conversion fails for fees
pub fn parse_deribit_instrument_any(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<Option<InstrumentAny>> {
    match instrument.kind {
        DeribitInstrumentKind::Spot => {
            parse_spot_instrument(instrument, ts_init, ts_event).map(Some)
        }
        DeribitInstrumentKind::Future => {
            // Check if it's a perpetual
            if instrument.instrument_name.as_str().contains("PERPETUAL") {
                parse_perpetual_instrument(instrument, ts_init, ts_event).map(Some)
            } else {
                parse_future_instrument(instrument, ts_init, ts_event).map(Some)
            }
        }
        DeribitInstrumentKind::Option => {
            parse_option_instrument(instrument, ts_init, ts_event).map(Some)
        }
        DeribitInstrumentKind::FutureCombo | DeribitInstrumentKind::OptionCombo => {
            // Skip combos for initial implementation
            Ok(None)
        }
    }
}

/// Parses a spot instrument into a [`CurrencyPair`].
fn parse_spot_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(instrument.instrument_name), *DERIBIT_VENUE);

    let base_currency = Currency::new(
        instrument.base_currency,
        8,
        0,
        instrument.base_currency,
        CurrencyType::Crypto,
    );
    let quote_currency = Currency::new(
        instrument.quote_currency,
        8,
        0,
        instrument.quote_currency,
        CurrencyType::Crypto,
    );

    let price_increment = Price::from(instrument.tick_size.to_string().as_str());
    let size_increment = Quantity::from(instrument.min_trade_amount.to_string().as_str());
    let min_quantity = Quantity::from(instrument.min_trade_amount.to_string().as_str());

    let maker_fee = Decimal::from_str(&instrument.maker_commission.to_string())
        .context("Failed to parse maker_commission")?;
    let taker_fee = Decimal::from_str(&instrument.taker_commission.to_string())
        .context("Failed to parse taker_commission")?;

    let currency_pair = CurrencyPair::new(
        instrument_id,
        instrument.instrument_name.into(),
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        Some(min_quantity),
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(currency_pair))
}

/// Parses a perpetual swap instrument into a [`CryptoPerpetual`].
fn parse_perpetual_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(instrument.instrument_name), *DERIBIT_VENUE);

    let base_currency = Currency::new(
        instrument.base_currency,
        8,
        0,
        instrument.base_currency,
        CurrencyType::Crypto,
    );
    let quote_currency = Currency::new(
        instrument.quote_currency,
        8,
        0,
        instrument.quote_currency,
        CurrencyType::Crypto,
    );
    let settlement_currency = instrument.settlement_currency.map_or(base_currency, |c| {
        Currency::new(c, 8, 0, c, CurrencyType::Crypto)
    });

    let is_inverse = instrument
        .instrument_type
        .as_ref()
        .is_some_and(|t| t == "reversed");

    let price_increment = Price::from(instrument.tick_size.to_string().as_str());
    let size_increment = Quantity::from(instrument.min_trade_amount.to_string().as_str());
    let min_quantity = Quantity::from(instrument.min_trade_amount.to_string().as_str());

    // Contract size represents the multiplier (e.g., 10 USD per contract for BTC-PERPETUAL)
    let multiplier = Some(Quantity::from(
        instrument.contract_size.to_string().as_str(),
    ));
    let lot_size = Some(size_increment);

    let maker_fee = Decimal::from_str(&instrument.maker_commission.to_string())
        .context("Failed to parse maker_commission")?;
    let taker_fee = Decimal::from_str(&instrument.taker_commission.to_string())
        .context("Failed to parse taker_commission")?;

    let perpetual = CryptoPerpetual::new(
        instrument_id,
        instrument.instrument_name.into(),
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        None, // max_quantity - Deribit doesn't specify a hard max
        Some(min_quantity),
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(perpetual))
}

/// Parses a futures instrument into a [`CryptoFuture`].
fn parse_future_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(instrument.instrument_name), *DERIBIT_VENUE);

    let underlying = Currency::new(
        instrument.base_currency,
        8,
        0,
        instrument.base_currency,
        CurrencyType::Crypto,
    );
    let quote_currency = Currency::new(
        instrument.quote_currency,
        8,
        0,
        instrument.quote_currency,
        CurrencyType::Crypto,
    );
    let settlement_currency = instrument.settlement_currency.map_or(underlying, |c| {
        Currency::new(c, 8, 0, c, CurrencyType::Crypto)
    });

    let is_inverse = instrument
        .instrument_type
        .as_ref()
        .is_some_and(|t| t == "reversed");

    // Convert timestamps from milliseconds to nanoseconds
    let activation_ns = (instrument.creation_timestamp as u64) * 1_000_000;
    let expiration_ns = instrument
        .expiration_timestamp
        .context("Missing expiration_timestamp for future")? as u64
        * 1_000_000; // milliseconds to nanoseconds

    let price_increment = Price::from(instrument.tick_size.to_string().as_str());
    let size_increment = Quantity::from(instrument.min_trade_amount.to_string().as_str());
    let min_quantity = Quantity::from(instrument.min_trade_amount.to_string().as_str());

    // Contract size represents the multiplier
    let multiplier = Some(Quantity::from(
        instrument.contract_size.to_string().as_str(),
    ));
    let lot_size = Some(size_increment); // Use min_trade_amount as lot size

    let maker_fee = Decimal::from_str(&instrument.maker_commission.to_string())
        .context("Failed to parse maker_commission")?;
    let taker_fee = Decimal::from_str(&instrument.taker_commission.to_string())
        .context("Failed to parse taker_commission")?;

    let future = CryptoFuture::new(
        instrument_id,
        instrument.instrument_name.into(),
        underlying,
        quote_currency,
        settlement_currency,
        is_inverse,
        UnixNanos::from(activation_ns),
        UnixNanos::from(expiration_ns),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        None, // max_quantity - Deribit doesn't specify a hard max
        Some(min_quantity),
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoFuture(future))
}

/// Parses an options instrument into an [`OptionContract`].
fn parse_option_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(instrument.instrument_name), *DERIBIT_VENUE);

    // Underlying is the base currency symbol (e.g., "BTC")
    let underlying = instrument.base_currency;

    // Settlement currency for Deribit options
    let settlement = instrument
        .settlement_currency
        .unwrap_or(instrument.base_currency);
    let currency = Currency::new(settlement, 8, 0, settlement, CurrencyType::Crypto);

    // Determine option kind
    let option_kind = match instrument.option_type {
        Some(DeribitOptionType::Call) => OptionKind::Call,
        Some(DeribitOptionType::Put) => OptionKind::Put,
        None => anyhow::bail!("Missing option_type for option instrument"),
    };

    // Parse strike price
    let strike = instrument.strike.context("Missing strike for option")?;
    let strike_price = Price::from(strike.to_string().as_str());

    // Convert timestamps from milliseconds to nanoseconds
    let activation_ns = (instrument.creation_timestamp as u64) * 1_000_000;
    let expiration_ns = instrument
        .expiration_timestamp
        .context("Missing expiration_timestamp for option")? as u64
        * 1_000_000;

    let price_increment = Price::from(instrument.tick_size.to_string().as_str());

    // Contract size is the multiplier (e.g., 1.0 for BTC options)
    let multiplier = Quantity::from(instrument.contract_size.to_string().as_str());
    let lot_size = Quantity::from(instrument.min_trade_amount.to_string().as_str());
    let min_quantity = Quantity::from(instrument.min_trade_amount.to_string().as_str());

    let maker_fee = Decimal::from_str(&instrument.maker_commission.to_string())
        .context("Failed to parse maker_commission")?;
    let taker_fee = Decimal::from_str(&instrument.taker_commission.to_string())
        .context("Failed to parse taker_commission")?;

    let option = OptionContract::new(
        instrument_id,
        instrument.instrument_name.into(),
        AssetClass::Cryptocurrency,
        None, // exchange - Deribit doesn't provide separate exchange field
        underlying,
        option_kind,
        strike_price,
        currency,
        UnixNanos::from(activation_ns),
        UnixNanos::from(expiration_ns),
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        None, // max_quantity
        Some(min_quantity),
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::OptionContract(option))
}

/// Parses Deribit account summaries into a Nautilus [`AccountState`].
///
/// Processes multiple currency summaries and creates balance entries for each currency.
///
/// # Errors
///
/// Returns an error if:
/// - Money conversion fails for any balance field
/// - Decimal conversion fails for margin values
pub fn parse_account_state(
    summaries: &[DeribitAccountSummary],
    account_id: AccountId,
    ts_init: UnixNanos,
    ts_event: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    let mut margins = Vec::new();

    // Parse each currency summary
    for summary in summaries {
        let ccy_str = summary.currency.as_str().trim();

        // Skip balances with empty currency codes
        if ccy_str.is_empty() {
            tracing::debug!(
                "Skipping balance detail with empty currency code | raw_data={:?}",
                summary
            );
            continue;
        }

        let currency = Currency::get_or_create_crypto_with_context(
            ccy_str,
            Some("DERIBIT - Parsing account state"),
        );

        // Parse balance: total (equity includes unrealized PnL), locked, free
        // Note: Deribit's available_funds = equity - initial_margin, so we must use equity for total
        let total = Money::new(summary.equity, currency);
        let free = Money::new(summary.available_funds, currency);
        let locked = Money::from_raw(total.raw - free.raw, currency);

        let balance = AccountBalance::new(total, locked, free);
        balances.push(balance);

        // Parse margin balances if present
        if let (Some(initial_margin), Some(maintenance_margin)) =
            (summary.initial_margin, summary.maintenance_margin)
        {
            // Only create margin balance if there are actual margin requirements
            if initial_margin > 0.0 || maintenance_margin > 0.0 {
                let initial = Money::new(initial_margin, currency);
                let maintenance = Money::new(maintenance_margin, currency);

                // Create a synthetic instrument_id for account-level margins
                let margin_instrument_id = InstrumentId::new(
                    Symbol::from_str_unchecked(format!("ACCOUNT-{}", summary.currency)),
                    Venue::new("DERIBIT"),
                );

                margins.push(MarginBalance::new(
                    initial,
                    maintenance,
                    margin_instrument_id,
                ));
            }
        }
    }

    // Ensure at least one balance exists (Nautilus requires non-empty balances)
    if balances.is_empty() {
        let zero_currency = Currency::USD();
        let zero_money = Money::new(0.0, zero_currency);
        let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
        balances.push(zero_balance);
    }

    let account_type = AccountType::Margin;
    let is_reported = true;

    Ok(AccountState::new(
        account_id,
        account_type,
        balances,
        margins,
        is_reported,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

// Parses a Deribit public trade into a Nautilus [`TradeTick`].
///
/// # Errors
///
/// Returns an error if:
/// - The direction is not "buy" or "sell"
/// - Decimal conversion fails for price or size
pub fn parse_trade_tick(
    trade: &DeribitPublicTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    // Parse aggressor side from direction
    let aggressor_side = match trade.direction.as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        other => anyhow::bail!("Invalid trade direction: {other}"),
    };
    let price = Price::new(trade.price, price_precision);
    let size = Quantity::new(trade.amount, size_precision);
    let ts_event = UnixNanos::from((trade.timestamp as u64) * 1_000_000);
    let trade_id = TradeId::new(&trade.trade_id);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        http::models::{
            DeribitAccountSummariesResponse, DeribitJsonRpcResponse, DeribitTradesResponse,
        },
    };

    #[rstest]
    fn test_parse_perpetual_instrument() {
        let json_data = load_test_json("http_get_instrument.json");
        let response: DeribitJsonRpcResponse<DeribitInstrument> =
            serde_json::from_str(&json_data).unwrap();
        let deribit_inst = response.result.expect("Test data must have result");

        let instrument_any =
            parse_deribit_instrument_any(&deribit_inst, UnixNanos::default(), UnixNanos::default())
                .unwrap();
        let instrument = instrument_any.expect("Should parse perpetual instrument");

        let InstrumentAny::CryptoPerpetual(perpetual) = instrument else {
            panic!("Expected CryptoPerpetual, got {instrument:?}");
        };
        assert_eq!(perpetual.id(), InstrumentId::from("BTC-PERPETUAL.DERIBIT"));
        assert_eq!(perpetual.raw_symbol(), Symbol::from("BTC-PERPETUAL"));
        assert_eq!(perpetual.base_currency().unwrap().code, "BTC");
        assert_eq!(perpetual.quote_currency().code, "USD");
        assert_eq!(perpetual.settlement_currency().code, "BTC");
        assert!(perpetual.is_inverse());
        assert_eq!(perpetual.price_precision(), 1);
        assert_eq!(perpetual.size_precision(), 0);
        assert_eq!(perpetual.price_increment(), Price::from("0.5"));
        assert_eq!(perpetual.size_increment(), Quantity::from("10"));
        assert_eq!(perpetual.multiplier(), Quantity::from("10"));
        assert_eq!(perpetual.lot_size(), Some(Quantity::from("10")));
        assert_eq!(perpetual.maker_fee(), dec!(0));
        assert_eq!(perpetual.taker_fee(), dec!(0.0005));
        assert_eq!(perpetual.max_quantity(), None);
        assert_eq!(perpetual.min_quantity(), Some(Quantity::from("10")));
    }

    #[rstest]
    fn test_parse_future_instrument() {
        let json_data = load_test_json("http_get_instruments.json");
        let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
            serde_json::from_str(&json_data).unwrap();
        let instruments = response.result.expect("Test data must have result");
        let deribit_inst = instruments
            .iter()
            .find(|i| i.instrument_name.as_str() == "BTC-27DEC24")
            .expect("Test data must contain BTC-27DEC24");

        let instrument_any =
            parse_deribit_instrument_any(deribit_inst, UnixNanos::default(), UnixNanos::default())
                .unwrap();
        let instrument = instrument_any.expect("Should parse future instrument");

        let InstrumentAny::CryptoFuture(future) = instrument else {
            panic!("Expected CryptoFuture, got {instrument:?}");
        };
        assert_eq!(future.id(), InstrumentId::from("BTC-27DEC24.DERIBIT"));
        assert_eq!(future.raw_symbol(), Symbol::from("BTC-27DEC24"));
        assert_eq!(future.underlying().unwrap(), "BTC");
        assert_eq!(future.quote_currency().code, "USD");
        assert_eq!(future.settlement_currency().code, "BTC");
        assert!(future.is_inverse());

        // Verify timestamps
        assert_eq!(
            future.activation_ns(),
            Some(UnixNanos::from(1719561600000_u64 * 1_000_000))
        );
        assert_eq!(
            future.expiration_ns(),
            Some(UnixNanos::from(1735300800000_u64 * 1_000_000))
        );
        assert_eq!(future.price_precision(), 1);
        assert_eq!(future.size_precision(), 0);
        assert_eq!(future.price_increment(), Price::from("0.5"));
        assert_eq!(future.size_increment(), Quantity::from("10"));
        assert_eq!(future.multiplier(), Quantity::from("10"));
        assert_eq!(future.lot_size(), Some(Quantity::from("10")));
        assert_eq!(future.maker_fee, dec!(0));
        assert_eq!(future.taker_fee, dec!(0.0005));
    }

    #[rstest]
    fn test_parse_option_instrument() {
        let json_data = load_test_json("http_get_instruments.json");
        let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
            serde_json::from_str(&json_data).unwrap();
        let instruments = response.result.expect("Test data must have result");
        let deribit_inst = instruments
            .iter()
            .find(|i| i.instrument_name.as_str() == "BTC-27DEC24-100000-C")
            .expect("Test data must contain BTC-27DEC24-100000-C");

        let instrument_any =
            parse_deribit_instrument_any(deribit_inst, UnixNanos::default(), UnixNanos::default())
                .unwrap();
        let instrument = instrument_any.expect("Should parse option instrument");

        // Verify it's an OptionContract
        let InstrumentAny::OptionContract(option) = instrument else {
            panic!("Expected OptionContract, got {instrument:?}");
        };

        assert_eq!(
            option.id(),
            InstrumentId::from("BTC-27DEC24-100000-C.DERIBIT")
        );
        assert_eq!(option.raw_symbol(), Symbol::from("BTC-27DEC24-100000-C"));
        assert_eq!(option.underlying(), Some("BTC".into()));
        assert_eq!(option.asset_class(), AssetClass::Cryptocurrency);
        assert_eq!(option.option_kind(), Some(OptionKind::Call));
        assert_eq!(option.strike_price(), Some(Price::from("100000")));
        assert_eq!(option.currency.code, "BTC");
        assert_eq!(
            option.activation_ns(),
            Some(UnixNanos::from(1719561600000_u64 * 1_000_000))
        );
        assert_eq!(
            option.expiration_ns(),
            Some(UnixNanos::from(1735300800000_u64 * 1_000_000))
        );
        assert_eq!(option.price_precision(), 4);
        assert_eq!(option.price_increment(), Price::from("0.0005"));
        assert_eq!(option.multiplier(), Quantity::from("1"));
        assert_eq!(option.lot_size(), Some(Quantity::from("0.1")));
        assert_eq!(option.maker_fee, dec!(0.0003));
        assert_eq!(option.taker_fee, dec!(0.0003));
    }

    #[rstest]
    fn test_parse_account_state_with_positions() {
        let json_data = load_test_json("http_get_account_summaries.json");
        let response: DeribitJsonRpcResponse<DeribitAccountSummariesResponse> =
            serde_json::from_str(&json_data).unwrap();
        let result = response.result.expect("Test data must have result");

        let account_id = AccountId::from("DERIBIT-001");

        // Extract server timestamp from response
        let ts_event =
            extract_server_timestamp(response.us_out).expect("Test data must have us_out");
        let ts_init = UnixNanos::default();

        let account_state = parse_account_state(&result.summaries, account_id, ts_init, ts_event)
            .expect("Should parse account state");

        // Verify we got 2 currencies (BTC and ETH)
        assert_eq!(account_state.balances.len(), 2);

        // Test BTC balance (has open positions with unrealized PnL)
        let btc_balance = account_state
            .balances
            .iter()
            .find(|b| b.currency.code == "BTC")
            .expect("BTC balance should exist");

        // From test data:
        // balance: 302.60065765, equity: 302.61869214, available_funds: 301.38059622
        // initial_margin: 1.24669592, session_upl: 0.05271555
        //
        // Using equity (correct):
        // total = equity = 302.61869214
        // free = available_funds = 301.38059622
        // locked = total - free = 302.61869214 - 301.38059622 = 1.23809592
        //
        // This is close to initial_margin (1.24669592), small difference due to other factors
        assert_eq!(btc_balance.total.as_f64(), 302.61869214);
        assert_eq!(btc_balance.free.as_f64(), 301.38059622);

        // Verify locked is positive and close to initial_margin
        let locked = btc_balance.locked.as_f64();
        assert!(
            locked > 0.0,
            "Locked should be positive when positions exist"
        );
        assert!(
            (locked - 1.24669592).abs() < 0.01,
            "Locked ({locked}) should be close to initial_margin (1.24669592)"
        );

        // Test ETH balance (no positions)
        let eth_balance = account_state
            .balances
            .iter()
            .find(|b| b.currency.code == "ETH")
            .expect("ETH balance should exist");

        // From test data: balance: 100, equity: 100, available_funds: 99.999598
        // total = equity = 100
        // free = available_funds = 99.999598
        // locked = 100 - 99.999598 = 0.000402 (matches initial_margin)
        assert_eq!(eth_balance.total.as_f64(), 100.0);
        assert_eq!(eth_balance.free.as_f64(), 99.999598);
        assert_eq!(eth_balance.locked.as_f64(), 0.000402);

        // Verify account metadata
        assert_eq!(account_state.account_id, account_id);
        assert_eq!(account_state.account_type, AccountType::Margin);
        assert!(account_state.is_reported);

        // Verify ts_event matches server timestamp (us_out = 1687352432005000 microseconds)
        let expected_ts_event = UnixNanos::from(1687352432005000_u64 * NANOSECONDS_IN_MICROSECOND);
        assert_eq!(
            account_state.ts_event, expected_ts_event,
            "ts_event should match server timestamp from response"
        );
    }

    #[rstest]
    fn test_parse_trade_tick_sell() {
        let json_data = load_test_json("http_get_last_trades.json");
        let response: DeribitJsonRpcResponse<DeribitTradesResponse> =
            serde_json::from_str(&json_data).unwrap();
        let result = response.result.expect("Test data must have result");

        assert!(result.has_more, "has_more should be true");
        assert_eq!(result.trades.len(), 10, "Should have 10 trades");

        let raw_trade = &result.trades[0];
        let instrument_id = InstrumentId::from("ETH-PERPETUAL.DERIBIT");
        let ts_init = UnixNanos::from(1766335632425576_u64 * 1000); // from usOut

        let trade = parse_trade_tick(raw_trade, instrument_id, 1, 0, ts_init)
            .expect("Should parse trade tick");

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("2968.3"));
        assert_eq!(trade.size, Quantity::from("1"));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id, TradeId::new("ETH-284830839"));
        // timestamp 1766332040636 ms -> ns
        assert_eq!(
            trade.ts_event,
            UnixNanos::from(1766332040636_u64 * 1_000_000)
        );
        assert_eq!(trade.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_trade_tick_buy() {
        let json_data = load_test_json("http_get_last_trades.json");
        let response: DeribitJsonRpcResponse<DeribitTradesResponse> =
            serde_json::from_str(&json_data).unwrap();
        let result = response.result.expect("Test data must have result");

        // Last trade is a buy with amount 106
        let raw_trade = &result.trades[9];
        let instrument_id = InstrumentId::from("ETH-PERPETUAL.DERIBIT");
        let ts_init = UnixNanos::default();

        let trade = parse_trade_tick(raw_trade, instrument_id, 1, 0, ts_init)
            .expect("Should parse trade tick");

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("2968.3"));
        assert_eq!(trade.size, Quantity::from("106"));
        assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
        assert_eq!(trade.trade_id, TradeId::new("ETH-284830854"));
    }
}
