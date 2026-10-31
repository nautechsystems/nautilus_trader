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

//! Checked outcome definitions, using verified Polymarket conventions where FinFeed lacks fields.

use indexmap::IndexMap;
use jiff::Timestamp;
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    enums::AssetClass,
    identifiers::InstrumentId,
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    error::Error,
    models::{Market, MarketStatus, OutcomeType},
};
use crate::common::{consts::FINFEED, symbol::to_instrument_id};

pub(crate) const TOKEN_ID_KEY: &str = "token_id";
const PRICE_PRECISION: u8 = 4;
const SIZE_PRECISION: u8 = 6;

/// Parses a checked provider-owned Polymarket Yes/No definition.
///
/// Polymarket uses pUSD and six-decimal shares, matching the native adapter's conventions.
/// Unavailable activation uses its zero timestamp convention. `EndDateIso` sets the simulated
/// trading cutoff; it does not prove actual venue closure or authoritative settlement.
/// Other venue families fail until their required field rules are verified.
///
/// # Errors
///
/// Returns an error if:
/// - The venue or contract is unsupported.
/// - Required identity or metadata is missing, invalid, or conflicting.
/// - The tick is outside `(0, 1)` or the provider price is outside `[0, 1]`.
/// - A price requires rounding or expiry is outside the supported timestamp range.
pub fn parse_instrument(market: &Market, ts_init: UnixNanos) -> Result<BinaryOption, Error> {
    let exchange_id = market
        .exchange_id
        .as_deref()
        .ok_or_else(|| definition_error(market, "exchange_id", "is missing"))?;
    let market_id = market
        .market_id
        .as_deref()
        .ok_or_else(|| definition_error(market, "market_id", "is missing"))?;
    let instrument_id = to_instrument_id(exchange_id, market_id)?;

    if market
        .exchange
        .as_ref()
        .and_then(|exchange| exchange.exchange_id.as_deref())
        .is_some_and(|nested| nested != exchange_id)
    {
        return Err(definition_error(
            market,
            "exchange_id",
            "conflicts with nested exchange",
        ));
    }

    if market.outcome_type != OutcomeType::Binary {
        return Err(definition_error(market, "outcome_type", "is unsupported"));
    }

    if exchange_id != "POLYMARKET" {
        return Err(definition_error(
            market,
            "source_specific_data",
            "has no verified complete definition rules for this venue",
        ));
    }

    let instrument = parse_polymarket(market, instrument_id, ts_init)?;

    if market.status == MarketStatus::Unknown {
        log::warn!(
            "Unknown FinFeed market status for {instrument_id}, retaining metadata without settlement inference"
        );
    }

    Ok(instrument)
}

fn parse_polymarket(
    market: &Market,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> Result<BinaryOption, Error> {
    let title = market
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| definition_error(market, "title", "is missing or empty"))?;
    let outcome = market
        .outcome_name
        .as_deref()
        .filter(|outcome| matches!(*outcome, "Yes" | "No"))
        .ok_or_else(|| definition_error(market, "outcome_name", "must be Yes or No"))?;
    let suffix = if outcome == "Yes" { "_YES" } else { "_NO" };
    if !instrument_id.symbol.as_str().ends_with(suffix) {
        return Err(definition_error(
            market,
            "outcome_name",
            "conflicts with the provider outcome ID suffix",
        ));
    }
    let condition_id = source_string(market, "ConditionId")?;
    if condition_id.len() != 66
        || !condition_id.starts_with("0x")
        || !condition_id[2..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(definition_error(market, "ConditionId", "is invalid"));
    }

    let token_id = outcome_token(market, outcome)?;
    let tick_string = source_string(market, "MinimumTickSize")?;
    let tick = Decimal::from_str_exact(&tick_string)
        .map_err(|_| definition_error(market, "MinimumTickSize", "is not an exact decimal"))?;
    if tick <= Decimal::ZERO || tick >= Decimal::ONE {
        return Err(definition_error(
            market,
            "MinimumTickSize",
            "must be in (0, 1)",
        ));
    }
    let price_increment = exact_price(market, tick, "MinimumTickSize")?;
    if market.price < Decimal::ZERO || market.price > Decimal::ONE {
        return Err(definition_error(market, "price", "must be in [0, 1]"));
    }
    exact_price(market, market.price, "price")?;

    let end_date = source_string(market, "EndDateIso")?;
    let expiration = end_date
        .parse::<Timestamp>()
        .map_err(|_| definition_error(market, "EndDateIso", "is not a timestamp with an offset"))?;
    let expiration_ns = u64::try_from(expiration.as_nanosecond())
        .ok()
        .filter(|time| *time > 0)
        .ok_or_else(|| {
            definition_error(
                market,
                "EndDateIso",
                "is outside the supported positive nanosecond range",
            )
        })?;

    let currency = Currency::pUSD();
    let size_decimal = Decimal::new(1, u32::from(SIZE_PRECISION));
    let size_increment = Quantity::from_decimal_dp(size_decimal, SIZE_PRECISION)
        .map_err(|_| definition_error(market, "size_increment", "could not be represented"))?;
    let description = match &market.description {
        Some(description) => description.clone(),
        None if market
            .source_specific_data
            .as_ref()
            .is_some_and(|source| source.contains_key("Description")) =>
        {
            source_string(market, "Description")?
        }
        None => title.to_owned(),
    };

    let mut info: IndexMap<String, Value> = serde_json::from_value(json!({
        "provider": FINFEED,
        "exchange_id": market.exchange_id,
        "market_id": market.market_id,
        "title": title,
        "status": market.status,
        "outcome_type": market.outcome_type,
        "mechanism": market.mechanism,
        "condition_id": condition_id,
        TOKEN_ID_KEY: token_id,
        "activation_source": "native_unknown_zero",
        "expiration_source": "EndDateIso",
    }))
    .map_err(|_| definition_error(market, "info", "could not be constructed"))?;
    if market
        .source_specific_data
        .as_ref()
        .is_some_and(|source| source.contains_key("QuestionId"))
    {
        info.insert(
            "question_id".into(),
            source_string(market, "QuestionId")?.into(),
        );
    }

    if market
        .source_specific_data
        .as_ref()
        .is_some_and(|source| source.contains_key("Tags"))
    {
        let tags: Vec<String> = serde_json::from_str(&source_string(market, "Tags")?)
            .map_err(|_| definition_error(market, "Tags", "must contain a string array"))?;
        info.insert("tags".into(), json!(tags));
    }

    let info = Params::new(info);
    BinaryOption::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(AssetClass::Alternative)
        .currency(currency)
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::from(expiration_ns))
        .price_precision(PRICE_PRECISION)
        .size_precision(SIZE_PRECISION)
        .price_increment(price_increment)
        .size_increment(size_increment)
        .outcome(outcome.into())
        .description(description.as_str().into())
        .info(info)
        .ts_event(ts_init)
        .ts_init(ts_init)
        .build()
        .map_err(|_| definition_error(market, "instrument", "failed checked construction"))
}

#[derive(Deserialize)]
struct OutcomeToken {
    token_id: String,
    outcome: String,
}

fn outcome_token(market: &Market, outcome: &str) -> Result<String, Error> {
    let value = source_string(market, "Tokens")?;
    let mut tokens: Vec<OutcomeToken> = serde_json::from_str(&value)
        .map_err(|_| definition_error(market, "Tokens", "is not a valid token array"))?;
    if tokens.len() != 2 {
        return Err(definition_error(
            market,
            "Tokens",
            "must contain exactly two binary outcomes",
        ));
    }

    if tokens[0].token_id == tokens[1].token_id
        || tokens[0].outcome == tokens[1].outcome
        || tokens.iter().any(|token| {
            !matches!(token.outcome.as_str(), "Yes" | "No")
                || token.token_id.is_empty()
                || !token.token_id.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(definition_error(
            market,
            "Tokens",
            "contains ambiguous or invalid identities",
        ));
    }
    let index = usize::from(tokens[0].outcome != outcome);
    Ok(tokens.swap_remove(index).token_id)
}

fn source_string(market: &Market, field: &'static str) -> Result<String, Error> {
    let value = market
        .source_specific_data
        .as_ref()
        .and_then(|source| source.get(field))
        .ok_or_else(|| definition_error(market, field, "is missing"))?;
    let value: String = serde_json::from_str(value.get())
        .map_err(|_| definition_error(market, field, "must be a string"))?;
    if value.trim().is_empty() {
        return Err(definition_error(market, field, "is empty"));
    }
    Ok(value)
}

fn exact_price(market: &Market, value: Decimal, field: &'static str) -> Result<Price, Error> {
    let price = Price::from_decimal_dp(value, PRICE_PRECISION)
        .map_err(|_| definition_error(market, field, "cannot be represented at price precision"))?;
    if price.as_decimal() != value {
        return Err(definition_error(
            market,
            field,
            "would require rounding at price precision",
        ));
    }
    Ok(price)
}

fn definition_error(market: &Market, field: &'static str, reason: &'static str) -> Error {
    Error::Definition {
        exchange_id: market.exchange_id.clone().unwrap_or_default(),
        market_id: market.market_id.clone().unwrap_or_default(),
        field,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::CurrencyType;
    use rstest::rstest;

    use super::*;
    use crate::http::models::MarketsPage;

    const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
    const LISTINGS: &str = include_str!("../../test_data/polymarket-listings.json");

    #[rstest]
    fn binary_pair_preserves_every_instrument_field() {
        let page: MarketsPage = serde_json::from_str(LISTINGS).unwrap();
        let markets = page.data.unwrap();
        let instruments: Vec<_> = markets
            .iter()
            .map(|market| parse_instrument(market, UnixNanos::from(123)).unwrap())
            .collect();
        let ids: Vec<_> = instruments
            .iter()
            .map(|instrument| instrument.id.to_string())
            .collect();
        assert_eq!(
            ids,
            [
                "TEST-PAIR_NO.FINFEED_POLYMARKET",
                "TEST-PAIR_YES.FINFEED_POLYMARKET"
            ]
        );

        for (index, instrument) in instruments.iter().enumerate() {
            let token_id = [
                "404044040440404404044040440404404044040440404404044040440404404044040440404",
                "303033030330303303033030330303303033030330303303033030330303303033030330303",
            ][index];
            assert_definition_fields(instrument, 123, "0.0010", 1_780_358_400_000_000_000);
            assert_eq!(instrument.outcome.unwrap().as_str(), ["No", "Yes"][index]);
            assert_eq!(
                instrument.description.unwrap().as_str(),
                markets[index].description.as_deref().unwrap()
            );
            let expected_info = serde_json::from_value::<Params>(json!({
                "provider": "FINFEED", "exchange_id": "POLYMARKET", "market_id": markets[index].market_id,
                "title": "Authored binary outcome", "status": "Closed",
                "outcome_type": "Binary", "mechanism": markets[index].mechanism,
                "condition_id": "0x3333333333333333333333333333333333333333333333333333333333333333",
                "token_id": token_id,
                "activation_source": "native_unknown_zero", "expiration_source": "EndDateIso",
                "question_id": "0x4444444444444444444444444444444444444444444444444444444444444444",
                "tags": ["Test", "Binary"]
            })).unwrap();
            assert_eq!(instrument.info.as_ref().unwrap(), &expected_info);
        }
    }

    #[rstest]
    fn full_metadata_preserves_description_and_question_tags() {
        let market: Market = serde_json::from_str(MARKET).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::from(456)).unwrap();
        assert_definition_fields(&instrument, 456, "0.0100", 1_798_761_600_000_000_000);
        assert_eq!(
            instrument.description.unwrap().as_str(),
            market.description.unwrap()
        );
        assert_eq!(instrument.outcome.unwrap().as_str(), "No");
        let info = instrument.info.unwrap();
        assert_eq!(
            info["question_id"],
            "0x2222222222222222222222222222222222222222222222222222222222222222"
        );
        assert_eq!(info["tags"], json!(["Test", "Binary"]));
        let encoded = serde_json::to_string(&instrument.currency).unwrap();
        let decoded: Currency = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, instrument.currency);
        assert_eq!(decoded.precision, instrument.currency.precision);
        assert_eq!(decoded.iso4217, instrument.currency.iso4217);
        assert_eq!(decoded.name, instrument.currency.name);
        assert_eq!(decoded.currency_type, instrument.currency.currency_type);
    }

    #[rstest]
    #[case(include_str!("../../test_data/kalshi-listings.json"), "KALSHI")]
    #[case(include_str!("../../test_data/myriad-listings.json"), "MYRIAD")]
    #[case(include_str!("../../test_data/manifold-listings.json"), "MANIFOLD")]
    #[case(include_str!("../../test_data/hyperliquidoutcome-listings.json"), "HYPERLIQUIDOUTCOME")]
    #[case(include_str!("../../test_data/pascal-listings.json"), "PASCAL")]
    #[case(include_str!("../../test_data/geminipm-listings.json"), "GEMINIPM")]
    #[case(include_str!("../../test_data/cryptocompm-listings.json"), "CRYPTOCOMPM")]
    #[case(include_str!("../../test_data/forecastex-listings.json"), "FORECASTEX")]
    fn incomplete_venue_definitions_fail_with_identity_context(
        #[case] fixture: &str,
        #[case] exchange: &str,
    ) {
        let page: MarketsPage = serde_json::from_str(fixture).unwrap();
        for market in page.data.unwrap() {
            match parse_instrument(&market, UnixNanos::default()).unwrap_err() {
                Error::Definition {
                    exchange_id,
                    market_id,
                    field,
                    reason,
                } => {
                    assert_eq!(exchange_id, exchange);
                    assert_eq!(Some(market_id), market.market_id);
                    assert_eq!(field, "source_specific_data");
                    assert_eq!(
                        reason,
                        "has no verified complete definition rules for this venue"
                    );
                }
                e => panic!("Unexpected error: {e}"),
            }
        }
    }

    #[rstest]
    #[case("MinimumTickSize", json!("0"), "must be in (0, 1)")]
    #[case("MinimumTickSize", json!("1"), "must be in (0, 1)")]
    #[case("MinimumTickSize", json!("0.00001"), "would require rounding at price precision")]
    #[case("MinimumTickSize", json!("0.1234567890123456789012345678901"), "is not an exact decimal")]
    #[case("MinimumTickSize", json!(false), "must be a string")]
    #[case("MinimumTickSize", json!(""), "is empty")]
    #[case("EndDateIso", json!("2027-01-01"), "is not a timestamp with an offset")]
    #[case("EndDateIso", json!("1960-01-01T00:00:00Z"), "is outside the supported positive nanosecond range")]
    #[case("EndDateIso", json!("2600-01-01T00:00:00Z"), "is outside the supported positive nanosecond range")]
    #[case("ConditionId", json!("not-a-condition"), "is invalid")]
    #[case("Tokens", json!("[]"), "must contain exactly two binary outcomes")]
    #[case("Tokens", json!("bad-array"), "is not a valid token array")]
    #[case("Tags", json!("{}"), "must contain a string array")]
    fn invalid_source_fields_fail_exactly(
        #[case] field: &str,
        #[case] value: Value,
        #[case] reason: &str,
    ) {
        let mut value_market: Value = serde_json::from_str(MARKET).unwrap();
        value_market["source_specific_data"][field] = value;
        let market: Market = serde_json::from_value(value_market).unwrap();
        match parse_instrument(&market, UnixNanos::default()).unwrap_err() {
            Error::Definition {
                exchange_id,
                market_id,
                field: actual_field,
                reason: actual_reason,
            } => {
                assert_eq!(exchange_id, "POLYMARKET");
                assert_eq!(Some(market_id), market.market_id);
                assert_eq!(actual_field, field);
                assert_eq!(actual_reason, reason);
            }
            e => panic!("Unexpected error: {e}"),
        }
    }

    #[rstest]
    #[case("ConditionId")]
    #[case("Tokens")]
    #[case("MinimumTickSize")]
    #[case("EndDateIso")]
    fn missing_source_fields_are_not_defaulted(#[case] field: &str) {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.source_specific_data.as_mut().unwrap().remove(field);
        assert!(
            matches!(parse_instrument(&market, UnixNanos::default()), Err(Error::Definition { field: actual, reason: "is missing", .. }) if actual == field)
        );
    }

    #[rstest]
    #[case(OutcomeType::MultipleChoice)]
    #[case(OutcomeType::Numeric)]
    #[case(OutcomeType::Other)]
    fn non_binary_outcomes_are_rejected(#[case] outcome_type: OutcomeType) {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.outcome_type = outcome_type;
        assert!(matches!(
            parse_instrument(&market, UnixNanos::default()),
            Err(Error::Definition {
                exchange_id,
                market_id,
                field: "outcome_type",
                reason: "is unsupported",
            }) if exchange_id == "POLYMARKET" && Some(market_id.as_str()) == market.market_id.as_deref()
        ));
    }

    #[rstest]
    #[case("-0.0001")]
    #[case("1.0001")]
    #[case("0.12345")]
    fn prices_outside_probability_or_exact_precision_are_rejected(#[case] price: &str) {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.price = Decimal::from_str_exact(price).unwrap();
        assert!(matches!(
            parse_instrument(&market, UnixNanos::default()),
            Err(Error::Definition { field: "price", .. })
        ));
    }

    #[rstest]
    #[case("0")]
    #[case("1")]
    #[case("0.1234")]
    fn exact_probability_boundaries_are_supported(#[case] price: &str) {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.price = Decimal::from_str_exact(price).unwrap();
        let instrument = parse_instrument(&market, UnixNanos::from(1)).unwrap();
        assert_eq!(instrument.price_precision, 4);
        assert_eq!(
            instrument.price_increment.as_decimal(),
            Decimal::from_str_exact("0.01").unwrap()
        );
    }

    #[rstest]
    fn conflicting_exchange_or_outcome_cannot_change_identity() {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.exchange.as_mut().unwrap().exchange_id = Some("KALSHI".into());
        assert!(matches!(
            parse_instrument(&market, UnixNanos::default()),
            Err(Error::Definition {
                field: "exchange_id",
                ..
            })
        ));
        market.exchange.as_mut().unwrap().exchange_id = Some("POLYMARKET".into());
        market.outcome_name = Some("Yes".into());
        assert!(matches!(
            parse_instrument(&market, UnixNanos::default()),
            Err(Error::Definition {
                field: "outcome_name",
                reason: "conflicts with the provider outcome ID suffix",
                ..
            })
        ));
    }

    #[rstest]
    #[case("duplicate-token")]
    #[case("duplicate-outcome")]
    #[case("unsupported-outcome")]
    #[case("empty-token")]
    fn ambiguous_tokens_are_rejected(#[case] change: &str) {
        let mut value: Value = serde_json::from_str(MARKET).unwrap();
        let mut tokens: Value =
            serde_json::from_str(value["source_specific_data"]["Tokens"].as_str().unwrap())
                .unwrap();

        match change {
            "duplicate-token" => tokens[1]["token_id"] = tokens[0]["token_id"].clone(),
            "duplicate-outcome" => tokens[1]["outcome"] = tokens[0]["outcome"].clone(),
            "unsupported-outcome" => tokens[1]["outcome"] = "Maybe".into(),
            "empty-token" => tokens[1]["token_id"] = "".into(),
            _ => unreachable!(),
        }

        value["source_specific_data"]["Tokens"] = serde_json::to_string(&tokens).unwrap().into();
        let market: Market = serde_json::from_value(value).unwrap();
        assert!(matches!(
            parse_instrument(&market, UnixNanos::default()),
            Err(Error::Definition {
                field: "Tokens",
                reason: "contains ambiguous or invalid identities",
                ..
            })
        ));
    }

    #[rstest]
    #[case("exchange_id", "exchange_id")]
    #[case("market_id", "market_id")]
    #[case("title", "title")]
    #[case("outcome_name", "outcome_name")]
    #[case("source_specific_data", "ConditionId")]
    fn missing_normalized_fields_fail_with_exact_context(
        #[case] field: &str,
        #[case] error_field: &str,
    ) {
        let mut value: Value = serde_json::from_str(MARKET).unwrap();
        value[field] = Value::Null;
        let market: Market = serde_json::from_value(value).unwrap();
        match parse_instrument(&market, UnixNanos::default()).unwrap_err() {
            Error::Definition {
                exchange_id,
                market_id,
                field: actual_field,
                ..
            } => {
                assert_eq!(actual_field, error_field);
                assert_eq!(exchange_id, market.exchange_id.unwrap_or_default());
                assert_eq!(market_id, market.market_id.unwrap_or_default());
            }
            e => panic!("Unexpected error: {e}"),
        }
    }

    #[rstest]
    fn absent_optional_description_and_event_fields_do_not_invent_metadata() {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        market.description = None;
        let source = market.source_specific_data.as_mut().unwrap();
        source.remove("QuestionId");
        source.remove("Tags");
        source.remove("Description");
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        assert_eq!(
            instrument.description.unwrap().as_str(),
            market.title.unwrap()
        );
        assert_eq!(instrument.event_id, None);
        let info = instrument.info.unwrap();
        assert_eq!(info.get("question_id"), None);
        assert_eq!(info.get("tags"), None);
        assert_eq!(info.get_str("outcome_type"), Some("Binary"));
        assert_eq!(info.get("mechanism"), Some(&serde_json::Value::Null));
        assert_eq!(info.len(), 11);
    }

    #[rstest]
    fn source_description_fills_missing_normalized_description() {
        let mut market: Market = serde_json::from_str(MARKET).unwrap();
        let expected = market.description.take().unwrap();
        let instrument = parse_instrument(&market, UnixNanos::default()).unwrap();
        assert_eq!(instrument.description.unwrap().as_str(), expected);
    }

    fn assert_definition_fields(instrument: &BinaryOption, ts: u64, tick: &str, expiry: u64) {
        assert_eq!(instrument.raw_symbol, instrument.id.symbol);
        assert_eq!(instrument.asset_class, AssetClass::Alternative);
        assert_eq!(instrument.currency.code.as_str(), "pUSD");
        assert_eq!(instrument.currency.precision, 6);
        assert_eq!(instrument.currency.iso4217, 0);
        assert_eq!(instrument.currency.name.as_str(), "Polymarket USD");
        assert_eq!(instrument.currency.currency_type, CurrencyType::Crypto);
        assert_eq!(instrument.price_precision, 4);
        assert_eq!(instrument.size_precision, 6);
        assert_eq!(instrument.price_increment.precision, 4);
        assert_eq!(
            instrument.price_increment.as_decimal(),
            Decimal::from_str_exact(tick).unwrap()
        );
        assert_eq!(instrument.size_increment.precision, 6);
        assert_eq!(
            instrument.size_increment.as_decimal(),
            Decimal::from_str_exact("0.000001").unwrap()
        );
        assert_eq!(instrument.activation_ns, UnixNanos::default());
        assert_eq!(instrument.expiration_ns, UnixNanos::from(expiry));
        assert_eq!(instrument.margin_init, Decimal::ZERO);
        assert_eq!(instrument.margin_maint, Decimal::ZERO);
        assert_eq!(instrument.event_id, None);
        assert_eq!(instrument.min_quantity, None);
        assert_eq!(instrument.max_quantity, None);
        assert_eq!(instrument.min_notional, None);
        assert_eq!(instrument.max_notional, None);
        assert_eq!(instrument.min_price, None);
        assert_eq!(instrument.max_price, None);
        assert_eq!(instrument.tick_scheme, None);
        assert_eq!(instrument.ts_event, UnixNanos::from(ts));
        assert_eq!(instrument.ts_init, UnixNanos::from(ts));
    }
}
