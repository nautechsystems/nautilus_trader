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

use nautilus_core::Params;
use nautilus_model::identifiers::InstrumentId;

use crate::{
    common::consts::POLYMARKET_VENUE,
    http::models::{ClobMarketResponse, GammaMarket},
    providers::extract_condition_id,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StrictResolvedMarket {
    pub(crate) condition_id: String,
    pub(crate) winning_asset_id: String,
    pub(crate) winning_outcome: String,
}

fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .filter(|values| !values.is_empty())
}

fn parse_string_array_param(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::String(single) => {
            if single.is_empty() {
                return None;
            }
            Some(vec![single.clone()])
        }
        serde_json::Value::Array(items) => {
            let mut parsed = Vec::with_capacity(items.len());
            for item in items {
                let value = item.as_str()?;
                if value.is_empty() {
                    return None;
                }
                parsed.push(value.to_string());
            }
            (!parsed.is_empty()).then_some(parsed)
        }
        _ => None,
    }
}

fn parse_outcome_prices(raw: &Option<String>) -> Option<Vec<f64>> {
    let raw = raw.as_ref()?;

    if let Ok(values) = serde_json::from_str::<Vec<f64>>(raw)
        && !values.is_empty()
    {
        return Some(values);
    }

    let as_strings = serde_json::from_str::<Vec<String>>(raw).ok()?;
    let mut values = Vec::with_capacity(as_strings.len());
    for value in as_strings {
        values.push(value.parse::<f64>().ok()?);
    }
    (!values.is_empty()).then_some(values)
}

fn strict_winner_index(prices: &[f64]) -> Option<usize> {
    if prices.is_empty() {
        return None;
    }

    let mut winner_idx: Option<usize> = None;

    for (idx, value) in prices.iter().copied().enumerate() {
        if value >= 0.999 {
            if winner_idx.is_some() {
                return None;
            }
            winner_idx = Some(idx);
        } else if value > 0.001 {
            return None;
        }
    }

    winner_idx
}

pub(crate) fn build_strict_resolved_market(market: &GammaMarket) -> Option<StrictResolvedMarket> {
    if market.closed != Some(true) {
        return None;
    }

    let asset_ids = parse_json_string_array(&market.clob_token_ids)?;
    if asset_ids.len() != 2 {
        return None;
    }

    let outcomes = parse_json_string_array(&market.outcomes)?;
    if outcomes.len() != 2 {
        return None;
    }

    let prices = parse_outcome_prices(&market.outcome_prices)?;
    if prices.len() != 2 {
        return None;
    }
    let winner_idx = strict_winner_index(&prices)?;
    let winning_asset_id = asset_ids.get(winner_idx)?.clone();
    let winning_outcome = outcomes.get(winner_idx)?.clone();

    Some(StrictResolvedMarket {
        condition_id: market.condition_id.clone(),
        winning_asset_id,
        winning_outcome,
    })
}

pub(crate) fn build_resolved_market_from_clob_market(
    market: &ClobMarketResponse,
) -> Option<StrictResolvedMarket> {
    if !market.closed {
        return None;
    }

    if market.tokens.len() != 2 {
        return None;
    }

    let mut winner_idx: Option<usize> = None;

    for (idx, token) in market.tokens.iter().enumerate() {
        if token.winner {
            if winner_idx.is_some() {
                return None;
            }
            winner_idx = Some(idx);
        }
    }

    let winner_idx = winner_idx?;
    let winner = market.tokens.get(winner_idx)?;
    if winner.token_id.is_empty() || winner.outcome.is_empty() {
        return None;
    }

    Some(StrictResolvedMarket {
        condition_id: market.condition_id.clone(),
        winning_asset_id: winner.token_id.clone(),
        winning_outcome: winner.outcome.clone(),
    })
}

pub(crate) fn parse_condition_ids_from_request_params(params: &Option<Params>) -> Vec<String> {
    let Some(params) = params.as_ref() else {
        return Vec::new();
    };

    let mut condition_ids = Vec::new();

    if let Some(condition_id_value) = params.get("condition_id") {
        if let Some(condition_id) = condition_id_value.as_str() {
            condition_ids.push(condition_id.to_string());
        } else {
            log::warn!(
                "Ignoring invalid `condition_id` param: expected string, received {condition_id_value}"
            );
        }
    }

    if let Some(condition_ids_value) = params.get("condition_ids") {
        if let Some(values) = parse_string_array_param(condition_ids_value) {
            condition_ids.extend(values);
        } else {
            log::warn!(
                "Ignoring invalid `condition_ids` param: expected string or array[string], received {condition_ids_value}"
            );
        }
    }

    if let Some(instrument_ids_value) = params.get("instrument_ids") {
        if let Some(instrument_ids) = parse_string_array_param(instrument_ids_value) {
            for value in instrument_ids {
                if let Ok(instrument_id) = value.parse::<InstrumentId>() {
                    if instrument_id.venue != *POLYMARKET_VENUE {
                        log::warn!(
                            "Ignoring `instrument_ids` entry with non-Polymarket venue: {instrument_id}"
                        );
                        continue;
                    }

                    if let Ok(condition_id) = extract_condition_id(&instrument_id) {
                        condition_ids.push(condition_id);
                    } else {
                        log::warn!(
                            "Ignoring `instrument_ids` entry that cannot extract condition_id: {value}"
                        );
                    }
                } else {
                    log::warn!("Ignoring invalid `instrument_ids` entry: {value}");
                }
            }
        } else {
            log::warn!(
                "Ignoring invalid `instrument_ids` param: expected string or array[string], received {instrument_ids_value}"
            );
        }
    }

    condition_ids.sort();
    condition_ids.dedup();
    condition_ids
}

pub(crate) fn request_params_has_explicit_condition_selector(params: &Option<Params>) -> bool {
    let Some(params) = params.as_ref() else {
        return false;
    };

    params.contains_key("condition_id")
        || params.contains_key("condition_ids")
        || params.contains_key("instrument_ids")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use super::*;

    fn make_gamma_market_value_with_outcome_prices(
        condition_id: &str,
        clob_token_ids: &str,
        outcome_prices: Option<&str>,
        closed: Option<bool>,
        accepting_orders: Option<bool>,
    ) -> serde_json::Value {
        let mut value = serde_json::json!({
            "id": "1557558",
            "conditionId": condition_id,
            "questionID": "0xquestion",
            "clobTokenIds": clob_token_ids,
            "outcomes": "[\"Yes\",\"No\"]",
            "question": "Will test pass?",
            "description": null,
            "startDate": null,
            "endDate": null,
            "active": false,
            "closed": closed,
            "acceptingOrders": accepting_orders,
            "enableOrderBook": false,
            "slug": "test-market",
            "events": []
        });

        if let Some(outcome_prices) = outcome_prices {
            value["outcomePrices"] = serde_json::Value::String(outcome_prices.to_string());
        }

        value
    }

    fn make_gamma_market_with_outcome_prices(
        condition_id: &str,
        clob_token_ids: &str,
        outcome_prices: Option<&str>,
        closed: Option<bool>,
        accepting_orders: Option<bool>,
    ) -> GammaMarket {
        serde_json::from_value(make_gamma_market_value_with_outcome_prices(
            condition_id,
            clob_token_ids,
            outcome_prices,
            closed,
            accepting_orders,
        ))
        .expect("valid gamma market")
    }

    fn load_gamma_market_fixture(filename: &str) -> GammaMarket {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let content = std::fs::read_to_string(path).expect("fixture missing");
        serde_json::from_str(&content).expect("invalid gamma fixture json")
    }

    fn load_clob_market_fixture(filename: &str) -> ClobMarketResponse {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let content = std::fs::read_to_string(path).expect("fixture missing");
        serde_json::from_str(&content).expect("invalid clob fixture json")
    }

    #[rstest]
    fn build_strict_resolved_market_requires_closed_and_binary_resolution_prices() {
        let good = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        );
        let resolved = build_strict_resolved_market(&good).expect("expected resolved market");
        assert_eq!(resolved.condition_id, "0xCOND");
        assert_eq!(resolved.winning_asset_id, "0xYES");
        assert_eq!(resolved.winning_outcome, "Yes");

        let ambiguous = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"0.7\",\"0.3\"]"),
            Some(true),
            Some(false),
        );
        assert!(build_strict_resolved_market(&ambiguous).is_none());

        let malformed_token_count = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\",\"0xMAYBE\"]",
            Some("[\"1\",\"0\",\"0\"]"),
            Some(true),
            Some(false),
        );
        assert!(build_strict_resolved_market(&malformed_token_count).is_none());

        let mut malformed_outcome_count = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        );
        malformed_outcome_count.outcomes = "[\"Yes\",\"No\",\"Other\"]".to_string();
        assert!(build_strict_resolved_market(&malformed_outcome_count).is_none());

        let accepting_true = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(true),
        );
        let resolved =
            build_strict_resolved_market(&accepting_true).expect("expected resolved market");
        assert_eq!(resolved.winning_asset_id, "0xYES");

        let not_final = make_gamma_market_with_outcome_prices(
            "0xCOND",
            "[\"0xYES\",\"0xNO\"]",
            Some("[\"1\",\"0\"]"),
            Some(false),
            Some(true),
        );
        assert!(build_strict_resolved_market(&not_final).is_none());
    }

    #[rstest]
    fn build_strict_resolved_market_matches_official_fixture_shapes() {
        let closed = load_gamma_market_fixture("gamma_market_sports_market_money_line.json");
        let resolved = build_strict_resolved_market(&closed).expect("expected resolved fixture");
        assert_eq!(
            resolved.condition_id,
            "0x202abb9a80673068ec5ce9294d60e31eeaf3ab5c82fb21fb0c9142e5d0cab385"
        );
        assert_eq!(
            resolved.winning_asset_id,
            "89972346417086440659189114668296975440208562769200022591480064439842896371398"
        );

        let active = load_gamma_market_fixture("gamma_market.json");
        assert!(build_strict_resolved_market(&active).is_none());
    }

    #[rstest]
    fn build_strict_resolved_market_real_gamma_samples_cover_resolution_buckets() {
        let closed_binary_accepting_false =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_false.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_false)
            .expect("expected resolved market for binary accepting=false fixture");
        assert_eq!(
            resolved.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );

        let closed_binary_accepting_true =
            load_gamma_market_fixture("gamma_market_closed_binary_accepting_true.json");
        let resolved = build_strict_resolved_market(&closed_binary_accepting_true)
            .expect("expected resolved market for binary accepting=true fixture");
        assert_eq!(
            resolved.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );

        let closed_zero_zero =
            load_gamma_market_fixture("gamma_market_closed_zero_zero_legacy.json");
        assert!(build_strict_resolved_market(&closed_zero_zero).is_none());

        let closed_non_binary =
            load_gamma_market_fixture("gamma_market_closed_nonbinary_legacy.json");
        assert!(build_strict_resolved_market(&closed_non_binary).is_none());
    }

    #[rstest]
    fn build_resolved_market_from_clob_market_real_samples() {
        let accepting_false =
            load_clob_market_fixture("clob_market_closed_binary_accepting_false.json");
        let resolved_false = build_resolved_market_from_clob_market(&accepting_false)
            .expect("expected resolved market for accepting=false fixture");
        assert_eq!(
            resolved_false.condition_id,
            "0x8ccc3f4951ff02c1d34b87988752b4444ad17228732780a6cf22afefe8478bb6"
        );
        assert_eq!(resolved_false.winning_outcome, "No");
        assert_eq!(
            resolved_false.winning_asset_id,
            "89711174926330519158043401581181146613785179104141808554061413232025882707365"
        );

        let accepting_true =
            load_clob_market_fixture("clob_market_closed_binary_accepting_true.json");
        let resolved_true = build_resolved_market_from_clob_market(&accepting_true)
            .expect("expected resolved market for accepting=true fixture");
        assert_eq!(
            resolved_true.condition_id,
            "0xd57eed0d44f5b8ca54925d8d6ff440b146b3e6e071da18136ee3ee572d34479e"
        );
        assert_eq!(resolved_true.winning_outcome, "Yes");
        assert_eq!(
            resolved_true.winning_asset_id,
            "22978793223071892222859460592277435458011604214087068523744633723809814935807"
        );
    }

    #[rstest]
    fn parse_condition_ids_supports_single_multi_and_dedup() {
        let mut params = Params::new();
        params.insert("condition_id".to_string(), serde_json::json!("0xCOND-A"));
        params.insert(
            "condition_ids".to_string(),
            serde_json::json!(["0xCOND-B", "0xCOND-A", "0xCOND-B"]),
        );

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]);
    }

    #[rstest]
    fn parse_condition_ids_accepts_single_condition_ids_string() {
        let mut params = Params::new();
        params.insert("condition_ids".to_string(), serde_json::json!("0xCOND-A"));

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string()]);
    }

    #[rstest]
    fn parse_condition_ids_ignores_non_polymarket_instrument_ids() {
        let mut params = Params::new();
        params.insert(
            "instrument_ids".to_string(),
            serde_json::json!([
                "0xCOND-A-0xTOKENA.POLYMARKET",
                "BTCUSDT-PERP.BINANCE",
                "ETHUSDT-PERP.BINANCE"
            ]),
        );

        let parsed = parse_condition_ids_from_request_params(&Some(params));
        assert_eq!(parsed, vec!["0xCOND-A".to_string()]);
    }
}
