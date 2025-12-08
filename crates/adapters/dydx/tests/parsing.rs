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

//! Data parsing tests for dYdX adapter using canonical fixtures.

use nautilus_core::UnixNanos;
use nautilus_dydx::{
    common::testing::load_json_result_fixture,
    http::{
        models::{
            CandlesResponse, FillsResponse, MarketsResponse, Order, OrderbookResponse,
            SubaccountResponse, TradesResponse, TransfersResponse,
        },
        parse::parse_instrument_any,
    },
};
use nautilus_model::{enums::OrderSide, instruments::Instrument};
use rstest::rstest;
use serde_json::Value;

fn load_json(filename: &str) -> Value {
    load_json_result_fixture(filename)
}

#[rstest]
fn test_parse_perpetual_markets() {
    let json = load_json("http_get_perpetual_markets.json");
    let response: MarketsResponse = serde_json::from_value(json).expect("Failed to parse markets");

    assert_eq!(response.markets.len(), 3);
    assert!(response.markets.contains_key("BTC-USD"));
    assert!(response.markets.contains_key("ETH-USD"));
    assert!(response.markets.contains_key("SOL-USD"));

    let btc = response.markets.get("BTC-USD").unwrap();
    assert_eq!(btc.ticker, "BTC-USD");
    assert_eq!(btc.clob_pair_id, 0);
    assert_eq!(btc.atomic_resolution, -10);
}

#[rstest]
fn test_parse_instrument_from_market() {
    let json = load_json("http_get_perpetual_markets.json");
    let response: MarketsResponse = serde_json::from_value(json).expect("Failed to parse markets");
    let btc = response.markets.get("BTC-USD").unwrap();

    let ts_init = UnixNanos::default();
    let instrument =
        parse_instrument_any(btc, None, None, ts_init).expect("Failed to parse instrument");

    assert_eq!(instrument.id().symbol.as_str(), "BTC-USD-PERP");
    assert_eq!(instrument.id().venue.as_str(), "DYDX");
}

#[rstest]
fn test_parse_orderbook_response() {
    let json = load_json("http_get_orderbook.json");
    let response: OrderbookResponse =
        serde_json::from_value(json).expect("Failed to parse orderbook");

    assert_eq!(response.bids.len(), 5);
    assert_eq!(response.asks.len(), 5);

    let best_bid = &response.bids[0];
    assert_eq!(best_bid.price.to_string(), "89947");
    assert_eq!(best_bid.size.to_string(), "0.0002");

    let best_ask = &response.asks[0];
    assert_eq!(best_ask.price.to_string(), "89958");
    assert_eq!(best_ask.size.to_string(), "0.1177");
}

#[rstest]
fn test_parse_trades_response() {
    let json = load_json("http_get_trades.json");
    let response: TradesResponse = serde_json::from_value(json).expect("Failed to parse trades");

    assert_eq!(response.trades.len(), 3);

    let first_trade = &response.trades[0];
    assert_eq!(first_trade.id, "03f89a550000000200000002");
    assert_eq!(first_trade.side, OrderSide::Buy);
    assert_eq!(first_trade.price.to_string(), "89942");
    assert_eq!(first_trade.size.to_string(), "0.0001");
}

#[rstest]
fn test_parse_candles_response() {
    let json = load_json("http_get_candles.json");
    let response: CandlesResponse = serde_json::from_value(json).expect("Failed to parse candles");

    assert_eq!(response.candles.len(), 3);

    let first_candle = &response.candles[0];
    assert_eq!(first_candle.ticker, "BTC-USD");
    assert_eq!(first_candle.open.to_string(), "89934");
    assert_eq!(first_candle.high.to_string(), "89970");
    assert_eq!(first_candle.low.to_string(), "89911");
    assert_eq!(first_candle.close.to_string(), "89941");
}

#[rstest]
fn test_parse_subaccount_response() {
    let json = load_json("http_get_subaccount.json");
    let response: SubaccountResponse =
        serde_json::from_value(json).expect("Failed to parse subaccount");

    let subaccount = &response.subaccount;
    assert_eq!(subaccount.subaccount_number, 0);
    assert_eq!(subaccount.equity.to_string(), "45.201296");
    assert_eq!(subaccount.free_collateral.to_string(), "45.201296");
    assert!(subaccount.margin_enabled);
    assert_eq!(subaccount.open_perpetual_positions.len(), 0);
}

#[rstest]
fn test_parse_orders_response() {
    let json = load_json("http_get_orders.json");
    let response: Vec<Order> = serde_json::from_value(json).expect("Failed to parse orders");

    assert_eq!(response.len(), 3);

    let first_order = &response[0];
    assert_eq!(first_order.id, "0f0981cb-152e-57d3-bea9-4d8e0dd5ed35");
    assert_eq!(first_order.side, OrderSide::Buy);
    assert_eq!(first_order.order_type, "LIMIT");
    assert!(first_order.reduce_only);

    let second_order = &response[1];
    assert_eq!(second_order.side, OrderSide::Sell);
    assert!(!second_order.reduce_only);
}

#[rstest]
fn test_parse_fills_response() {
    let json = load_json("http_get_fills.json");
    let response: FillsResponse = serde_json::from_value(json).expect("Failed to parse fills");

    assert_eq!(response.fills.len(), 3);

    let first_fill = &response.fills[0];
    assert_eq!(first_fill.id, "6450e369-1dc3-5229-8dc2-fb3b5d1cf2ab");
    assert_eq!(first_fill.side, OrderSide::Buy);
    assert_eq!(first_fill.market, "BTC-USD");
    assert_eq!(first_fill.price.to_string(), "105117");
}

#[rstest]
fn test_parse_transfers_response() {
    let json = load_json("http_get_transfers.json");
    let response: TransfersResponse =
        serde_json::from_value(json).expect("Failed to parse transfers");

    assert_eq!(response.transfers.len(), 1);

    let deposit = &response.transfers[0];
    assert_eq!(deposit.transfer_type, "DEPOSIT");
    assert_eq!(deposit.asset, "USDC");
    assert_eq!(deposit.amount.to_string(), "45.334703");
}
