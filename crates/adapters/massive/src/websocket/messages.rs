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

//! Wire messages for the Massive WebSocket streaming API.
//!
//! Events arrive as JSON arrays; each element carries an `ev` discriminator
//! (`T` trade, `Q` quote, `A` second aggregate, `AM` minute aggregate,
//! `status` control).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Client-to-server action envelope (`auth`, `subscribe`, `unsubscribe`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveWsRequest {
    /// The action to perform.
    pub action: String,
    /// Comma-separated parameters (channels or the API key).
    pub params: String,
}

impl MassiveWsRequest {
    /// Creates a `subscribe` request for the given topics.
    #[must_use]
    pub fn subscribe(topics: &[String]) -> Self {
        Self {
            action: "subscribe".to_string(),
            params: topics.join(","),
        }
    }

    /// Creates an `unsubscribe` request for the given topics.
    #[must_use]
    pub fn unsubscribe(topics: &[String]) -> Self {
        Self {
            action: "unsubscribe".to_string(),
            params: topics.join(","),
        }
    }
}

/// A tick-level trade event (`ev = T`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveWsTrade {
    /// The ticker symbol.
    pub sym: Ustr,
    /// The trade price.
    pub p: Decimal,
    /// The trade size in shares (fractional executions carry decimals).
    #[serde(default)]
    pub s: Option<Decimal>,
    /// The trade ID.
    #[serde(default)]
    pub i: Option<String>,
    /// The exchange ID.
    #[serde(default)]
    pub x: Option<i64>,
    /// The trade condition codes.
    #[serde(default)]
    pub c: Option<Vec<i64>>,
    /// The tape identifier.
    #[serde(default)]
    pub z: Option<i64>,
    /// The SIP timestamp (Unix milliseconds).
    pub t: i64,
    /// The sequence number.
    #[serde(default)]
    pub q: Option<i64>,
}

/// An NBBO quote event (`ev = Q`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveWsQuote {
    /// The ticker symbol.
    pub sym: Ustr,
    /// The best bid price.
    #[serde(default)]
    pub bp: Option<Decimal>,
    /// The best bid size in shares.
    #[serde(default)]
    pub bs: Option<Decimal>,
    /// The bid exchange ID.
    #[serde(default)]
    pub bx: Option<i64>,
    /// The best ask price.
    #[serde(default)]
    pub ap: Option<Decimal>,
    /// The best ask size in shares.
    #[serde(default, rename = "as")]
    pub ask_size: Option<Decimal>,
    /// The ask exchange ID.
    #[serde(default)]
    pub ax: Option<i64>,
    /// The tape identifier.
    #[serde(default)]
    pub z: Option<i64>,
    /// The SIP timestamp (Unix milliseconds).
    pub t: i64,
    /// The sequence number.
    #[serde(default)]
    pub q: Option<i64>,
}

/// A per-second or per-minute OHLCV aggregate event (`ev = A` / `ev = AM`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveWsAggregate {
    /// The ticker symbol.
    pub sym: Ustr,
    /// The open price for the window.
    pub o: Decimal,
    /// The highest price for the window.
    pub h: Decimal,
    /// The lowest price for the window.
    pub l: Decimal,
    /// The close price for the window.
    pub c: Decimal,
    /// The volume for the window.
    pub v: Decimal,
    /// The accumulated volume for the session.
    #[serde(default)]
    pub av: Option<Decimal>,
    /// The official opening price for the session.
    #[serde(default)]
    pub op: Option<Decimal>,
    /// The volume weighted average price for the window.
    #[serde(default)]
    pub vw: Option<Decimal>,
    /// The session volume weighted average price.
    #[serde(default)]
    pub a: Option<Decimal>,
    /// The average trade size for the window.
    #[serde(default)]
    pub z: Option<Decimal>,
    /// The start of the window (Unix milliseconds).
    pub s: i64,
    /// The end of the window (Unix milliseconds).
    pub e: i64,
    /// Whether the window contains only OTC trades.
    #[serde(default)]
    pub otc: Option<bool>,
}

/// A control/status event (`ev = status`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MassiveWsStatus {
    /// The status code (`connected`, `auth_success`, `auth_failed`,
    /// `success`, `error`).
    pub status: String,
    /// A human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

/// A single event from the Massive WebSocket feed, discriminated by `ev`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ev")]
pub enum MassiveWsEvent {
    /// Tick-level trade.
    #[serde(rename = "T")]
    Trade(MassiveWsTrade),
    /// NBBO quote.
    #[serde(rename = "Q")]
    Quote(MassiveWsQuote),
    /// Per-second aggregate.
    #[serde(rename = "A")]
    AggregateSecond(MassiveWsAggregate),
    /// Per-minute aggregate.
    #[serde(rename = "AM")]
    AggregateMinute(MassiveWsAggregate),
    /// Control/status message.
    #[serde(rename = "status")]
    Status(MassiveWsStatus),
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::testing::load_test_fixture;

    #[rstest]
    fn test_deserialize_ws_trade() {
        let json = load_test_fixture("ws_trade.json");
        let events: Vec<MassiveWsEvent> = serde_json::from_str(&json).unwrap();

        assert_eq!(events.len(), 1);
        let MassiveWsEvent::Trade(trade) = &events[0] else {
            panic!("expected Trade, was {:?}", events[0]);
        };
        assert_eq!(trade.sym.as_str(), "MSFT");
        assert_eq!(trade.p, dec!(114.125));
        assert_eq!(trade.s, Some(dec!(100)));
        assert_eq!(trade.t, 1_536_036_818_784);
        assert_eq!(trade.i.as_deref(), Some("12345"));
    }

    #[rstest]
    fn test_deserialize_ws_quote() {
        let json = load_test_fixture("ws_quote.json");
        let events: Vec<MassiveWsEvent> = serde_json::from_str(&json).unwrap();

        let MassiveWsEvent::Quote(quote) = &events[0] else {
            panic!("expected Quote, was {:?}", events[0]);
        };
        assert_eq!(quote.sym.as_str(), "MSFT");
        assert_eq!(quote.bp, Some(dec!(114.125)));
        assert_eq!(quote.ap, Some(dec!(114.128)));
        assert_eq!(quote.bs, Some(dec!(100)));
        assert_eq!(quote.ask_size, Some(dec!(160)));
        assert_eq!(quote.t, 1_536_036_818_784);
    }

    #[rstest]
    fn test_deserialize_ws_aggregates() {
        let json = load_test_fixture("ws_aggregates.json");
        let events: Vec<MassiveWsEvent> = serde_json::from_str(&json).unwrap();

        assert_eq!(events.len(), 2);
        let MassiveWsEvent::AggregateSecond(agg) = &events[0] else {
            panic!("expected AggregateSecond, was {:?}", events[0]);
        };
        assert_eq!(agg.sym.as_str(), "SPCE");
        assert_eq!(agg.o, dec!(25.39));
        assert_eq!(agg.c, dec!(25.39));
        assert_eq!(agg.s, 1_610_144_868_000);
        assert_eq!(agg.e, 1_610_144_869_000);

        assert!(matches!(events[1], MassiveWsEvent::AggregateMinute(_)));
    }

    #[rstest]
    fn test_deserialize_ws_status() {
        let json = load_test_fixture("ws_status.json");
        let events: Vec<MassiveWsEvent> = serde_json::from_str(&json).unwrap();

        assert_eq!(events.len(), 2);
        let MassiveWsEvent::Status(status) = &events[0] else {
            panic!("expected Status, was {:?}", events[0]);
        };
        assert_eq!(status.status, "connected");

        let MassiveWsEvent::Status(status) = &events[1] else {
            panic!("expected Status, was {:?}", events[1]);
        };
        assert_eq!(status.status, "auth_success");
    }

    #[rstest]
    fn test_subscribe_request_serialization() {
        let request = MassiveWsRequest::subscribe(&["T.AAPL".to_string(), "Q.MSFT".to_string()]);
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"action":"subscribe","params":"T.AAPL,Q.MSFT"}"#);
    }

    #[rstest]
    fn test_unsubscribe_request_serialization() {
        let request = MassiveWsRequest::unsubscribe(&["AM.TSLA".to_string()]);
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"action":"unsubscribe","params":"AM.TSLA"}"#);
    }
}
