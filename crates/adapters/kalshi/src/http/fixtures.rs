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

//! Response fixtures shared by the adapter's unit tests.
//!
//! The payloads are shaped like the exchange's own responses, including the fixed-point string
//! encodings, so the parsers are exercised against the wire format rather than a convenient one.

use crate::http::models::{KalshiEvent, KalshiMarket};

/// A fixed timestamp used by the unit tests.
pub(crate) const TS: u64 = 1_735_732_800_000_000_000;

/// A market payload shaped like the exchange's own response.
pub(crate) const MARKET_JSON: &str = r#"{
    "ticker": "KXHIGHNY-25JAN01-T50",
    "event_ticker": "KXHIGHNY-25JAN01",
    "market_type": "binary",
    "yes_sub_title": "50 degrees or above",
    "no_sub_title": "49 degrees or below",
    "created_time": "2024-12-30T15:00:00Z",
    "updated_time": "2025-01-01T06:00:00Z",
    "open_time": "2024-12-30T15:00:00Z",
    "close_time": "2025-01-02T05:00:00Z",
    "latest_expiration_time": "2025-01-05T05:00:00Z",
    "settlement_timer_seconds": 1800,
    "status": "active",
    "notional_value_dollars": "1.0000",
    "yes_bid_dollars": "0.3400",
    "yes_ask_dollars": "0.3500",
    "no_bid_dollars": "0.6500",
    "no_ask_dollars": "0.6600",
    "yes_bid_size_fp": "120.00",
    "yes_ask_size_fp": "80.00",
    "last_price_dollars": "0.3500",
    "previous_yes_bid_dollars": "0.3300",
    "previous_yes_ask_dollars": "0.3600",
    "previous_price_dollars": "0.3400",
    "volume_fp": "1520.00",
    "volume_24h_fp": "310.00",
    "open_interest_fp": "900.00",
    "result": "",
    "can_close_early": true,
    "expiration_value": "51",
    "rules_primary": "Resolves YES if the high is 50 or above.",
    "rules_secondary": "Source: NWS Central Park.",
    "price_level_structure": "linear_cent",
    "price_ranges": [
        {"start": "0.0000", "end": "1.0000", "step": "0.0100"}
    ]
}"#;

/// Returns the market fixture.
pub(crate) fn market() -> KalshiMarket {
    serde_json::from_str(MARKET_JSON).expect("market fixture decodes")
}

/// Returns the market fixture settled with the given result.
///
/// The settlement value is the exchange's reported value of the YES side, which is the contract's
/// notional value when YES wins and nothing when NO wins.
pub(crate) fn settled_market(result: &str) -> KalshiMarket {
    let settlement_value = if result == "yes" { "1.0000" } else { "0.0000" };

    serde_json::from_str(&settled_json(result, Some(settlement_value)))
        .expect("settled market fixture decodes")
}

/// Returns the market fixture settled with the given result, without a published settlement value.
pub(crate) fn settled_market_without_settlement_value(result: &str) -> KalshiMarket {
    serde_json::from_str(&settled_json(result, None)).expect("settled market fixture decodes")
}

fn settled_json(result: &str, settlement_value: Option<&str>) -> String {
    let raw = MARKET_JSON
        .replace("\"status\": \"active\"", "\"status\": \"finalized\"")
        .replace("\"result\": \"\"", &format!("\"result\": \"{result}\""));
    let mut settled_fields = String::from("\"settlement_ts\": \"2025-01-02T06:00:00Z\",");

    if let Some(value) = settlement_value {
        settled_fields = format!("\"settlement_value_dollars\": \"{value}\", {settled_fields}");
    }

    raw.replace(
        "\"expiration_value\": \"51\",",
        &format!("\"expiration_value\": \"51\", {settled_fields}"),
    )
}

/// Returns the event fixture that owns the market fixture.
pub(crate) fn event() -> KalshiEvent {
    serde_json::from_str(
        r#"{
            "event_ticker": "KXHIGHNY-25JAN01",
            "series_ticker": "KXHIGHNY",
            "sub_title": "Highest temperature in NYC",
            "title": "Highest temperature in NYC on Jan 1, 2025",
            "collateral_return_type": "binary",
            "mutually_exclusive": true
        }"#,
    )
    .expect("event fixture decodes")
}
