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

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ustr::Ustr;

use crate::{
    common::enums::{
        HyperliquidBarInterval::{self, OneMinute},
        HyperliquidSide,
    },
    websocket::{
        HyperliquidWsChannel, HyperliquidWsError,
        messages::{
            HyperliquidWsMessage, HyperliquidWsRequest, PostRequest, SubscriptionRequest,
            WsLevelData,
        },
    },
};

/// Codec for encoding and decoding Hyperliquid WebSocket messages.
///
/// This struct provides methods to validate URLs and serialize/deserialize messages,
/// according to the Hyperliquid WebSocket protocol.
#[derive(Debug, Default)]
pub struct HyperliquidCodec;

impl HyperliquidCodec {
    /// Creates a new Hyperliquid codec instance.
    pub fn new() -> Self {
        Self
    }

    /// Validates that a URL is a proper WebSocket URL.
    pub fn validate_url(url: &str) -> Result<(), HyperliquidWsError> {
        if url.starts_with("ws://") || url.starts_with("wss://") {
            Ok(())
        } else {
            Err(HyperliquidWsError::UrlParsing(format!(
                "URL must start with ws:// or wss://, was: {url}"
            )))
        }
    }

    /// Encodes a WebSocket request to JSON bytes.
    pub fn encode(&self, request: &HyperliquidWsRequest) -> Result<Vec<u8>, HyperliquidWsError> {
        serde_json::to_vec(request).map_err(|e| {
            HyperliquidWsError::MessageSerialization(format!("Failed to serialize request: {e}"))
        })
    }

    /// Decodes JSON bytes to a WebSocket message.
    pub fn decode(&self, data: &[u8]) -> Result<HyperliquidWsMessage, HyperliquidWsError> {
        serde_json::from_slice(data).map_err(|e| {
            HyperliquidWsError::MessageDeserialization(format!(
                "Failed to deserialize message: {e}"
            ))
        })
    }
}

/// Canonical outbound (mirrors OKX/BitMEX "op + args" pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum WsOutbound {
    Subscribe {
        args: Vec<SubArg>,
        id: Option<String>,
    },
    Unsubscribe {
        args: Vec<SubArg>,
        id: Option<String>,
    },
    Ping,
    Post {
        id: String,
        path: String,
        body: serde_json::Value,
    },
    Auth {
        payload: serde_json::Value,
    },
}

// Type aliases for convenience and compatibility with your request
pub type SubRequest = SubArg;
pub type TradeSide = Side;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubArg {
    pub channel: HyperliquidWsChannel,
    #[serde(default)]
    pub symbol: Option<Ustr>, // unified symbol (coin in Hyperliquid)
    #[serde(default)]
    pub params: Option<serde_json::Value>, // {"interval":"1m","user":"0x123"} etc.
}

/// Canonical inbound (single tagged enum). Unknown stays debuggable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel", content = "data", rename_all = "camelCase")]
pub enum WsInbound {
    Trades(Vec<WsTrade>),
    L2Book(WsBook),
    Bbo(WsBbo),
    Candle(Vec<WsCandle>),
    AllMids(Vec<WsMid>),
    UserFills(Vec<WsFill>),
    UserFundings(Vec<WsFunding>),
    UserEvents(Vec<WsUserEvent>),

    SubscriptionResponse(SubResp),
    Pong(Option<i64>),
    Notification(Notice),
    Post(PostAck),

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubResp {
    pub ok: bool,
    pub id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notice {
    pub code: Option<String>,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostAck {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsTrade {
    pub instrument: Ustr,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub px: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub qty: Decimal,
    pub side: Side,
    pub ts: i64, // ms
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsBbo {
    pub instrument: Ustr,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub bid_px: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub bid_qty: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub ask_px: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub ask_qty: Decimal,
    pub ts: i64, // ms
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsCandle {
    pub instrument: Ustr,
    pub interval: HyperliquidBarInterval,
    pub open_ts: i64,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub o: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub h: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub l: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub c: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub v: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsBook {
    pub instrument: Ustr,
    pub is_snapshot: bool,
    pub seq: Option<u64>,
    pub checksum: Option<u32>,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
    pub ts: i64, // ms
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub px: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub qty: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMid {
    pub symbol: String,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub mid: Decimal,
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFill {
    pub symbol: String,
    pub order_id: String,
    pub trade_id: String,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub px: Decimal,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub qty: Decimal,
    pub side: Side,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFunding {
    pub symbol: String,
    #[serde(
        serialize_with = "serialize_decimal",
        deserialize_with = "deserialize_decimal"
    )]
    pub rate: Decimal,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsUserEvent {
    pub event_type: String,
    pub data: serde_json::Value,
    pub ts: i64,
}

fn serialize_decimal<S: Serializer>(d: &Decimal, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.normalize().to_string())
}

fn deserialize_decimal<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Decimal::from_str(&s).map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => {
            Decimal::from_str(&n.to_string()).map_err(serde::de::Error::custom)
        }
        _ => Err(serde::de::Error::custom(
            "expected decimal string or number",
        )),
    }
}

/// Convert normalized outbound message to Hyperliquid native format.
pub fn encode_outbound(msg: &WsOutbound) -> HyperliquidWsRequest {
    match msg {
        WsOutbound::Subscribe { args, id: _ } => {
            // Convert first SubArg to Hyperliquid SubscriptionRequest
            if let Some(arg) = args.first() {
                let subscription = match arg.channel {
                    HyperliquidWsChannel::Trades => SubscriptionRequest::Trades {
                        coin: arg.symbol.unwrap_or_default(),
                    },
                    HyperliquidWsChannel::L2Book => SubscriptionRequest::L2Book {
                        coin: arg.symbol.unwrap_or_default(),
                        n_sig_figs: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("nSigFigs"))
                            .and_then(|v| v.as_u64())
                            .map(|u| u as u32),
                        mantissa: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("mantissa"))
                            .and_then(|v| v.as_u64())
                            .map(|u| u as u32),
                    },
                    HyperliquidWsChannel::Bbo => SubscriptionRequest::Bbo {
                        coin: arg.symbol.unwrap_or_default(),
                    },
                    HyperliquidWsChannel::Candle => SubscriptionRequest::Candle {
                        coin: arg.symbol.unwrap_or_default(),
                        interval: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("interval"))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<HyperliquidBarInterval>().ok())
                            .unwrap_or(OneMinute),
                    },
                    HyperliquidWsChannel::AllMids => SubscriptionRequest::AllMids {
                        dex: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("dex"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    },
                    HyperliquidWsChannel::Notification => SubscriptionRequest::Notification {
                        user: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("user"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    },
                    _ => SubscriptionRequest::AllMids { dex: None }, // Default fallback
                };

                HyperliquidWsRequest::Subscribe { subscription }
            } else {
                HyperliquidWsRequest::Ping // Fallback
            }
        }
        WsOutbound::Unsubscribe { args, id: _ } => {
            if let Some(arg) = args.first() {
                let subscription = match arg.channel {
                    HyperliquidWsChannel::Trades => SubscriptionRequest::Trades {
                        coin: arg.symbol.unwrap_or_default(),
                    },
                    HyperliquidWsChannel::L2Book => SubscriptionRequest::L2Book {
                        coin: arg.symbol.unwrap_or_default(),
                        n_sig_figs: None,
                        mantissa: None,
                    },
                    HyperliquidWsChannel::Bbo => SubscriptionRequest::Bbo {
                        coin: arg.symbol.unwrap_or_default(),
                    },
                    HyperliquidWsChannel::Candle => SubscriptionRequest::Candle {
                        coin: arg.symbol.unwrap_or_default(),
                        interval: arg
                            .params
                            .as_ref()
                            .and_then(|p| p.get("interval"))
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<HyperliquidBarInterval>().ok())
                            .unwrap_or(OneMinute),
                    },
                    _ => SubscriptionRequest::AllMids { dex: None },
                };

                HyperliquidWsRequest::Unsubscribe { subscription }
            } else {
                HyperliquidWsRequest::Ping
            }
        }
        WsOutbound::Ping => HyperliquidWsRequest::Ping,
        WsOutbound::Post { id, path: _, body } => HyperliquidWsRequest::Post {
            id: id.parse::<u64>().unwrap_or(1),
            request: PostRequest::Info {
                payload: body.clone(),
            },
        },
        WsOutbound::Auth { payload } => HyperliquidWsRequest::Post {
            id: 1,
            request: PostRequest::Info {
                payload: payload.clone(),
            }, // Simplified for now
        },
    }
}

/// Convert Hyperliquid native message to normalized inbound format.
pub fn decode_inbound(msg: &HyperliquidWsMessage) -> WsInbound {
    match msg {
        HyperliquidWsMessage::SubscriptionResponse { data } => {
            WsInbound::SubscriptionResponse(SubResp {
                ok: true,
                id: None,
                message: Some(format!("Subscribed to {data:?}")),
            })
        }
        HyperliquidWsMessage::Post { data } => WsInbound::Post(PostAck {
            id: data.id.to_string(),
            ok: true,
            error: None,
        }),
        HyperliquidWsMessage::Trades { data } => {
            let trades = data
                .iter()
                .map(|t| WsTrade {
                    instrument: t.coin,
                    px: Decimal::from_str(&t.px).unwrap_or_default(),
                    qty: Decimal::from_str(&t.sz).unwrap_or_default(),
                    side: match t.side {
                        HyperliquidSide::Sell => Side::Sell,
                        HyperliquidSide::Buy => Side::Buy,
                    },
                    ts: t.time as i64,
                    id: Some(t.tid.to_string()),
                })
                .collect();
            WsInbound::Trades(trades)
        }
        HyperliquidWsMessage::L2Book { data } => {
            let bids = data.levels[0]
                .iter()
                .filter(|l| l.n > 0) // Active levels
                .map(|l| Level {
                    px: Decimal::from_str(&l.px).unwrap_or_default(),
                    qty: Decimal::from_str(&l.sz).unwrap_or_default(),
                })
                .collect();

            let asks = data.levels[1]
                .iter()
                .filter(|l| l.n > 0) // Active levels
                .map(|l| Level {
                    px: Decimal::from_str(&l.px).unwrap_or_default(),
                    qty: Decimal::from_str(&l.sz).unwrap_or_default(),
                })
                .collect();

            WsInbound::L2Book(WsBook {
                instrument: data.coin,
                is_snapshot: true, // Hyperliquid sends snapshots
                seq: Some(data.time),
                checksum: None,
                bids,
                asks,
                ts: data.time as i64,
            })
        }
        HyperliquidWsMessage::Bbo { data } => {
            // Access bid and ask from the bbo array: [bid, ask]
            let default_level = WsLevelData {
                px: "0".to_string(),
                sz: "0".to_string(),
                n: 0,
            };
            let bid = data.bbo[0].as_ref().unwrap_or(&default_level);
            let ask = data.bbo[1].as_ref().unwrap_or(&default_level);

            WsInbound::Bbo(WsBbo {
                instrument: data.coin,
                bid_px: Decimal::from_str(&bid.px).unwrap_or_default(),
                bid_qty: Decimal::from_str(&bid.sz).unwrap_or_default(),
                ask_px: Decimal::from_str(&ask.px).unwrap_or_default(),
                ask_qty: Decimal::from_str(&ask.sz).unwrap_or_default(),
                ts: data.time as i64,
            })
        }
        HyperliquidWsMessage::Candle { data } => match HyperliquidBarInterval::from_str(&data.i) {
            Ok(interval) => {
                let candle = WsCandle {
                    instrument: data.s,
                    interval,
                    open_ts: data.t as i64,
                    o: Decimal::from_str(&data.o).unwrap_or_default(),
                    h: Decimal::from_str(&data.h).unwrap_or_default(),
                    l: Decimal::from_str(&data.l).unwrap_or_default(),
                    c: Decimal::from_str(&data.c).unwrap_or_default(),
                    v: Decimal::from_str(&data.v).unwrap_or_default(),
                };
                WsInbound::Candle(vec![candle])
            }
            Err(e) => {
                tracing::error!("Failed to parse candle interval '{}': {}", data.i, e);
                WsInbound::Unknown
            }
        },
        HyperliquidWsMessage::Notification { data } => WsInbound::Notification(Notice {
            code: None,
            msg: Some(data.notification.clone()),
        }),
        HyperliquidWsMessage::Pong => WsInbound::Pong(Some(chrono::Utc::now().timestamp_millis())),
        _ => WsInbound::Unknown,
    }
}
