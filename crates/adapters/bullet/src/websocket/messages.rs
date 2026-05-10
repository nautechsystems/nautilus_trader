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

//! Bullet WebSocket message types (Binance FAPI-compatible wire format).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
// All ServerMessage variants derive Serialize so they can be forwarded as JSON to Python.

use crate::common::enums::BulletMessageType;

// ── Client → server ───────────────────────────────────────────────────────────

/// Subscribe request sent to the Bullet WS server.
#[derive(Debug, Serialize)]
pub struct SubscribeRequest {
    pub method: &'static str,
    pub params: Vec<String>,
    pub id: u64,
}

impl SubscribeRequest {
    /// Create a SUBSCRIBE message.
    pub fn subscribe(params: Vec<String>, id: u64) -> Self {
        Self { method: "SUBSCRIBE", params, id }
    }

    /// Create an UNSUBSCRIBE message.
    pub fn unsubscribe(params: Vec<String>, id: u64) -> Self {
        Self { method: "UNSUBSCRIBE", params, id }
    }
}

// ── Server → client ───────────────────────────────────────────────────────────

/// All possible server-pushed messages (untagged serde dispatch).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ServerMessage {
    DepthUpdate(DepthUpdate),
    BookTicker(BookTickerUpdate),
    AggTrade(AggTradeUpdate),
    MarkPrice(MarkPriceUpdate),
    OrderUpdate(OrderUpdate),
    /// Subscribe/unsubscribe acknowledgement or error.
    Result(MethodResult),
    /// Unrecognized payload — kept for debugging.
    Unknown(serde_json::Value),
}

/// L2 depth snapshot or incremental update.
///
/// `mt: "s"` = full snapshot; `mt: "u"` = incremental update.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DepthUpdate {
    /// Event type: `"depthUpdate"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time (ms).
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    /// Previous update ID (for gap detection).
    pub pu: u64,
    /// Update ID (last update id in this batch).
    #[serde(rename = "u")]
    pub update_id: u64,
    /// Message type: snapshot or update.
    pub mt: BulletMessageType,
    /// Bids as `[[price, qty], ...]`.
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    /// Asks as `[[price, qty], ...]`.
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

/// Best bid/ask quote (book ticker).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BookTickerUpdate {
    /// Event type: `"bookTicker"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Update ID.
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    /// Best bid price.
    #[serde(rename = "b")]
    pub bid_price: Decimal,
    /// Best bid qty.
    #[serde(rename = "B")]
    pub bid_qty: Decimal,
    /// Best ask price.
    #[serde(rename = "a")]
    pub ask_price: Decimal,
    /// Best ask qty.
    #[serde(rename = "A")]
    pub ask_qty: Decimal,
    /// Event time (ms).
    #[serde(rename = "T")]
    pub transaction_time: i64,
}

/// Aggregated trade event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AggTradeUpdate {
    /// Event type: `"aggTrade"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time (ms).
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    /// Aggregate trade ID.
    #[serde(rename = "a")]
    pub agg_trade_id: u64,
    /// Price.
    #[serde(rename = "p")]
    pub price: Decimal,
    /// Quantity.
    #[serde(rename = "q")]
    pub quantity: Decimal,
    /// Trade time (ms).
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Whether the buyer is market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Mark price update.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkPriceUpdate {
    /// Event type: `"markPriceUpdate"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time (ms).
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    /// Mark price.
    #[serde(rename = "p")]
    pub mark_price: Decimal,
    /// Funding rate.
    #[serde(rename = "r")]
    pub funding_rate: Decimal,
    /// Next funding time (ms) — absent on some feeds.
    #[serde(rename = "T", default)]
    pub next_funding_time: Option<i64>,
}

/// User order update — wire format used by the Bullet `{address}@user.orders` stream.
///
/// The outer envelope has `e:"orderTradeUpdate"`, `E:<timestamp_us>`, and `o:{...}` (nested).
/// The `o` object is an untagged enum dispatched on which fields are present.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderUpdate {
    /// Event type: `"orderTradeUpdate"`.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event timestamp (microseconds).
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Order data (nested under `"o"`).
    #[serde(rename = "o")]
    pub order: OrderUpdateData,
}

impl OrderUpdate {
    /// Flatten to a simple JSON object for dispatch to Python.
    ///
    /// Python only needs a handful of fields; this avoids exposing the nested structure.
    pub fn to_flat_json(&self) -> serde_json::Value {
        let common = self.order.common();
        let (side, price, qty, last_fill_qty, last_fill_price) = self.order.execution_fields();
        serde_json::json!({
            "e": "ORDER_TRADE_UPDATE",
            "E": self.event_time,
            "s": common.symbol,
            "orderId": common.order_id,
            "clientOrderId": common.client_order_id,
            "status": common.status,
            "side": side,
            "price": price,
            "origQty": qty,
            "lastFilledQty": last_fill_qty,
            "lastFilledPrice": last_fill_price,
            "T": common.transaction_time,
        })
    }
}

/// Shared fields present in every order update variant.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderUpdateCommon {
    #[serde(rename = "s")]
    pub symbol: String,
    /// Venue-assigned order ID.
    #[serde(rename = "i")]
    pub order_id: u64,
    /// Client order ID (our auto-increment integer, cast to string).
    #[serde(rename = "co", default)]
    pub client_order_id: Option<serde_json::Value>,
    /// Order status: `"NEW"`, `"CANCELED"`, `"PARTIALLY_FILLED"`, `"FILLED"`.
    #[serde(rename = "X")]
    pub status: String,
    #[serde(rename = "x")]
    pub execution_type: String,
    /// Transaction timestamp (microseconds).
    #[serde(rename = "T")]
    pub transaction_time: i64,
    #[serde(rename = "th")]
    pub tx_hash: String,
    #[serde(rename = "ua")]
    pub user_address: String,
}

/// NEW order placed (status = NEW, no fill yet).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaceOrderData {
    #[serde(flatten)]
    pub common: OrderUpdateCommon,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "o")]
    pub order_type: String,
    #[serde(rename = "f")]
    pub time_in_force: String,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
}

/// CANCELED order.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CancelOrderData {
    #[serde(flatten)]
    pub common: OrderUpdateCommon,
}

/// PARTIALLY_FILLED or FILLED order.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TradeFillData {
    #[serde(flatten)]
    pub common: OrderUpdateCommon,
    #[serde(rename = "S")]
    pub side: String,
    /// Original limit price (may be absent for market orders).
    #[serde(rename = "p", default)]
    pub price: Option<String>,
    /// Original quantity.
    #[serde(rename = "q", default)]
    pub quantity: Option<String>,
    /// Quantity filled in this event.
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    /// Price of this fill.
    #[serde(rename = "L")]
    pub last_filled_price: String,
    /// Commission charged for this fill.
    #[serde(rename = "n")]
    pub commission: String,
}

/// Untagged order data enum — serde tries each variant in order.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum OrderUpdateData {
    TradeFill(TradeFillData),
    PlaceOrder(PlaceOrderData),
    Cancel(CancelOrderData),
}

impl OrderUpdateData {
    pub fn common(&self) -> &OrderUpdateCommon {
        match self {
            Self::TradeFill(d) => &d.common,
            Self::PlaceOrder(d) => &d.common,
            Self::Cancel(d) => &d.common,
        }
    }

    /// Returns `(side, limit_price, orig_qty, last_fill_qty, last_fill_price)`.
    fn execution_fields(&self) -> (&str, &str, &str, &str, &str) {
        match self {
            Self::TradeFill(d) => (
                &d.side,
                d.price.as_deref().unwrap_or("0"),
                d.quantity.as_deref().unwrap_or("0"),
                &d.last_filled_qty,
                &d.last_filled_price,
            ),
            Self::PlaceOrder(d) => (&d.side, &d.price, &d.quantity, "0", "0"),
            Self::Cancel(d) => {
                let _ = d;
                ("", "0", "0", "0", "0")
            }
        }
    }
}

/// Subscribe/unsubscribe acknowledgement from the server.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MethodResult {
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDR: &str = "4XN8Apf9powArmYLH1DRb2QvmyuYuvZp4qtdU8AtCavU";

    // ── PlaceOrder ────────────────────────────────────────────────────────────

    #[test]
    fn deserialize_place_order() {
        let json = r#"{"e":"orderTradeUpdate","E":1778341535100726,"o":{"s":"SOL-USD","i":85060264,"co":"42","X":"NEW","x":"NEW","T":1778341535093092,"th":"0xabc","ua":"4XN8Apf9powArmYLH1DRb2QvmyuYuvZp4qtdU8AtCavU","S":"BUY","o":"LIMIT","f":"GTC","p":"92.50","q":"0.5"}}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        match msg {
            ServerMessage::OrderUpdate(u) => {
                assert_eq!(u.event_type, "orderTradeUpdate");
                assert_eq!(u.event_time, 1778341535100726);
                match u.order {
                    OrderUpdateData::PlaceOrder(d) => {
                        assert_eq!(d.common.symbol, "SOL-USD");
                        assert_eq!(d.common.order_id, 85060264);
                        assert_eq!(d.common.status, "NEW");
                        assert_eq!(d.side, "BUY");
                        assert_eq!(d.price, "92.50");
                        assert_eq!(d.quantity, "0.5");
                    }
                    other => panic!("expected PlaceOrder, got {other:?}"),
                }
            }
            other => panic!("expected OrderUpdate, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_cancel_order() {
        let json = r#"{"e":"orderTradeUpdate","E":1778341535100726,"o":{"s":"SOL-USD","i":85060264,"co":"42","X":"CANCELED","x":"CANCELED","T":1778341535093092,"th":"0xabc","ua":"4XN8Apf9powArmYLH1DRb2QvmyuYuvZp4qtdU8AtCavU"}}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        match msg {
            ServerMessage::OrderUpdate(u) => match u.order {
                OrderUpdateData::Cancel(d) => {
                    assert_eq!(d.common.status, "CANCELED");
                    assert_eq!(d.common.order_id, 85060264);
                }
                other => panic!("expected Cancel, got {other:?}"),
            },
            other => panic!("expected OrderUpdate, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_trade_fill() {
        let json = r#"{"e":"orderTradeUpdate","E":1778341535100726,"o":{"s":"SOL-USD","i":85060264,"co":"42","X":"FILLED","x":"TRADE","T":1778341535093092,"th":"0xabc","ua":"4XN8Apf9powArmYLH1DRb2QvmyuYuvZp4qtdU8AtCavU","S":"BUY","p":"92.50","q":"0.5","l":"0.5","L":"92.50","n":"0.001"}}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        match msg {
            ServerMessage::OrderUpdate(u) => match u.order {
                OrderUpdateData::TradeFill(d) => {
                    assert_eq!(d.common.status, "FILLED");
                    assert_eq!(d.last_filled_qty, "0.5");
                    assert_eq!(d.last_filled_price, "92.50");
                    assert_eq!(d.commission, "0.001");
                }
                other => panic!("expected TradeFill, got {other:?}"),
            },
            other => panic!("expected OrderUpdate, got {other:?}"),
        }
    }

    // ── to_flat_json ──────────────────────────────────────────────────────────

    fn make_place_order(status: &str) -> OrderUpdate {
        OrderUpdate {
            event_type: "orderTradeUpdate".into(),
            event_time: 1_000_000,
            order: OrderUpdateData::PlaceOrder(PlaceOrderData {
                common: OrderUpdateCommon {
                    symbol: "SOL-USD".into(),
                    order_id: 99,
                    client_order_id: Some(serde_json::json!(42)),
                    status: status.into(),
                    execution_type: status.into(),
                    transaction_time: 999_999,
                    tx_hash: "0x0".into(),
                    user_address: ADDR.into(),
                },
                side: "BUY".into(),
                order_type: "LIMIT".into(),
                time_in_force: "GTC".into(),
                price: "92.50".into(),
                quantity: "0.5".into(),
            }),
        }
    }

    #[test]
    fn flat_json_place_order_has_correct_fields() {
        let update = make_place_order("NEW");
        let flat = update.to_flat_json();

        assert_eq!(flat["e"], "ORDER_TRADE_UPDATE");
        assert_eq!(flat["s"], "SOL-USD");
        assert_eq!(flat["orderId"], 99);
        assert_eq!(flat["status"], "NEW");
        assert_eq!(flat["side"], "BUY");
        assert_eq!(flat["price"], "92.50");
        assert_eq!(flat["origQty"], "0.5");
        assert_eq!(flat["lastFilledQty"], "0");
        assert_eq!(flat["lastFilledPrice"], "0");
        assert_eq!(flat["E"], 1_000_000);
        assert_eq!(flat["T"], 999_999);
    }

    #[test]
    fn flat_json_trade_fill_carries_fill_fields() {
        let update = OrderUpdate {
            event_type: "orderTradeUpdate".into(),
            event_time: 2_000_000,
            order: OrderUpdateData::TradeFill(TradeFillData {
                common: OrderUpdateCommon {
                    symbol: "BTC-USD".into(),
                    order_id: 77,
                    client_order_id: None,
                    status: "FILLED".into(),
                    execution_type: "TRADE".into(),
                    transaction_time: 1_999_999,
                    tx_hash: "0x1".into(),
                    user_address: ADDR.into(),
                },
                side: "SELL".into(),
                price: Some("50000.0".into()),
                quantity: Some("0.001".into()),
                last_filled_qty: "0.001".into(),
                last_filled_price: "49999.5".into(),
                commission: "0.005".into(),
            }),
        };
        let flat = update.to_flat_json();

        assert_eq!(flat["lastFilledQty"], "0.001");
        assert_eq!(flat["lastFilledPrice"], "49999.5");
        assert_eq!(flat["side"], "SELL");
    }

    #[test]
    fn flat_json_cancel_has_empty_side_and_zero_fills() {
        let update = OrderUpdate {
            event_type: "orderTradeUpdate".into(),
            event_time: 3_000_000,
            order: OrderUpdateData::Cancel(CancelOrderData {
                common: OrderUpdateCommon {
                    symbol: "SOL-USD".into(),
                    order_id: 55,
                    client_order_id: Some(serde_json::json!(7)),
                    status: "CANCELED".into(),
                    execution_type: "CANCELED".into(),
                    transaction_time: 2_999_999,
                    tx_hash: "0x2".into(),
                    user_address: ADDR.into(),
                },
            }),
        };
        let flat = update.to_flat_json();

        assert_eq!(flat["status"], "CANCELED");
        assert_eq!(flat["side"], "");
        assert_eq!(flat["lastFilledQty"], "0");
        assert_eq!(flat["lastFilledPrice"], "0");
    }

    // ── Non-order messages ────────────────────────────────────────────────────

    #[test]
    fn deserialize_book_ticker() {
        let json = r#"{"e":"bookTicker","u":12345,"s":"BTC-USD","b":"49999.0","B":"0.5","a":"50001.0","A":"0.3","T":1000}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ServerMessage::BookTicker(_)));
    }

    #[test]
    fn deserialize_mark_price() {
        let json = r#"{"e":"markPriceUpdate","E":1000,"s":"BTC-USD","p":"50000.0","r":"0.0001"}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ServerMessage::MarkPrice(_)));
    }

    #[test]
    fn status_message_parses_as_result() {
        // The connection status message has no id/result/error fields that conflict,
        // so it matches MethodResult (all-optional fields) before Unknown.
        let json = r#"{"e":"status","connected":true}"#;
        let msg: ServerMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ServerMessage::Result(_)));
    }
}
