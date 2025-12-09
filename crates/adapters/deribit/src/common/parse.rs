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
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::{AssetClass, CurrencyType, OptionKind},
    identifiers::{InstrumentId, Symbol},
    instruments::{
        CryptoFuture, CryptoPerpetual, CurrencyPair, OptionContract, any::InstrumentAny,
    },
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::consts::DERIBIT_VENUE,
    http::models::{DeribitInstrument, DeribitInstrumentKind, DeribitOptionType},
};

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
) -> anyhow::Result<Option<InstrumentAny>> {
    match instrument.kind {
        DeribitInstrumentKind::Spot => parse_spot_instrument(instrument, ts_init).map(Some),
        DeribitInstrumentKind::Future => {
            // Check if it's a perpetual
            if instrument.instrument_name.as_str().contains("PERPETUAL") {
                parse_perpetual_instrument(instrument, ts_init).map(Some)
            } else {
                parse_future_instrument(instrument, ts_init).map(Some)
            }
        }
        DeribitInstrumentKind::Option => parse_option_instrument(instrument, ts_init).map(Some),
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
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event
        ts_init, // ts_init
    );

    Ok(InstrumentAny::CurrencyPair(currency_pair))
}

/// Parses a perpetual swap instrument into a [`CryptoPerpetual`].
fn parse_perpetual_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
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
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event
        ts_init, // ts_init
    );

    Ok(InstrumentAny::CryptoPerpetual(perpetual))
}

/// Parses a futures instrument into a [`CryptoFuture`].
fn parse_future_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
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
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event
        ts_init, // ts_init
    );

    Ok(InstrumentAny::CryptoFuture(future))
}

/// Parses an options instrument into an [`OptionContract`].
fn parse_option_instrument(
    instrument: &DeribitInstrument,
    ts_init: UnixNanos,
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
        None, // min_quantity
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        Some(maker_fee),
        Some(taker_fee),
        ts_init, // ts_event
        ts_init, // ts_init
    );

    Ok(InstrumentAny::OptionContract(option))
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{common::testing::load_test_json, http::models::DeribitJsonRpcResponse};

    #[rstest]
    fn test_parse_perpetual_instrument() {
        let json_data = load_test_json("http_get_instrument.json");
        let response: DeribitJsonRpcResponse<DeribitInstrument> =
            serde_json::from_str(&json_data).unwrap();
        let deribit_inst = response.result.expect("Test data must have result");

        let instrument_any =
            parse_deribit_instrument_any(&deribit_inst, UnixNanos::default()).unwrap();
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
        assert_eq!(perpetual.min_quantity(), None);
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
            parse_deribit_instrument_any(deribit_inst, UnixNanos::default()).unwrap();
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
            parse_deribit_instrument_any(deribit_inst, UnixNanos::default()).unwrap();
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
}
