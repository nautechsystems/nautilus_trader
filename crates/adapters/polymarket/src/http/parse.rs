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

//! Instrument parsing for Polymarket markets.

use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    enums::{AssetClass, CurrencyType},
    identifiers::{InstrumentId, Symbol},
    instruments::{BinaryOption, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::models::GammaMarket;
use crate::common::{
    consts::{MAX_PRICE, MIN_PRICE, POLYMARKET_VENUE, USDC},
    enums::PolymarketOutcome,
};

const DEFAULT_TICK_SIZE: &str = "0.001";

/// Normalized instrument definition for a single Polymarket outcome token.
///
/// Each Polymarket market produces two of these (Yes and No).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolymarketInstrumentDef {
    /// Nautilus symbol: `{conditionId}-{tokenId}`.
    pub symbol: Ustr,
    /// CLOB token ID (ERC1155 token, used for orders/subscriptions).
    pub token_id: Ustr,
    /// On-chain condition ID.
    pub condition_id: Ustr,
    /// Gamma market ID.
    pub market_id: String,
    /// Question ID (resolution hash).
    pub question_id: Option<String>,
    /// Outcome label.
    pub outcome: PolymarketOutcome,
    /// Market question/title.
    pub question: String,
    /// Market description.
    pub description: Option<String>,
    /// Price precision (decimal places).
    pub price_precision: u8,
    /// Minimum tick size.
    pub tick_size: Decimal,
    /// Minimum order size.
    pub min_size: Option<Decimal>,
    /// Maker fee (decimal, not bps).
    pub maker_fee: Option<Decimal>,
    /// Taker fee (decimal, not bps).
    pub taker_fee: Option<Decimal>,
    /// Market start timestamp (ISO 8601).
    pub start_date: Option<String>,
    /// Market end timestamp (ISO 8601).
    pub end_date: Option<String>,
    /// Whether the market is active and accepting orders.
    pub active: bool,
    /// URL slug for the market.
    pub market_slug: Option<String>,
}

/// Parses a Gamma market response into instrument definitions.
///
/// Each market produces two definitions: one for the Yes outcome
/// and one for the No outcome.
pub fn parse_gamma_market(market: &GammaMarket) -> anyhow::Result<Vec<PolymarketInstrumentDef>> {
    let token_ids: Vec<String> = serde_json::from_str(&market.clob_token_ids).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse clob_token_ids '{}': {e}",
            market.clob_token_ids
        )
    })?;

    if token_ids.len() != 2 {
        anyhow::bail!("Expected 2 token IDs, received {}", token_ids.len());
    }

    let outcomes: Vec<String> = serde_json::from_str(&market.outcomes)
        .map_err(|e| anyhow::anyhow!("Failed to parse outcomes '{}': {e}", market.outcomes))?;

    if outcomes.len() != 2 {
        anyhow::bail!("Expected 2 outcomes, received {}", outcomes.len());
    }

    let tick_size_str = market
        .order_price_min_tick_size
        .map_or_else(|| DEFAULT_TICK_SIZE.to_string(), |ts| ts.to_string());
    let tick_size: Decimal = tick_size_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse tick size '{tick_size_str}': {e}"))?;
    let price_precision = tick_size.scale() as u8;

    // Gamma API fee fields are unreliable, actual fees come from
    // the CLOB API at trade time (matches Python adapter behavior)
    let maker_fee: Option<Decimal> = None;
    let taker_fee: Option<Decimal> = None;

    let min_size: Option<Decimal> = market
        .order_min_size
        .map(|s| s.to_string().parse())
        .transpose()
        .map_err(|e| anyhow::anyhow!("Failed to parse min size: {e}"))?;

    let active = market.active.unwrap_or(false)
        && !market.closed.unwrap_or(false)
        && market.accepting_orders.unwrap_or(false);

    let mut defs = Vec::with_capacity(2);

    for (token_id, outcome_label) in token_ids.iter().zip(outcomes.iter()) {
        let outcome: PolymarketOutcome = outcome_label
            .parse()
            .map_err(|_| anyhow::anyhow!("Unknown outcome label '{outcome_label}'"))?;

        let symbol_str = format!("{}-{token_id}", market.condition_id);

        defs.push(PolymarketInstrumentDef {
            symbol: Ustr::from(&symbol_str),
            token_id: Ustr::from(token_id.as_str()),
            condition_id: Ustr::from(market.condition_id.as_str()),
            market_id: market.id.clone(),
            question_id: market.question_id.clone(),
            outcome,
            question: market.question.clone(),
            description: market.description.clone(),
            price_precision,
            tick_size,
            min_size,
            maker_fee,
            taker_fee,
            start_date: market.start_date.clone(),
            end_date: market.end_date.clone(),
            active,
            market_slug: market.market_slug.clone(),
        });
    }

    Ok(defs)
}

/// Converts a Polymarket instrument definition into a Nautilus `InstrumentAny`.
pub fn create_instrument_from_def(
    def: &PolymarketInstrumentDef,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol = Symbol::new(def.symbol);
    let venue = *POLYMARKET_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);
    let raw_symbol = Symbol::new(def.token_id);
    let currency = get_currency(USDC);

    let price_increment = Price::from(def.tick_size.to_string());
    let size_increment = Quantity::from("0.000001");

    let activation_ns = def
        .start_date
        .as_deref()
        .and_then(parse_datetime_to_nanos)
        .unwrap_or_default();
    let expiration_ns = def
        .end_date
        .as_deref()
        .and_then(parse_datetime_to_nanos)
        .unwrap_or_default();

    let max_price = Some(Price::from(MAX_PRICE));
    let min_price = Some(Price::from(MIN_PRICE));
    let min_quantity = def.min_size.map(|s| Quantity::from(s.to_string()));

    let outcome_str = match def.outcome {
        PolymarketOutcome::Yes => "Yes",
        PolymarketOutcome::No => "No",
    };

    let info: Params = serde_json::from_value(build_info_json(def))?;

    let binary_option = BinaryOption::new_checked(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        currency,
        activation_ns,
        expiration_ns,
        def.price_precision,
        6, // size_precision: USDC.e increments
        price_increment,
        size_increment,
        Some(Ustr::from(outcome_str)),
        Some(Ustr::from(def.question.as_str())),
        None, // max_quantity
        min_quantity,
        None, // max_notional
        None, // min_notional
        max_price,
        min_price,
        None, // margin_init
        None, // margin_maint
        def.maker_fee,
        def.taker_fee,
        Some(info),
        ts_init,
        ts_init,
    )?;

    Ok(InstrumentAny::BinaryOption(binary_option))
}

/// Converts a collection of definitions into Nautilus instruments.
#[must_use]
pub fn instruments_from_defs(
    defs: &[PolymarketInstrumentDef],
    ts_init: UnixNanos,
) -> Vec<InstrumentAny> {
    defs.iter()
        .filter_map(|def| {
            create_instrument_from_def(def, ts_init)
                .map_err(|e| log::warn!("Failed to create instrument {}: {e}", def.symbol))
                .ok()
        })
        .collect()
}

fn build_info_json(def: &PolymarketInstrumentDef) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "token_id".to_string(),
        serde_json::Value::String(def.token_id.to_string()),
    );
    map.insert(
        "condition_id".to_string(),
        serde_json::Value::String(def.condition_id.to_string()),
    );
    map.insert(
        "market_id".to_string(),
        serde_json::Value::String(def.market_id.clone()),
    );

    if let Some(qid) = &def.question_id {
        map.insert(
            "question_id".to_string(),
            serde_json::Value::String(qid.clone()),
        );
    }

    if let Some(slug) = &def.market_slug {
        map.insert(
            "market_slug".to_string(),
            serde_json::Value::String(slug.clone()),
        );
    }
    serde_json::Value::Object(map)
}

fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code).unwrap_or_else(|| {
        let currency = Currency::new(code, 6, 0, code, CurrencyType::Crypto);
        if let Err(e) = Currency::register(currency, false) {
            log::error!("Failed to register currency '{code}': {e}");
        }
        currency
    })
}

fn parse_datetime_to_nanos(s: &str) -> Option<UnixNanos> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .and_then(|dt| dt.timestamp_nanos_opt())
        .map(|ns| UnixNanos::from(ns as u64))
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::Instrument;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn load_gamma_market(filename: &str) -> GammaMarket {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    fn test_parse_gamma_market_produces_two_defs() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();

        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].outcome, PolymarketOutcome::Yes);
        assert_eq!(defs[1].outcome, PolymarketOutcome::No);
    }

    #[rstest]
    fn test_parse_gamma_market_fields() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        let yes_def = &defs[0];

        assert_eq!(yes_def.condition_id.as_str(), "0xabc123def456789");
        assert_eq!(yes_def.market_id, "123456");
        assert_eq!(yes_def.question_id.as_deref(), Some("0xquestion123"));
        assert_eq!(yes_def.question, "Will BTC exceed $100k by end of 2025?");
        assert_eq!(yes_def.tick_size, dec!(0.01));
        assert_eq!(yes_def.price_precision, 2);
        assert_eq!(yes_def.min_size, Some(dec!(5.0)));
        assert!(yes_def.maker_fee.is_none());
        assert!(yes_def.taker_fee.is_none());
        assert!(yes_def.active);
        assert_eq!(
            yes_def.market_slug.as_deref(),
            Some("will-btc-exceed-100k-by-end-of-2025")
        );
    }

    #[rstest]
    fn test_parse_gamma_market_symbol_format() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();

        assert_eq!(
            defs[0].symbol.as_str(),
            "0xabc123def456789-71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(
            defs[1].symbol.as_str(),
            "0xabc123def456789-52114319501245678901234567890123456789012345678901234567890123456789"
        );
    }

    #[rstest]
    fn test_parse_gamma_market_token_ids() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();

        assert_eq!(
            defs[0].token_id.as_str(),
            "71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(
            defs[1].token_id.as_str(),
            "52114319501245678901234567890123456789012345678901234567890123456789"
        );
    }

    #[rstest]
    fn test_parse_gamma_market_derives_outcome_from_label() {
        let mut market = load_gamma_market("gamma_market.json");

        // Reverse the outcomes order so No comes first
        market.outcomes = r#"["No", "Yes"]"#.to_string();

        let defs = parse_gamma_market(&market).unwrap();

        assert_eq!(defs[0].outcome, PolymarketOutcome::No);
        assert_eq!(defs[1].outcome, PolymarketOutcome::Yes);
    }

    #[rstest]
    fn test_parse_gamma_market_unknown_outcome_label_errors() {
        let mut market = load_gamma_market("gamma_market.json");
        market.outcomes = r#"["Maybe", "No"]"#.to_string();

        let result = parse_gamma_market(&market);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Maybe"),
            "Error should mention bad label: {err}"
        );
    }

    #[rstest]
    fn test_parse_gamma_market_null_tick_size_uses_default() {
        let mut market = load_gamma_market("gamma_market.json");
        market.order_price_min_tick_size = None;

        let defs = parse_gamma_market(&market).unwrap();

        assert_eq!(defs[0].tick_size, dec!(0.001));
        assert_eq!(defs[0].price_precision, 3);
    }

    #[rstest]
    fn test_parse_gamma_market_closed_is_inactive() {
        let mut market = load_gamma_market("gamma_market.json");
        market.closed = Some(true);

        let defs = parse_gamma_market(&market).unwrap();

        assert!(!defs[0].active);
        assert!(!defs[1].active);
    }

    #[rstest]
    fn test_create_instrument_from_def() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let instrument = create_instrument_from_def(&defs[0], ts_init).unwrap();

        let binary = match &instrument {
            InstrumentAny::BinaryOption(b) => b,
            other => panic!("Expected BinaryOption, was {other:?}"),
        };

        assert_eq!(
            binary.id.to_string(),
            "0xabc123def456789-71321045679252212594626385532706912750332728571942532289631379312455583992563.POLYMARKET"
        );
        assert_eq!(binary.outcome, Some(Ustr::from("Yes")));
        assert_eq!(binary.asset_class, AssetClass::Alternative);
        assert_eq!(binary.currency.code.as_str(), "USDC");
        assert_eq!(binary.price_precision, 2);
        assert_eq!(binary.size_precision, 6);
        assert_eq!(binary.price_increment(), Price::from("0.01"));
        assert_eq!(binary.size_increment(), Quantity::from("0.000001"));
    }

    #[rstest]
    fn test_create_instrument_info_params() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let instrument = create_instrument_from_def(&defs[0], ts_init).unwrap();

        let binary = match &instrument {
            InstrumentAny::BinaryOption(b) => b,
            other => panic!("Expected BinaryOption, was {other:?}"),
        };

        let info = binary.info.as_ref().expect("info should be Some");
        assert_eq!(
            info.get_str("token_id"),
            Some("71321045679252212594626385532706912750332728571942532289631379312455583992563")
        );
        assert_eq!(info.get_str("condition_id"), Some("0xabc123def456789"));
        assert_eq!(info.get_str("market_id"), Some("123456"));
        assert_eq!(info.get_str("question_id"), Some("0xquestion123"));
        assert_eq!(
            info.get_str("market_slug"),
            Some("will-btc-exceed-100k-by-end-of-2025")
        );
    }

    #[rstest]
    fn test_instruments_from_defs_batch() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let instruments = instruments_from_defs(&defs, ts_init);

        assert_eq!(instruments.len(), 2);
    }

    #[rstest]
    fn test_create_instrument_max_min_price() {
        let market = load_gamma_market("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let instrument = create_instrument_from_def(&defs[0], ts_init).unwrap();

        let binary = match &instrument {
            InstrumentAny::BinaryOption(b) => b,
            other => panic!("Expected BinaryOption, was {other:?}"),
        };

        assert_eq!(binary.max_price, Some(Price::from("0.999")));
        assert_eq!(binary.min_price, Some(Price::from("0.001")));
    }
}
