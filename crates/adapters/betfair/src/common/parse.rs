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

//! Parsing utilities that convert Betfair payloads into Nautilus domain models.

use anyhow::Context;
use chrono::DateTime;
use nautilus_core::{UUID4, UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    enums::AccountType,
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::{BettingInstrument, InstrumentAny},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use ustr::Ustr;

use super::{
    consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION, BETFAIR_VENUE},
    types::SelectionId,
};
use crate::{
    http::models::{AccountFundsResponse, MarketCatalogue},
    stream::messages::MarketDefinition,
};

/// Constructs a Nautilus [`Symbol`] from Betfair market and selection identifiers.
///
/// Format: `"{market_id}-{selection_id}"` or `"{market_id}-{selection_id}-{handicap}"`
/// when handicap is non-zero.
#[must_use]
pub fn make_symbol(market_id: &str, selection_id: u64, handicap: Decimal) -> Symbol {
    if handicap == Decimal::ZERO {
        Symbol::new(format!("{market_id}-{selection_id}"))
    } else {
        Symbol::new(format!("{market_id}-{selection_id}-{handicap}"))
    }
}

/// Constructs a Nautilus [`InstrumentId`] from Betfair market and selection identifiers.
///
/// Format: `"{market_id}-{selection_id}.BETFAIR"` or
/// `"{market_id}-{selection_id}-{handicap}.BETFAIR"` when handicap is non-zero.
#[must_use]
pub fn make_instrument_id(market_id: &str, selection_id: u64, handicap: Decimal) -> InstrumentId {
    let symbol = make_symbol(market_id, selection_id, handicap);
    InstrumentId::new(symbol, *BETFAIR_VENUE)
}

/// Parses an RFC 3339 / ISO 8601 timestamp string into [`UnixNanos`].
///
/// Handles both UTC (`"2023-11-27T05:43:00Z"`) and offset
/// (`"2021-03-19T12:07:00+10:00"`) formats.
///
/// # Errors
///
/// Returns an error if the string is not a valid RFC 3339 datetime.
///
/// # Panics
///
/// Panics if the parsed datetime cannot be represented as nanoseconds.
pub fn parse_betfair_timestamp(s: &str) -> anyhow::Result<UnixNanos> {
    let dt = DateTime::parse_from_rfc3339(s)
        .or_else(|_| {
            // Betfair sometimes uses ".000Z" millis suffix
            DateTime::parse_from_rfc3339(&s.replace(".000Z", "Z"))
        })
        .with_context(|| format!("invalid Betfair timestamp: {s}"))?;
    Ok(UnixNanos::from(dt.timestamp_nanos_opt().unwrap() as u64))
}

/// Converts a millisecond epoch timestamp (as used in stream `pt` field) into [`UnixNanos`].
#[must_use]
pub fn parse_millis_timestamp(timestamp_ms: u64) -> UnixNanos {
    UnixNanos::from(timestamp_ms * NANOSECONDS_IN_MILLISECOND)
}

/// Truncates a client order ID to a Betfair `customer_order_ref`.
///
/// Takes the last 32 characters to preserve the high-entropy UUID suffix.
/// Returns the full string if it is already 32 characters or shorter.
#[must_use]
pub fn make_customer_order_ref(client_order_id: &str) -> String {
    let len = client_order_id.len();
    if len <= super::consts::BETFAIR_CUSTOMER_ORDER_REF_MAX_LEN {
        client_order_id.to_string()
    } else {
        client_order_id[len - super::consts::BETFAIR_CUSTOMER_ORDER_REF_MAX_LEN..].to_string()
    }
}

/// Parses a Betfair [`MarketCatalogue`] into a vec of [`InstrumentAny`].
///
/// Each runner in the catalogue becomes a separate [`BettingInstrument`].
///
/// # Errors
///
/// Returns an error if required fields are missing or instrument construction fails.
pub fn parse_market_catalogue(
    catalogue: &MarketCatalogue,
    currency: Currency,
    ts_init: UnixNanos,
    min_notional: Option<Money>,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let runners = catalogue
        .runners
        .as_ref()
        .context("MarketCatalogue missing runners")?;

    let market_id = &catalogue.market_id;

    let (event_type_id, event_type_name) = match &catalogue.event_type {
        Some(et) => (
            et.id
                .as_ref()
                .and_then(|id| id.parse::<u64>().ok())
                .unwrap_or(0),
            Ustr::from(et.name.as_deref().unwrap_or("")),
        ),
        None => (0, Ustr::from("")),
    };

    let (competition_id, competition_name) = match &catalogue.competition {
        Some(c) => (
            c.id.as_ref()
                .and_then(|id| id.parse::<u64>().ok())
                .unwrap_or(0),
            Ustr::from(c.name.as_deref().unwrap_or("")),
        ),
        None => (0, Ustr::from("")),
    };

    let (event_id, event_name, event_country_code, event_open_date) = match &catalogue.event {
        Some(e) => {
            let eid =
                e.id.as_ref()
                    .and_then(|id| id.parse::<u64>().ok())
                    .unwrap_or(0);
            let ename = Ustr::from(e.name.as_deref().unwrap_or(""));
            let cc = e.country_code.unwrap_or_else(|| Ustr::from(""));
            let open_date = e
                .open_date
                .as_deref()
                .and_then(|d| parse_betfair_timestamp(d).ok())
                .unwrap_or_default();
            (eid, ename, cc, open_date)
        }
        None => (0, Ustr::from(""), Ustr::from(""), UnixNanos::default()),
    };

    let (betting_type, market_type, market_base_rate) = match &catalogue.description {
        Some(desc) => (
            Ustr::from(&format!("{}", desc.betting_type)),
            desc.market_type,
            desc.market_base_rate,
        ),
        None => (Ustr::from("ODDS"), Ustr::from("WIN"), Decimal::ZERO),
    };

    let market_name = Ustr::from(&catalogue.market_name);
    let market_start_time = catalogue
        .market_start_time
        .as_deref()
        .and_then(|t| parse_betfair_timestamp(t).ok())
        .unwrap_or_default();

    // Convert market base rate from percentage to decimal fraction
    let fee_rate = market_base_rate / Decimal::ONE_HUNDRED;

    let tick = Decimal::new(1, 2); // 0.01
    let price_increment = Price::from_decimal_dp(tick, BETFAIR_PRICE_PRECISION)?;
    let size_increment = Quantity::from_decimal_dp(tick, BETFAIR_QUANTITY_PRECISION)?;

    let mut instruments = Vec::with_capacity(runners.len());

    for runner in runners {
        let handicap = runner.handicap;
        let instrument_id = make_instrument_id(market_id, runner.selection_id, handicap);
        let raw_symbol = make_symbol(market_id, runner.selection_id, handicap);

        let instrument = BettingInstrument::new_checked(
            instrument_id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            Ustr::from(market_id.as_str()),
            market_name,
            market_type,
            market_start_time,
            runner.selection_id,
            Ustr::from(&runner.runner_name),
            handicap.to_f64().unwrap_or(0.0),
            currency,
            BETFAIR_PRICE_PRECISION,
            BETFAIR_QUANTITY_PRECISION,
            price_increment,
            size_increment,
            None,               // max_quantity
            None,               // min_quantity
            None,               // max_notional
            min_notional,       // min_notional
            None,               // max_price
            None,               // min_price
            Some(Decimal::ONE), // margin_init (pre-funded)
            Some(Decimal::ONE), // margin_maint
            Some(fee_rate),     // maker_fee
            Some(fee_rate),     // taker_fee
            None,               // info
            ts_init,            // ts_event
            ts_init,            // ts_init
        )
        .with_context(|| {
            format!(
                "failed to create BettingInstrument for {market_id}/{}/{}",
                runner.selection_id, runner.runner_name
            )
        })?;

        instruments.push(InstrumentAny::Betting(instrument));
    }

    Ok(instruments)
}

/// Parses a stream [`MarketDefinition`] into a vec of [`InstrumentAny`].
///
/// Each runner definition becomes a separate [`BettingInstrument`].
/// Stream definitions have many optional fields — missing values are
/// defaulted gracefully.
///
/// # Errors
///
/// Returns an error if runners are missing or instrument construction fails.
pub fn parse_market_definition(
    market_id: &str,
    def: &MarketDefinition,
    currency: Currency,
    ts_init: UnixNanos,
    min_notional: Option<Money>,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let runners = def
        .runners
        .as_ref()
        .context("MarketDefinition missing runners")?;

    let event_type_id = def
        .event_type_id
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .unwrap_or(0);
    let event_type_name = def.event_type_name.unwrap_or_else(|| Ustr::from(""));

    let competition_id = def
        .competition_id
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .unwrap_or(0);
    let competition_name = Ustr::from(def.competition_name.as_deref().unwrap_or(""));

    let event_id = def
        .event_id
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .unwrap_or(0);
    let event_name = Ustr::from(def.event_name.as_deref().unwrap_or(""));
    let event_country_code = def.country_code.unwrap_or_else(|| Ustr::from(""));
    let event_open_date = def
        .open_date
        .as_deref()
        .and_then(|d| parse_betfair_timestamp(d).ok())
        .unwrap_or_default();

    let betting_type = match &def.betting_type {
        Some(bt) => Ustr::from(&format!("{bt}")),
        None => Ustr::from("ODDS"),
    };
    let market_name = Ustr::from(def.market_name.as_deref().unwrap_or(""));
    let market_type = def.market_type.unwrap_or_else(|| Ustr::from("WIN"));
    let market_start_time = def
        .market_time
        .as_deref()
        .and_then(|t| parse_betfair_timestamp(t).ok())
        .unwrap_or_default();

    let fee_rate = def
        .market_base_rate
        .map(|r| r / Decimal::ONE_HUNDRED)
        .unwrap_or_default();

    let tick = Decimal::new(1, 2); // 0.01
    let price_increment = Price::from_decimal_dp(tick, BETFAIR_PRICE_PRECISION)?;
    let size_increment = Quantity::from_decimal_dp(tick, BETFAIR_QUANTITY_PRECISION)?;

    let market_id_ustr = Ustr::from(market_id);

    let mut instruments = Vec::with_capacity(runners.len());

    for runner in runners {
        let handicap = runner.hc.unwrap_or(Decimal::ZERO);

        let instrument_id = make_instrument_id(market_id, runner.id, handicap);
        let raw_symbol = make_symbol(market_id, runner.id, handicap);
        let runner_name = Ustr::from(runner.name.as_deref().unwrap_or(""));

        let instrument = BettingInstrument::new_checked(
            instrument_id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id_ustr,
            market_name,
            market_type,
            market_start_time,
            runner.id,
            runner_name,
            handicap.to_f64().unwrap_or(0.0),
            currency,
            BETFAIR_PRICE_PRECISION,
            BETFAIR_QUANTITY_PRECISION,
            price_increment,
            size_increment,
            None,               // max_quantity
            None,               // min_quantity
            None,               // max_notional
            min_notional,       // min_notional
            None,               // max_price
            None,               // min_price
            Some(Decimal::ONE), // margin_init
            Some(Decimal::ONE), // margin_maint
            Some(fee_rate),     // maker_fee
            Some(fee_rate),     // taker_fee
            None,               // info
            ts_init,            // ts_event
            ts_init,            // ts_init
        )
        .with_context(|| {
            format!(
                "failed to create BettingInstrument for {market_id}/{}",
                runner.id
            )
        })?;

        instruments.push(InstrumentAny::Betting(instrument));
    }

    Ok(instruments)
}

/// Parses a Betfair [`AccountFundsResponse`] into a Nautilus [`AccountState`].
///
/// # Errors
///
/// Returns an error if monetary values cannot be converted.
pub fn parse_account_state(
    funds: &AccountFundsResponse,
    account_id: AccountId,
    currency: Currency,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let available = funds.available_to_bet_balance.unwrap_or_default();
    let exposure = funds.exposure.unwrap_or_default().abs();
    let total = available + exposure;

    let total_money = Money::from_decimal(total, currency)?;
    let locked_money = Money::from_decimal(exposure, currency)?;
    let free_money = Money::from_decimal(available, currency)?;

    let balance = AccountBalance::new(total_money, locked_money, free_money);

    Ok(AccountState::new(
        account_id,
        AccountType::Betting,
        vec![balance],
        vec![],
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        Some(currency),
    ))
}

/// Extracts the Betfair market ID from a Nautilus instrument ID.
///
/// Instrument IDs follow the format `{market_id}-{selection_id}.BETFAIR`
/// or `{market_id}-{selection_id}-{handicap}.BETFAIR`.
///
/// # Errors
///
/// Returns an error if the symbol does not contain a hyphen separator.
pub fn extract_market_id(instrument_id: &InstrumentId) -> anyhow::Result<String> {
    let symbol = instrument_id.symbol.as_str();
    let parts: Vec<&str> = symbol.splitn(3, '-').collect();
    if parts.len() >= 2 {
        Ok(parts[0].to_string())
    } else {
        anyhow::bail!("Cannot extract market ID from {instrument_id}")
    }
}

/// Extracts the selection ID and handicap from a Nautilus instrument ID.
///
/// # Errors
///
/// Returns an error if the symbol cannot be parsed into the expected format.
pub fn extract_selection_id(
    instrument_id: &InstrumentId,
) -> anyhow::Result<(SelectionId, Decimal)> {
    let symbol = instrument_id.symbol.as_str();
    let parts: Vec<&str> = symbol.splitn(3, '-').collect();
    if parts.len() < 2 {
        anyhow::bail!("Cannot extract selection ID from {instrument_id}");
    }

    let selection_id: SelectionId = parts[1]
        .parse()
        .with_context(|| format!("invalid selection ID in {instrument_id}"))?;

    let handicap = if parts.len() == 3 {
        parts[2]
            .parse::<Decimal>()
            .with_context(|| format!("invalid handicap in {instrument_id}"))?
    } else {
        Decimal::ZERO
    };

    Ok((selection_id, handicap))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{common::testing::load_test_json, stream::messages::StreamMessage};

    #[rstest]
    fn test_make_instrument_id_no_handicap() {
        let id = make_instrument_id("1.180737206", 19248890, Decimal::ZERO);
        assert_eq!(id.to_string(), "1.180737206-19248890.BETFAIR");
    }

    #[rstest]
    fn test_make_instrument_id_with_handicap() {
        let id = make_instrument_id("1.180737206", 19248890, Decimal::new(15, 1));
        assert_eq!(id.to_string(), "1.180737206-19248890-1.5.BETFAIR");
    }

    #[rstest]
    fn test_make_symbol_no_handicap() {
        let sym = make_symbol("1.180737206", 19248890, Decimal::ZERO);
        assert_eq!(sym.to_string(), "1.180737206-19248890");
    }

    #[rstest]
    fn test_make_symbol_with_handicap() {
        let sym = make_symbol("1.180737206", 19248890, Decimal::new(-5, 1));
        assert_eq!(sym.to_string(), "1.180737206-19248890--0.5");
    }

    #[rstest]
    fn test_parse_betfair_timestamp_utc() {
        let ts = parse_betfair_timestamp("2023-11-27T05:43:00Z").unwrap();
        assert!(ts.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_betfair_timestamp_with_offset() {
        let ts = parse_betfair_timestamp("2021-03-19T12:07:00+10:00").unwrap();
        assert!(ts.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_betfair_timestamp_with_millis() {
        let ts = parse_betfair_timestamp("2021-03-19T08:50:00.000Z").unwrap();
        assert!(ts.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_millis_timestamp() {
        let ts = parse_millis_timestamp(1_471_370_159_007);
        assert_eq!(ts.as_u64(), 1_471_370_159_007 * 1_000_000);
    }

    #[rstest]
    fn test_parse_market_catalogue() {
        let data = load_test_json("rest/list_market_catalogue.json");
        let catalogue: MarketCatalogue = serde_json::from_str(&data).unwrap();
        let instruments =
            parse_market_catalogue(&catalogue, Currency::GBP(), UnixNanos::default(), None)
                .unwrap();

        assert_eq!(instruments.len(), 3);

        // Verify first instrument
        if let InstrumentAny::Betting(inst) = &instruments[0] {
            assert_eq!(inst.market_id.as_str(), "1.221718403");
            assert_eq!(inst.selection_id, 20075720);
            assert_eq!(inst.selection_name.as_str(), "1. Searover");
            assert_eq!(inst.event_type_name.as_str(), "Horse Racing");
            assert_eq!(inst.event_name.as_str(), "Globe Derby (AUS) 27th Nov");
            assert_eq!(inst.event_country_code.as_str(), "AU");
            assert_eq!(inst.market_type.as_str(), "WIN");
            assert_eq!(inst.betting_type.as_str(), "ODDS");
            assert_eq!(inst.price_precision, 2);
            assert_eq!(inst.size_precision, 2);
            assert_eq!(inst.currency, Currency::GBP());
        } else {
            panic!("expected BettingInstrument");
        }
    }

    #[rstest]
    fn test_parse_market_catalogue_batch() {
        let data = load_test_json("rest/betting_list_market_catalogue.json");
        let catalogues: Vec<MarketCatalogue> = serde_json::from_str(&data).unwrap();

        let mut total = 0;
        for cat in &catalogues {
            let instruments =
                parse_market_catalogue(cat, Currency::GBP(), UnixNanos::default(), None).unwrap();
            total += instruments.len();
        }
        assert!(total > 0);
    }

    #[rstest]
    fn test_parse_market_definition_from_stream() {
        let data = load_test_json("stream/mcm_SUB_IMAGE.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().expect("market changes");
            let change = &mc[0];
            let def = change
                .market_definition
                .as_ref()
                .expect("market definition");

            let instruments = parse_market_definition(
                &change.id,
                def,
                Currency::GBP(),
                parse_millis_timestamp(mcm.pt),
                None,
            )
            .unwrap();

            assert_eq!(instruments.len(), 7);

            if let InstrumentAny::Betting(inst) = &instruments[0] {
                assert_eq!(inst.market_id.as_str(), "1.180737206");
                assert_eq!(inst.market_type.as_str(), "WIN");
            } else {
                panic!("expected BettingInstrument");
            }
        } else {
            panic!("expected MarketChange message");
        }
    }

    #[rstest]
    fn test_parse_account_state() {
        let data = load_test_json("rest/account_funds_with_exposure.json");
        let funds: AccountFundsResponse = serde_json::from_str(&data).unwrap();

        let state = parse_account_state(
            &funds,
            AccountId::from("BETFAIR-001"),
            Currency::GBP(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(state.account_type, AccountType::Betting);
        assert_eq!(state.balances.len(), 1);
        assert!(state.is_reported);
        assert_eq!(state.base_currency, Some(Currency::GBP()));
    }

    #[rstest]
    fn test_extract_market_id_no_handicap() {
        let instrument_id = make_instrument_id("1.180737206", 19248890, Decimal::ZERO);
        let market_id = extract_market_id(&instrument_id).unwrap();
        assert_eq!(market_id, "1.180737206");
    }

    #[rstest]
    fn test_extract_market_id_with_handicap() {
        let instrument_id = make_instrument_id("1.180737206", 19248890, Decimal::new(15, 1));
        let market_id = extract_market_id(&instrument_id).unwrap();
        assert_eq!(market_id, "1.180737206");
    }

    #[rstest]
    fn test_extract_selection_id_no_handicap() {
        let instrument_id = make_instrument_id("1.180737206", 19248890, Decimal::ZERO);
        let (selection_id, handicap) = extract_selection_id(&instrument_id).unwrap();
        assert_eq!(selection_id, 19248890);
        assert_eq!(handicap, Decimal::ZERO);
    }

    #[rstest]
    fn test_extract_selection_id_with_handicap() {
        let instrument_id = make_instrument_id("1.180737206", 19248890, Decimal::new(15, 1));
        let (selection_id, handicap) = extract_selection_id(&instrument_id).unwrap();
        assert_eq!(selection_id, 19248890);
        assert_eq!(handicap, Decimal::new(15, 1));
    }

    #[rstest]
    fn test_make_customer_order_ref_short_id() {
        let result = make_customer_order_ref("O-20240101-001");
        assert_eq!(result, "O-20240101-001");
    }

    #[rstest]
    fn test_make_customer_order_ref_exactly_32_chars() {
        let id = "12345678901234567890123456789012";
        assert_eq!(id.len(), 32);
        let result = make_customer_order_ref(id);
        assert_eq!(result, id);
    }

    #[rstest]
    fn test_make_customer_order_ref_truncates_to_last_32() {
        // UUID-style ID longer than 32 chars
        let id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
        assert!(id.len() > 32);
        let result = make_customer_order_ref(id);
        assert_eq!(result.len(), 32);
        // Should keep the last 32 characters (high-entropy UUID tail)
        assert_eq!(result, &id[id.len() - 32..]);
    }
}
