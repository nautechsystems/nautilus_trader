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

//! Shared model types for the Polymarket adapter.

use std::fmt::Display;

use nautilus_common::cache::Cache;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::PolymarketOutcome,
    parse::{deserialize_decimal_from_str, serialize_decimal_as_str},
};

/// A maker order included in trade messages.
///
/// Used by both REST trade reports and WebSocket user trade updates
/// to describe each maker-side fill in a match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketMakerOrder {
    pub asset_id: Ustr,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub fee_rate_bps: Decimal,
    pub maker_address: String,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub matched_amount: Decimal,
    pub order_id: String,
    pub outcome: PolymarketOutcome,
    pub owner: String,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub price: Decimal,
}

/// Human-readable label for a Polymarket instrument.
#[derive(Debug, Clone)]
pub struct PolymarketLabel {
    pub description: String,
    pub outcome: String,
}

impl Display for PolymarketLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]", self.description, self.outcome)
    }
}

impl PolymarketLabel {
    /// Build a label from an instrument reference.
    pub fn from_instrument(instrument: &InstrumentAny) -> Self {
        if let InstrumentAny::BinaryOption(opt) = instrument {
            Self {
                description: opt
                    .description
                    .map_or_else(|| instrument.id().to_string(), |d| d.to_string()),
                outcome: opt
                    .outcome
                    .map_or_else(|| "?".to_string(), |o| o.to_string()),
            }
        } else {
            Self {
                description: instrument.id().to_string(),
                outcome: "?".to_string(),
            }
        }
    }

    /// Look up an instrument by ID in the cache and build a label.
    /// Returns `None` if the instrument is not in the cache.
    pub fn from_cache(instrument_id: &InstrumentId, cache: &Cache) -> Option<Self> {
        cache.instrument(instrument_id).map(Self::from_instrument)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{common::enums::PolymarketOutcome, http::models::PolymarketTradeReport};

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn sample_maker_order_json() -> &'static str {
        r#"{
            "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            "fee_rate_bps": "10",
            "maker_address": "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            "matched_amount": "50.0000",
            "order_id": "0xorder001",
            "outcome": "Yes",
            "owner": "00000000-0000-0000-0000-000000000002",
            "price": "0.6000"
        }"#
    }

    #[rstest]
    fn test_maker_order_deserialization() {
        let order: PolymarketMakerOrder = serde_json::from_str(sample_maker_order_json()).unwrap();

        assert_eq!(
            order.asset_id.as_str(),
            "71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(order.fee_rate_bps, dec!(10));
        assert_eq!(
            order.maker_address.as_str(),
            "0x70997970c51812dc3a010c7d01b50e0d17dc79c8"
        );
        assert_eq!(order.matched_amount, dec!(50.0000));
        assert_eq!(order.order_id, "0xorder001");
        assert_eq!(order.outcome, PolymarketOutcome::yes());
        assert_eq!(order.price, dec!(0.6000));
    }

    #[rstest]
    fn test_maker_order_roundtrip() {
        let order: PolymarketMakerOrder = serde_json::from_str(sample_maker_order_json()).unwrap();
        let json = serde_json::to_string(&order).unwrap();
        let order2: PolymarketMakerOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(order, order2);
    }

    #[rstest]
    fn test_maker_order_outcome_no() {
        let json = r#"{
            "asset_id": "12345",
            "fee_rate_bps": "0",
            "maker_address": "0xaddr",
            "matched_amount": "10.0",
            "order_id": "order-1",
            "outcome": "No",
            "owner": "owner-1",
            "price": "0.4"
        }"#;
        let order: PolymarketMakerOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.outcome, PolymarketOutcome::no());
    }

    #[rstest]
    fn test_maker_order_decimal_precision() {
        // Verifies Decimal fields are serialized as strings (not floats)
        let order: PolymarketMakerOrder = serde_json::from_str(sample_maker_order_json()).unwrap();
        let json = serde_json::to_string(&order).unwrap();
        // Decimals must appear as quoted strings, not bare numbers
        assert!(
            json.contains("\"fee_rate_bps\":\"10\"") || json.contains("\"fee_rate_bps\": \"10\"")
        );
    }

    // Tests for embedded maker orders from the trade report fixture
    #[rstest]
    fn test_maker_orders_from_trade_report() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");

        assert_eq!(trade.maker_orders.len(), 2);
        let m0 = &trade.maker_orders[0];
        assert_eq!(m0.matched_amount, dec!(25.0000));
        assert_eq!(m0.fee_rate_bps, dec!(0));
        assert_eq!(m0.outcome, PolymarketOutcome::yes());

        let m1 = &trade.maker_orders[1];
        assert_eq!(m1.matched_amount, dec!(5.0000));
        assert_eq!(m1.fee_rate_bps, dec!(10));
    }
}
