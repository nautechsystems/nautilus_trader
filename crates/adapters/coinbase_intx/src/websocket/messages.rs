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

use chrono::{DateTime, Utc};
use nautilus_model::{
    data::{Data, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas},
    events::OrderEventAny,
    instruments::InstrumentAny,
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{CoinbaseIntxWsChannel, WsMessageType, WsOperation};
use crate::common::enums::{CoinbaseIntxInstrumentType, CoinbaseIntxSide};

#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Data(Data),
    DataVec(Vec<Data>),
    Deltas(OrderBookDeltas),
    Instrument(InstrumentAny),
    OrderEvent(OrderEventAny),
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    MarkAndIndex((MarkPriceUpdate, IndexPriceUpdate)),
}

#[derive(Debug, Serialize)]
pub struct CoinbaseIntxSubscription {
    #[serde(rename = "type")]
    pub op: WsOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_ids: Option<Vec<Ustr>>,
    pub channels: Vec<CoinbaseIntxWsChannel>,
    pub time: String,
    pub key: Ustr,
    pub passphrase: Ustr,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum CoinbaseIntxWsMessage {
    Reject(CoinbaseIntxWsRejectMsg),
    Confirmation(CoinbaseIntxWsConfirmationMsg),
    Instrument(CoinbaseIntxWsInstrumentMsg),
    Funding(CoinbaseIntxWsFundingMsg),
    Risk(CoinbaseIntxWsRiskMsg),
    BookSnapshot(CoinbaseIntxWsOrderBookSnapshotMsg),
    BookUpdate(CoinbaseIntxWsOrderBookUpdateMsg),
    Quote(CoinbaseIntxWsQuoteMsg),
    Trade(CoinbaseIntxWsTradeMsg),
    CandleSnapshot(CoinbaseIntxWsCandleSnapshotMsg),
    CandleUpdate(CoinbaseIntxWsCandleUpdateMsg),
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsRejectMsg {
    pub message: String,
    pub reason: String,
    pub channel: CoinbaseIntxWsChannel,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsConfirmationMsg {
    pub channels: Vec<CoinbaseIntxWsChannelDetails>,
    pub authenticated: bool,
    pub channel: CoinbaseIntxWsChannel,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsChannelDetails {
    pub name: CoinbaseIntxWsChannel,
    pub product_ids: Option<Vec<Ustr>>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsInstrumentMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub instrument_type: CoinbaseIntxInstrumentType,
    pub instrument_mode: String,
    pub base_asset_name: String,
    pub quote_asset_name: String,
    pub base_increment: String,
    pub quote_increment: String,
    pub avg_daily_quantity: String,
    pub avg_daily_volume: String,
    pub total30_day_quantity: String,
    pub total30_day_volume: String,
    pub total24_hour_quantity: String,
    pub total24_hour_volume: String,
    pub base_imf: String,
    pub min_quantity: String,
    pub position_size_limit: Option<String>,
    pub position_notional_limit: Option<String>,
    pub funding_interval: Option<String>,
    pub trading_state: String,
    pub last_updated_time: DateTime<Utc>,
    pub default_initial_margin: Option<String>,
    pub base_asset_multiplier: String,
    pub underlying_type: CoinbaseIntxInstrumentType,
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsFundingMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub funding_rate: String,
    pub is_final: bool,
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsRiskMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub limit_up: String,
    pub limit_down: String,
    pub index_price: String,
    pub mark_price: String,
    pub settlement_price: String,
    pub open_interest: String,
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsOrderBookSnapshotMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub bids: Vec<[String; 2]>, // [price, size]
    pub asks: Vec<[String; 2]>, // [price, size]
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsOrderBookUpdateMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub changes: Vec<[String; 3]>, // [side, price, size]
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsQuoteMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub bid_price: String,
    pub bid_qty: String,
    pub ask_price: String,
    pub ask_qty: String,
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsTradeMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub match_id: String,
    pub trade_price: String,
    pub trade_qty: String,
    pub aggressor_side: CoinbaseIntxSide,
    pub sequence: u64,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsCandle {
    pub start: DateTime<Utc>,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsCandleSnapshotMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub granularity: Ustr,
    pub candles: Vec<CoinbaseIntxWsCandle>,
    pub sequence: u64,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseIntxWsCandleUpdateMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channel: CoinbaseIntxWsChannel,
    pub product_id: Ustr,
    pub start: DateTime<Utc>,
    #[serde(default)]
    pub open: Option<String>,
    #[serde(default)]
    pub high: Option<String>,
    #[serde(default)]
    pub low: Option<String>,
    #[serde(default)]
    pub close: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    pub sequence: u64,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_parse_asset_model() {
        let json_data = load_test_json("ws_instruments.json");
        let parsed: CoinbaseIntxWsInstrumentMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.product_id, "ETH-PERP");
        assert_eq!(parsed.message_type, WsMessageType::Snapshot);
        assert_eq!(parsed.channel, CoinbaseIntxWsChannel::Instruments);
        assert_eq!(parsed.instrument_type, CoinbaseIntxInstrumentType::Perp);
        assert_eq!(parsed.instrument_mode, "standard");
        assert_eq!(parsed.base_asset_name, "ETH");
        assert_eq!(parsed.quote_asset_name, "USDC");
        assert_eq!(parsed.base_increment, "0.0001");
        assert_eq!(parsed.quote_increment, "0.01");
        assert_eq!(parsed.avg_daily_quantity, "229061.15400333333");
        assert_eq!(parsed.avg_daily_volume, "5.33931093731498E8");
        assert_eq!(parsed.total30_day_quantity, "6871834.6201");
        assert_eq!(parsed.total30_day_volume, "1.601793281194494E10");
        assert_eq!(parsed.total24_hour_quantity, "116705.0261");
        assert_eq!(parsed.total24_hour_volume, "2.22252453944151E8");
        assert_eq!(parsed.base_imf, "0.05");
        assert_eq!(parsed.min_quantity, "0.0001");
        assert_eq!(parsed.position_size_limit, Some("5841.0594".to_string()));
        assert_eq!(parsed.position_notional_limit, Some("70000000".to_string()));
        assert_eq!(parsed.funding_interval, Some("3600000000000".to_string()));
        assert_eq!(parsed.trading_state, "trading");
        assert_eq!(
            parsed.last_updated_time.to_rfc3339(),
            "2025-03-14T22:00:00+00:00"
        );
        assert_eq!(parsed.default_initial_margin, Some("0.2".to_string()));
        assert_eq!(parsed.base_asset_multiplier, "1.0");
        assert_eq!(parsed.underlying_type, CoinbaseIntxInstrumentType::Spot);
        assert_eq!(parsed.sequence, 0);
        assert_eq!(parsed.time.to_rfc3339(), "2025-03-14T22:59:53.373+00:00");
    }

    #[rstest]
    fn test_parse_ws_trade_msg() {
        let json_data = load_test_json("ws_match.json");
        let parsed: CoinbaseIntxWsTradeMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.product_id, "BTC-PERP");
        assert_eq!(parsed.message_type, WsMessageType::Update);
        assert_eq!(parsed.channel, CoinbaseIntxWsChannel::Match);
        assert_eq!(parsed.match_id, "423596942694547460");
        assert_eq!(parsed.trade_price, "84374");
        assert_eq!(parsed.trade_qty, "0.0213");
        assert_eq!(parsed.aggressor_side, CoinbaseIntxSide::Buy);
        assert_eq!(parsed.sequence, 0);
        assert_eq!(parsed.time.to_rfc3339(), "2025-03-14T23:03:01.189+00:00");
    }

    #[rstest]
    fn test_parse_ws_quote_msg() {
        let json_data = load_test_json("ws_quote.json");
        let parsed: CoinbaseIntxWsQuoteMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.product_id, "BTC-PERP");
        assert_eq!(parsed.message_type, WsMessageType::Snapshot);
        assert_eq!(parsed.channel, CoinbaseIntxWsChannel::Level1);
        assert_eq!(parsed.bid_price, "84368.5");
        assert_eq!(parsed.bid_qty, "2.608");
        assert_eq!(parsed.ask_price, "84368.6");
        assert_eq!(parsed.ask_qty, "2.9453");
        assert_eq!(parsed.sequence, 0);
        assert_eq!(parsed.time.to_rfc3339(), "2025-03-14T23:05:39.533+00:00");
    }

    #[rstest]
    fn test_parse_ws_order_book_snapshot_msg() {
        let json_data = load_test_json("ws_book_snapshot.json");
        let parsed: CoinbaseIntxWsOrderBookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.product_id, "BTC-PERP");
        assert_eq!(parsed.message_type, WsMessageType::Snapshot);
        assert_eq!(parsed.channel, CoinbaseIntxWsChannel::Level2);
        assert_eq!(parsed.sequence, 0);
        assert_eq!(parsed.time.to_rfc3339(), "2025-03-14T23:09:43.993+00:00");

        assert_eq!(parsed.bids.len(), 50);
        assert_eq!(parsed.asks.len(), 50);

        assert_eq!(parsed.bids[0][0], "84323.6");
        assert_eq!(parsed.bids[0][1], "4.9466");

        assert_eq!(parsed.bids[49][0], "84296.2");
        assert_eq!(parsed.bids[49][1], "0.0237");

        assert_eq!(parsed.asks[0][0], "84323.7");
        assert_eq!(parsed.asks[0][1], "2.6944");

        assert_eq!(parsed.asks[49][0], "84346.9");
        assert_eq!(parsed.asks[49][1], "0.3257");
    }

    #[rstest]
    fn test_parse_ws_order_book_update_msg() {
        let json_data = load_test_json("ws_book_update.json");
        let parsed: CoinbaseIntxWsOrderBookUpdateMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.product_id, "BTC-PERP");
        assert_eq!(parsed.message_type, WsMessageType::Update);
        assert_eq!(parsed.channel, CoinbaseIntxWsChannel::Level2);
        assert_eq!(parsed.sequence, 1);
        assert_eq!(parsed.time.to_rfc3339(), "2025-03-14T23:09:44.095+00:00");

        assert_eq!(parsed.changes.len(), 2);

        assert_eq!(parsed.changes[0][0], "BUY"); // side
        assert_eq!(parsed.changes[0][1], "84296.2"); // price
        assert_eq!(parsed.changes[0][2], "0"); // size (0 means delete)

        assert_eq!(parsed.changes[1][0], "BUY"); // side
        assert_eq!(parsed.changes[1][1], "84296.3"); // price
        assert_eq!(parsed.changes[1][2], "0.1779"); // size
    }

    #[rstest]
    fn test_parse_ws_candle_snapshot_msg() {
        let json_data = load_test_json("ws_candles.json");
        let parsed: CoinbaseIntxWsCandleSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(parsed.granularity, "ONE_MINUTE");
        assert_eq!(parsed.sequence, 0);
        assert_eq!(parsed.candles.len(), 1);

        let candle = &parsed.candles[0];
        assert_eq!(candle.start.to_rfc3339(), "2025-03-14T23:14:00+00:00");
        assert_eq!(candle.open, "1921.71");
        assert_eq!(candle.high, "1921.71");
        assert_eq!(candle.low, "1919.87");
        assert_eq!(candle.close, "1919.87");
        assert_eq!(candle.volume, "11.2803");
    }
}
