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

#![cfg(all(feature = "simulation", madsim))]

use std::{path::PathBuf, time::Duration};

use futures_util::{SinkExt, StreamExt};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, PositionSide, TimeInForce, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use nautilus_network::{dst::net::TcpListener, websocket::TransportBackend};
use nautilus_okx::{
    common::{
        consts::OKX_NAUTILUS_BROKER_ID,
        credential::Credential,
        enums::{OKXInstrumentType, OKXTradeMode},
        models::OKXInstrument,
        parse::parse_instrument_any,
    },
    http::client::OKXResponse,
    websocket::{
        client::OKXWebSocketClient,
        enums::{OKXWsChannel, OKXWsOperation},
        messages::{OKXSubscription, OKXSubscriptionArg},
    },
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use ustr::Ustr;

fn subscribe_frame(channel: &OKXWsChannel, inst_id: &str) -> String {
    subscribe_frames(channel, &[inst_id])
}

fn subscribe_frames(channel: &OKXWsChannel, inst_ids: &[&str]) -> String {
    serde_json::to_string(&OKXSubscription {
        op: OKXWsOperation::Subscribe,
        args: inst_ids
            .iter()
            .map(|inst_id| OKXSubscriptionArg {
                channel: channel.clone(),
                inst_type: None,
                inst_family: None,
                inst_id: Some(Ustr::from(inst_id)),
            })
            .collect(),
    })
    .expect("subscribe frame")
}

fn public_client(url: &str) -> OKXWebSocketClient {
    OKXWebSocketClient::new(
        Some(url.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        TransportBackend::Tungstenite,
        None,
    )
    .expect("websocket client")
}

fn private_client(url: &str) -> OKXWebSocketClient {
    OKXWebSocketClient::new(
        Some(url.to_string()),
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        None,
        Some(30),
        None,
        TransportBackend::Tungstenite,
        None,
    )
    .expect("websocket client")
}

fn load_spot_instruments() -> Vec<InstrumentAny> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data/http_get_instruments_spot.json");
    let content = std::fs::read_to_string(&path).expect("spot instrument fixture must be readable");
    let response: OKXResponse<OKXInstrument> =
        serde_json::from_str(&content).expect("valid spot instrument fixture");
    response
        .data
        .iter()
        .filter_map(|raw| {
            parse_instrument_any(raw, None, None, None, None, UnixNanos::default())
                .ok()
                .flatten()
        })
        .collect()
}

fn parse_frame(message: Message) -> Value {
    let text = message.into_text().expect("text frame");
    serde_json::from_str(text.as_str()).expect("valid JSON frame")
}

#[rstest]
#[case::quotes(OKXWsChannel::BboTbt)]
#[case::trades(OKXWsChannel::Trades)]
#[case::books(OKXWsChannel::Books)]
#[madsim::test]
async fn public_spot_subscribe_sends_exact_wire_frame(#[case] channel: OKXWsChannel) {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18090").await.unwrap();
        let expected = subscribe_frame(&channel, "BTC-USDT");

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.close(None).await.ok();
            message
        });

        let mut client = public_client("ws://127.0.0.1:18090");
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        match channel {
            OKXWsChannel::BboTbt => client.subscribe_quotes(instrument_id).await.unwrap(),
            OKXWsChannel::Trades => client.subscribe_trades(instrument_id, false).await.unwrap(),
            OKXWsChannel::Books => client.subscribe_book(instrument_id).await.unwrap(),
            other => unreachable!("unexpected channel {other:?}"),
        }

        let message = peer.await.unwrap();
        client.close().await.unwrap();

        assert_eq!(message, Message::text(expected));
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn business_spot_bar_subscribe_sends_exact_wire_frame() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18091").await.unwrap();
        let expected = subscribe_frame(&OKXWsChannel::Candle1Minute, "BTC-USDT");

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.close(None).await.ok();
            message
        });

        let mut client = public_client("ws://127.0.0.1:18091");
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();
        client
            .subscribe_bars(BarType::from("BTC-USDT.OKX-1-MINUTE-LAST-EXTERNAL"))
            .await
            .unwrap();

        let message = peer.await.unwrap();
        client.close().await.unwrap();

        assert_eq!(message, Message::text(expected));
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn reconnect_resubscribes_multi_instrument_quotes_in_topic_order() {
    madsim::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:18092").await.unwrap();
        let eth = subscribe_frame(&OKXWsChannel::BboTbt, "ETH-USDT");
        let btc = subscribe_frame(&OKXWsChannel::BboTbt, "BTC-USDT");
        let reconnect = subscribe_frames(&OKXWsChannel::BboTbt, &["BTC-USDT", "ETH-USDT"]);

        let peer = madsim::task::spawn(async move {
            let mut frames = Vec::new();

            for generation in 1..=2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                frames.push(socket.next().await.unwrap().unwrap());
                if generation == 1 {
                    frames.push(socket.next().await.unwrap().unwrap());
                    socket.close(None).await.unwrap();
                }
            }

            frames
        });

        let mut client = public_client("ws://127.0.0.1:18092");
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();
        client
            .subscribe_quotes(InstrumentId::from("ETH-USDT.OKX"))
            .await
            .unwrap();
        client
            .subscribe_quotes(InstrumentId::from("BTC-USDT.OKX"))
            .await
            .unwrap();

        let frames = peer.await.unwrap();
        client.close().await.unwrap();

        assert_eq!(
            frames,
            vec![
                Message::text(eth),
                Message::text(btc),
                Message::text(reconnect),
            ]
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_connect_sends_deterministic_login_frame() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18093").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            message
        });

        let mut client = private_client("ws://127.0.0.1:18093");
        let expected_timestamp = get_atomic_clock_realtime()
            .get_time_ns()
            .as_seconds()
            .to_string();
        client.connect().await.unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(frame["op"], "login");
        let arg = &frame["args"][0];
        assert_eq!(arg["apiKey"], "api_key");
        assert_eq!(arg["passphrase"], "passphrase");
        let timestamp = arg["timestamp"].as_str().expect("timestamp string");
        assert_eq!(timestamp, expected_timestamp);

        let credential = Credential::new(
            "api_key".to_string(),
            "api_secret".to_string(),
            "passphrase".to_string(),
        );
        let expected_sign = credential.sign(&expected_timestamp, "GET", "/users/self/verify", "");
        assert_eq!(arg["sign"].as_str().unwrap(), expected_sign);
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_order_submit_sends_exact_wire_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18094").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18094");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .submit_order(
                TraderId::from("TRADER-001"),
                StrategyId::from("STRATEGY-001"),
                InstrumentId::from("BTC-USD.OKX"),
                OKXTradeMode::Cash,
                ClientOrderId::from("Odstspotlimitorder0001"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.25"),
                Some(TimeInForce::Gtc),
                Some(Price::from("65000.1")),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "order",
                "args": [{
                    "instIdCode": 10_459,
                    "tdMode": "cash",
                    "ccy": "USD",
                    "clOrdId": "Odstspotlimitorder0001",
                    "side": "buy",
                    "ordType": "limit",
                    "sz": "0.25",
                    "px": "65000.1",
                    "tag": OKX_NAUTILUS_BROKER_ID,
                }],
            })
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_batch_submit_preserves_input_order_on_wire() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18095").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18095");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .batch_submit_orders(vec![
                spot_limit_order("Zbatchlastfirst0000001", OrderSide::Sell, "65002.3"),
                spot_limit_order("Abatchmiddleorder00001", OrderSide::Buy, "64999.0"),
                spot_limit_order("Mbatchendorder0000001", OrderSide::Buy, "65001.2"),
            ])
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "batch-orders",
                "args": [
                    {
                        "instIdCode": 10_459,
                        "tdMode": "cash",
                        "ccy": "USD",
                        "clOrdId": "Zbatchlastfirst0000001",
                        "side": "sell",
                        "ordType": "limit",
                        "sz": "0.25",
                        "px": "65002.3",
                        "tag": OKX_NAUTILUS_BROKER_ID,
                    },
                    {
                        "instIdCode": 10_459,
                        "tdMode": "cash",
                        "ccy": "USD",
                        "clOrdId": "Abatchmiddleorder00001",
                        "side": "buy",
                        "ordType": "limit",
                        "sz": "0.25",
                        "px": "64999.0",
                        "tag": OKX_NAUTILUS_BROKER_ID,
                    },
                    {
                        "instIdCode": 10_459,
                        "tdMode": "cash",
                        "ccy": "USD",
                        "clOrdId": "Mbatchendorder0000001",
                        "side": "buy",
                        "ordType": "limit",
                        "sz": "0.25",
                        "px": "65001.2",
                        "tag": OKX_NAUTILUS_BROKER_ID,
                    },
                ],
            })
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_order_amend_sends_exact_wire_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18096").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18096");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .modify_order(
                TraderId::from("TRADER-001"),
                StrategyId::from("STRATEGY-001"),
                InstrumentId::from("BTC-USD.OKX"),
                Some(ClientOrderId::from("Odstspotamendorder0001")),
                Some(Price::from("64999.5")),
                Some(Quantity::from("0.5")),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "amend-order",
                "args": [{
                    "instIdCode": 10_459,
                    "clOrdId": "Odstspotamendorder0001",
                    "newPx": "64999.5",
                    "newSz": "0.5",
                }],
            })
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_order_cancel_sends_exact_wire_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18097").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18097");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .cancel_order(
                TraderId::from("TRADER-001"),
                StrategyId::from("STRATEGY-001"),
                InstrumentId::from("BTC-USD.OKX"),
                Some(ClientOrderId::from("Odstspotcancelorder001")),
                None,
            )
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "cancel-order",
                "args": [{
                    "instIdCode": 10_459,
                    "clOrdId": "Odstspotcancelorder001",
                }],
            })
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_algo_submit_sends_exact_wire_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18098").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18098");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .submit_algo_order(
                TraderId::from("TRADER-001"),
                StrategyId::from("STRATEGY-001"),
                InstrumentId::from("BTC-USD.OKX"),
                OKXTradeMode::Cash,
                ClientOrderId::from("Odstalgoorder00000001"),
                OrderSide::Buy,
                OrderType::StopMarket,
                Quantity::from("0.25"),
                Some(Price::from("64000.0")),
                Some(TriggerType::LastPrice),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "order-algo",
                "args": [{
                    "instIdCode": 10_459,
                    "tdMode": "cash",
                    "clOrdId": "Odstalgoorder00000001",
                    "side": "buy",
                    "ordType": "trigger",
                    "sz": "0.25",
                    "triggerPx": "64000.0",
                    "triggerPxType": "last",
                    "tag": OKX_NAUTILUS_BROKER_ID,
                }],
            })
        );
    })
    .await
    .unwrap();
}

#[madsim::test]
async fn private_algo_cancel_sends_exact_wire_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:18099").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _login = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    r#"{"event":"login","code":"0","msg":"","connId":"dst-conn"}"#,
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap()
        });

        let mut client = private_client("ws://127.0.0.1:18099");
        client.cache_instruments(&load_spot_instruments());
        client.cache_inst_id_code(Ustr::from("BTC-USD"), 10_459);
        client.connect().await.unwrap();
        client.wait_until_active(5.0).await.unwrap();

        client
            .cancel_algo_order(
                TraderId::from("TRADER-001"),
                StrategyId::from("STRATEGY-001"),
                InstrumentId::from("BTC-USD.OKX"),
                Some(ClientOrderId::from("Odstalgocancel0000001")),
                None,
            )
            .await
            .unwrap();

        let frame = parse_frame(peer.await.unwrap());
        client.close().await.unwrap();

        assert_eq!(
            frame,
            json!({
                "id": "1",
                "op": "cancel-algos",
                "args": [{
                    "instIdCode": 10_459,
                    "algoClOrdId": "Odstalgocancel0000001",
                }],
            })
        );
    })
    .await
    .unwrap();
}

#[expect(clippy::type_complexity)]
fn spot_limit_order(
    client_order_id: &str,
    side: OrderSide,
    px: &str,
) -> (
    OKXInstrumentType,
    InstrumentId,
    OKXTradeMode,
    ClientOrderId,
    OrderSide,
    Option<PositionSide>,
    OrderType,
    Quantity,
    Option<Price>,
    Option<Price>,
    Option<bool>,
    Option<bool>,
    Option<String>,
    Option<bool>,
    Option<bool>,
    Option<bool>,
) {
    (
        OKXInstrumentType::Spot,
        InstrumentId::from("BTC-USD.OKX"),
        OKXTradeMode::Cash,
        ClientOrderId::from(client_order_id),
        side,
        None,
        OrderType::Limit,
        Quantity::from("0.25"),
        Some(Price::from(px)),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}
