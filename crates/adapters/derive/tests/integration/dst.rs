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

use std::time::{Duration, UNIX_EPOCH};

use alloy::signers::local::PrivateKeySigner;
use futures_util::{SinkExt, StreamExt};
use nautilus_core::UnixNanos;
use nautilus_derive::{
    common::{enums::DeriveEnvironment, parse::parse_derive_instrument_any},
    http::{DeriveHttpClient, query::DeriveCancelParams},
    signing::{auth::build_ws_login_at, encoding::utc_now_ms, nonce::NonceManager},
    websocket::{
        DeriveWebSocketClient, DeriveWsCredentials, DeriveWsMessage,
        parse::{parse_ticker_msg, parse_ticker_quote},
    },
};
use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    instruments::Instrument,
    types::{Price, Quantity},
};
use nautilus_network::{dst::net::TcpListener, websocket::TransportBackend};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const WALLET: &str = "0x0000000000000000000000000000000000001234";
const SESSION_KEY: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";

#[madsim::test]
async fn signing_uses_virtual_wall_clock_and_monotonic_nonces() {
    let clock = madsim::time::TimeHandle::try_current().unwrap();
    let now = clock.now_time().duration_since(UNIX_EPOCH).unwrap();
    let manager = NonceManager::new();
    let first = manager.next_nonce(WALLET, 75101).unwrap();
    let second = manager.next_nonce(WALLET, 75101).unwrap();
    let milliseconds = utc_now_ms().unwrap();
    madsim::time::sleep(Duration::from_secs(2)).await;
    let later_time = clock.now_time().duration_since(UNIX_EPOCH).unwrap();
    let later = manager.next_nonce(WALLET, 75101).unwrap();

    assert_eq!(first, u64::try_from(now.as_nanos()).unwrap());
    assert_eq!(second, first + 1);
    assert_eq!(milliseconds, u64::try_from(now.as_millis()).unwrap());
    assert_eq!(utc_now_ms().unwrap(), milliseconds + 2000);
    assert_eq!(later, u64::try_from(later_time.as_nanos()).unwrap());
}

#[rstest]
#[case::ticker(19101, "ticker_slim.ETH-PERP.1000")]
#[case::book(19102, "orderbook.ETH-PERP.1.10")]
#[case::trades(19103, "trades.perp.ETH")]
#[madsim::test]
async fn public_subscriptions_pin_exact_request_bytes(
    #[case] port: u16,
    #[case] topic: &'static str,
) {
    madsim::time::timeout(Duration::from_secs(5), async {
        let address = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&address).await.unwrap();
        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let text = socket.next().await.unwrap().unwrap().into_text().unwrap();
            socket.send(Message::text(json!({"id": 1, "result": {"current_subscriptions": [topic], "status": {topic: "ok"}}}).to_string())).await.unwrap();

            if port == 19101 {
                let data: Value = serde_json::from_str(include_str!("../../test_data/perps/ws_ticker_slim_eth.json")).unwrap();
                socket.send(Message::text(json!({"method": "subscription", "params": {"channel": topic, "data": data}}).to_string())).await.unwrap();
            }

            (socket, text.to_string())
        });

        let mut client = DeriveWebSocketClient::new(Some(format!("ws://{address}")), DeriveEnvironment::Testnet, TransportBackend::Tungstenite, None);
        client.connect().await.unwrap();
        match port {
            19101 => client.subscribe_ticker("ETH-PERP", "1000").await.unwrap(),
            19102 => client.subscribe_orderbook("ETH-PERP", "1", "10").await.unwrap(),
            19103 => client.subscribe_trades("perp", "ETH").await.unwrap(),
            _ => unreachable!(),
        }

        if port == 19101 {
            let payload = loop {
                if let Some(DeriveWsMessage::Subscription(payload)) = client.next_event().await { break payload; }
            };

            let ticker = parse_ticker_msg(&payload).unwrap();
            let quote = parse_ticker_quote(&ticker, 2, 3, UnixNanos::from(751)).unwrap();
            let expected = QuoteTick::new(InstrumentId::from("ETH-PERP.DERIVE"), Price::from("1992.36"), Price::from("1992.37"), Quantity::from("1.505"), Quantity::from("1.505"), UnixNanos::from(1779953796714000000), UnixNanos::from(751));
            assert_eq!(quote, expected);
            println!("DST_DOMAIN={}", serde_json::to_string(&quote).unwrap());
        }

        let (socket, frame) = peer.await.unwrap();
        client.disconnect().await.unwrap();
        drop(socket);
        assert_eq!(frame, format!(r#"{{"jsonrpc":"2.0","id":1,"method":"subscribe","params":{{"channels":["{topic}"]}}}}"#));
        println!("DST_TRACE={frame}");
    }).await.unwrap();
}

#[madsim::test]
async fn private_login_and_cancel_pin_signature_and_request_fields() {
    madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:19104").await.unwrap();
        let peer = madsim::task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let login: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().into_text().unwrap().as_str()).unwrap();
            socket.send(Message::text(r#"{"id":1,"result":[75102]}"#)).await.unwrap();
            let cancel = socket.next().await.unwrap().unwrap().into_text().unwrap().to_string();
            socket.send(Message::text(r#"{"id":2,"result":{}}"#)).await.unwrap();
            (socket, login, cancel)
        });

        let credentials = DeriveWsCredentials::new(WALLET, SESSION_KEY).unwrap();
        let mut client = DeriveWebSocketClient::with_credentials(Some("ws://127.0.0.1:19104".to_string()), DeriveEnvironment::Testnet, TransportBackend::Tungstenite, None, credentials, None, None);
        client.connect().await.unwrap();
        assert!(client.is_authenticated());
        client.execution_handle().cancel_order(&DeriveCancelParams::new(75102, "ETH-PERP", "native-control-order")).await.unwrap();
        let (socket, login, cancel) = peer.await.unwrap();
        client.disconnect().await.unwrap();
        drop(socket);
        let timestamp: u64 = login["params"]["timestamp"].as_str().unwrap().parse().unwrap();
        let signer: PrivateKeySigner = SESSION_KEY.parse().unwrap();
        let expected = build_ws_login_at(WALLET, &signer, timestamp).unwrap();
        assert_eq!(login, json!({"jsonrpc": "2.0", "id": 1, "method": "public/login", "params": {"wallet": expected.wallet, "timestamp": expected.timestamp, "signature": expected.signature.expose_secret()}}));
        assert_eq!(cancel, r#"{"jsonrpc":"2.0","id":2,"method":"private/cancel","params":{"instrument_name":"ETH-PERP","order_id":"native-control-order","subaccount_id":75102}}"#);
        println!("DST_TRACE={login}|{cancel}");
    }).await.unwrap();
}

#[madsim::test]
async fn reconnect_replays_sorted_subscriptions_before_publishing_ready() {
    madsim::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:19105").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let mut frames = Vec::new();

            for generation in 1..=2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let request: Value = serde_json::from_str(message.as_str()).unwrap();
                let channels = request["params"]["channels"].clone();
                let mut status = serde_json::Map::new();
                for channel in channels.as_array().unwrap() { status.insert(channel.as_str().unwrap().to_string(), json!("ok")); }
                socket.send(Message::text(json!({"id": request["id"], "result": {"current_subscriptions": channels, "status": status}}).to_string())).await.unwrap();
                frames.push(message.to_string());

                if generation == 1 { socket.close(None).await.unwrap(); }
                else { return (socket, frames); }
            }

            unreachable!()
        });

        let mut client = DeriveWebSocketClient::new(Some("ws://127.0.0.1:19105".to_string()), DeriveEnvironment::Testnet, TransportBackend::Tungstenite, None);
        client.connect().await.unwrap();
        client.subscribe_channels(vec!["ticker_slim.ETH-PERP.1000", "ticker_slim.BTC-PERP.1000"]).await.unwrap();
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(DeriveWsMessage::SessionRecoveryFailed(reason)) => panic!("recovery failed: {reason}"),
                Some(_) => {},
                None => panic!("stream closed before recovery"),
            }
        }

        let (socket, frames) = peer.await.unwrap();
        assert!(client.is_active());
        assert_eq!(client.subscription_count(), 2);
        client.disconnect().await.unwrap();
        drop(socket);
        assert_eq!(frames, [r#"{"jsonrpc":"2.0","id":1,"method":"subscribe","params":{"channels":["ticker_slim.ETH-PERP.1000","ticker_slim.BTC-PERP.1000"]}}"#, r#"{"jsonrpc":"2.0","id":2,"method":"subscribe","params":{"channels":["ticker_slim.BTC-PERP.1000","ticker_slim.ETH-PERP.1000"]}}"#]);
        println!("DST_TRACE={}", frames.join("|"));
    }).await.unwrap();
}

#[madsim::test]
async fn public_http_instrument_preserves_request_and_domain_output() {
    Box::pin(madsim::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:19106").await.unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];

            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).await.unwrap(), 1);
                request.push(byte[0]);
            }

            let headers = String::from_utf8(request.clone()).unwrap();
            let length: usize = headers.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length: ").map(str::to_string)).unwrap().parse().unwrap();
            let mut body = vec![0_u8; length];
            stream.read_exact(&mut body).await.unwrap();
            request.extend(body);
            let instrument: Value = serde_json::from_str(include_str!("../../test_data/perps/instrument_eth.json")).unwrap();
            let response = json!({"id": 1, "result": instrument}).to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            String::from_utf8(request).unwrap()
        });

        let client = DeriveHttpClient::new("http://127.0.0.1:19106", Some(2), None, None).unwrap();
        let native = client.get_instrument("ETH-PERP").await.unwrap();
        let instrument = parse_derive_instrument_any(&native, UnixNanos::from(752)).unwrap().unwrap();
        let request = peer.await.unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /public/get_instrument HTTP/1.1\r\n"));
        assert_eq!(body, json!({"instrument_name": "ETH-PERP"}).to_string());
        assert_eq!(instrument.id(), InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(instrument.price_precision(), 2);
        assert_eq!(instrument.size_precision(), 3);
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.001"));
        println!("DST_TRACE={request:?}");
        println!("DST_DOMAIN={}", serde_json::to_string(&instrument).unwrap());
    })).await.unwrap();
}
