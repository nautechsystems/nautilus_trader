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

//! WebSocket message types for the Kalshi real-time feed.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::KalshiTakerSide;

// ---------------------------------------------------------------------------
// Outbound: subscription commands (client → server)
// ---------------------------------------------------------------------------

/// Parameters for a WebSocket subscription command.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiSubscribeParams {
    pub channels: Vec<String>,
    pub market_tickers: Vec<String>,
}

/// Command to subscribe to one or more market channels.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiSubscribeCmd {
    pub id: u32,
    pub cmd: &'static str,
    pub params: KalshiSubscribeParams,
}

impl KalshiSubscribeCmd {
    /// Create an `orderbook_delta` subscription for the given market tickers.
    #[must_use]
    pub fn orderbook(id: u32, market_tickers: Vec<String>) -> Self {
        Self {
            id,
            cmd: "subscribe",
            params: KalshiSubscribeParams {
                channels: vec!["orderbook_delta".to_string()],
                market_tickers,
            },
        }
    }

    /// Create a `trade` subscription for the given market tickers.
    #[must_use]
    pub fn trades(id: u32, market_tickers: Vec<String>) -> Self {
        Self {
            id,
            cmd: "subscribe",
            params: KalshiSubscribeParams {
                channels: vec!["trade".to_string()],
                market_tickers,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Inbound: message envelope (server → client)
// ---------------------------------------------------------------------------

/// Raw top-level envelope for all server→client WebSocket messages.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsEnvelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub sid: Option<u32>,
    pub seq: Option<u64>,
    pub msg: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Inbound: typed message bodies
// ---------------------------------------------------------------------------

/// Price level in a WebSocket orderbook message: `(price_dollars, count_fp)`.
pub type WsPriceLevel = (String, String);

/// Orderbook snapshot — sent as the first message after subscribing to `orderbook_delta`.
///
/// YES bids and NO bids are both sorted ascending by price.
/// The **best bid is the last element** in each vector.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsOrderbookSnapshot {
    pub market_ticker: Ustr,
    /// YES bids sorted ascending (best bid = last element).
    #[serde(default)]
    pub yes_dollars_fp: Vec<WsPriceLevel>,
    /// NO bids sorted ascending (best NO bid = last element).
    #[serde(default)]
    pub no_dollars_fp: Vec<WsPriceLevel>,
}

/// Orderbook delta — incremental update after the initial snapshot.
///
/// A delta at a price level means the quantity changed by `delta_fp`.
/// When `delta_fp == "0.00"` (or the result reaches zero), that level is removed.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsOrderbookDelta {
    pub market_ticker: Ustr,
    /// Price level being updated (dollar string, e.g. `"0.4200"`).
    pub price_dollars: String,
    /// Signed quantity change. Negative = contracts removed.
    pub delta_fp: String,
    /// Which side is updated: `"yes"` or `"no"`.
    pub side: String,
    /// ISO 8601 timestamp (optional).
    pub ts: Option<String>,
}

/// A public trade event from the `trade` channel.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsTrade {
    pub trade_id: String,
    pub market_ticker: Ustr,
    /// YES execution price as dollar string.
    pub yes_price_dollars: String,
    /// NO execution price as dollar string.
    pub no_price_dollars: String,
    /// Contract count with 2 decimal places.
    pub count_fp: String,
    pub taker_side: KalshiTakerSide,
    /// Unix timestamp in seconds.
    pub ts: u64,
}

/// Error message from the server.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsErrorMsg {
    pub code: u32,
    pub msg: String,
}

// ---------------------------------------------------------------------------
// Parsed message enum
// ---------------------------------------------------------------------------

/// A parsed, typed WebSocket message from the Kalshi server.
#[derive(Clone, Debug)]
pub enum KalshiWsMessage {
    /// Orderbook snapshot (first message after subscribing to `orderbook_delta`).
    OrderbookSnapshot {
        sid: u32,
        seq: u64,
        data: KalshiWsOrderbookSnapshot,
    },
    /// Incremental orderbook update.
    OrderbookDelta {
        sid: u32,
        seq: u64,
        data: KalshiWsOrderbookDelta,
    },
    /// Public trade event.
    Trade {
        sid: u32,
        seq: u64,
        data: KalshiWsTrade,
    },
    /// Server error response.
    Error(KalshiWsErrorMsg),
    /// An unrecognized message type (logged and ignored).
    Unknown(String),
}

impl KalshiWsMessage {
    /// Parse a raw JSON string from the WebSocket into a typed [`KalshiWsMessage`].
    ///
    /// # Errors
    ///
    /// Returns an error if the top-level envelope JSON is invalid.
    /// Individual message body parse failures are returned as [`KalshiWsMessage::Unknown`]
    /// with a warning log rather than propagating as errors (resilient parsing).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let env: KalshiWsEnvelope = serde_json::from_str(json)?;
        let sid = env.sid.unwrap_or(0);
        let seq = env.seq.unwrap_or(0);
        let raw_msg = env.msg.unwrap_or(serde_json::Value::Null);

        let msg = match env.msg_type.as_str() {
            "orderbook_snapshot" => {
                match serde_json::from_value::<KalshiWsOrderbookSnapshot>(raw_msg) {
                    Ok(data) => Self::OrderbookSnapshot { sid, seq, data },
                    Err(e) => {
                        log::warn!("Kalshi: failed to parse orderbook_snapshot: {e}");
                        Self::Unknown("orderbook_snapshot".to_string())
                    }
                }
            }
            "orderbook_delta" => {
                match serde_json::from_value::<KalshiWsOrderbookDelta>(raw_msg) {
                    Ok(data) => Self::OrderbookDelta { sid, seq, data },
                    Err(e) => {
                        log::warn!("Kalshi: failed to parse orderbook_delta: {e}");
                        Self::Unknown("orderbook_delta".to_string())
                    }
                }
            }
            "trade" => match serde_json::from_value::<KalshiWsTrade>(raw_msg) {
                Ok(data) => Self::Trade { sid, seq, data },
                Err(e) => {
                    log::warn!("Kalshi: failed to parse trade: {e}");
                    Self::Unknown("trade".to_string())
                }
            },
            "error" => match serde_json::from_value::<KalshiWsErrorMsg>(raw_msg) {
                Ok(err) => Self::Error(err),
                Err(_) => Self::Unknown("error".to_string()),
            },
            other => Self::Unknown(other.to_string()),
        };

        Ok(msg)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing: {name}"))
    }

    #[test]
    fn test_parse_orderbook_snapshot() {
        let json = load_fixture("ws_orderbook_snapshot.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::OrderbookSnapshot { sid, seq, data } => {
                assert_eq!(sid, 1);
                assert_eq!(seq, 1);
                assert_eq!(data.market_ticker.as_str(), "KXBTC-25MAR15-B100000");
                assert_eq!(data.yes_dollars_fp.len(), 2);
                assert_eq!(data.no_dollars_fp.len(), 2);
                // Best YES bid is last (highest price).
                assert_eq!(data.yes_dollars_fp[1].0, "0.4200");
                assert_eq!(data.yes_dollars_fp[1].1, "13.00");
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn test_parse_orderbook_delta() {
        let json = load_fixture("ws_orderbook_delta.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::OrderbookDelta { sid, seq, data } => {
                assert_eq!(sid, 1);
                assert_eq!(seq, 2);
                assert_eq!(data.market_ticker.as_str(), "KXBTC-25MAR15-B100000");
                assert_eq!(data.price_dollars, "0.4200");
                assert_eq!(data.delta_fp, "50.00");
                assert_eq!(data.side, "yes");
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn test_parse_trade() {
        let json = load_fixture("ws_trade.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::Trade { sid: _, seq, data } => {
                assert_eq!(seq, 1);
                assert_eq!(data.market_ticker.as_str(), "KXBTC-25MAR15-B100000");
                assert_eq!(data.yes_price_dollars, "0.3600");
                assert_eq!(data.taker_side, KalshiTakerSide::No);
                assert_eq!(data.count_fp, "136.00");
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn test_subscribe_cmd_orderbook_serializes() {
        let cmd = KalshiSubscribeCmd::orderbook(1, vec!["KXBTC-25MAR15-B100000".to_string()]);
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orderbook_delta"));
        assert!(json.contains("KXBTC-25MAR15-B100000"));
    }

    #[test]
    fn test_subscribe_cmd_trades_serializes() {
        let cmd = KalshiSubscribeCmd::trades(2, vec!["KXBTC-25MAR15-B100000".to_string()]);
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""trade""#));
    }
}
