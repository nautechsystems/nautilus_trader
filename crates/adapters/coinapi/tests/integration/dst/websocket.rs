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

#![allow(
    clippy::large_futures,
    reason = "simulation futures hold peer sockets and exceed the default size under madsim"
)]

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
#[cfg(not(all(feature = "simulation", madsim)))]
use nautilus_coinapi::config::CoinApiDataClientConfig;
use nautilus_coinapi::websocket::{
    client::{CoinApiWebSocketClient, CoinApiWebSocketConfig},
    error::Error,
    messages::{Channel, CoinApiWsMessage, DataType, ScopeChange, Subscription},
};
use nautilus_core::string::secret::SecretString;
use nautilus_network::{
    dst::{
        net::{TcpListener, TcpStream},
        task, time,
    },
    websocket::TransportBackend,
};
use rstest::rstest;
#[cfg(not(all(feature = "simulation", madsim)))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{
    WebSocketStream, accept_async, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};
use ustr::Ustr;

type Socket = WebSocketStream<TcpStream>;

const BTC: &str = "COINBASE_SPOT_BTC_USD";
const ETH: &str = "COINBASE_SPOT_ETH_USD";
const HELLO_BTC: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
const HELLO_BTC_ETH: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$","COINBASE_SPOT_ETH_USD$"]}"#;
const HELLO_ETH: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_ETH_USD$"]}"#;
const TRADE: &str = include_str!("../../../test_data/ws_trade.json");

const TRADES: Channel = Channel {
    data_type: DataType::Trade,
    period_id: None,
};

fn subscription(channel: Channel, symbol_id: &str) -> Subscription {
    Subscription {
        channel,
        symbol_id: symbol_id.into(),
    }
}

fn client(
    address: &str,
) -> (
    CoinApiWebSocketClient,
    tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>,
) {
    CoinApiWebSocketClient::new(
        SecretString::from("test-api-key"),
        CoinApiWebSocketConfig {
            url: format!("ws://{address}/v1/"),
            backend: TransportBackend::Tungstenite,
            ..CoinApiWebSocketConfig::default()
        },
    )
    .unwrap()
}

fn trade_for(symbol_id: &str) -> String {
    TRADE.replace(BTC, symbol_id)
}

fn trade_message(bytes: &str) -> CoinApiWsMessage {
    CoinApiWsMessage::Trade(serde_json::from_str(bytes).unwrap())
}

fn ohlcv_message(bytes: &str) -> CoinApiWsMessage {
    CoinApiWsMessage::Ohlcv(serde_json::from_str(bytes).unwrap())
}

fn ohlcv(period_id: &str) -> Channel {
    Channel {
        data_type: DataType::Ohlcv,
        period_id: Some(Ustr::from(period_id)),
    }
}

#[cfg(all(feature = "simulation", madsim))]
async fn listener() -> (TcpListener, String) {
    (
        TcpListener::bind("127.0.0.1:18993").await.unwrap(),
        "127.0.0.1:18993".into(),
    )
}

#[cfg(not(all(feature = "simulation", madsim)))]
async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    (listener, address)
}

async fn bounded<F: Future>(future: F) -> F::Output {
    time::timeout(Duration::from_secs(10), future)
        .await
        .unwrap()
}

// Returns the socket and its upgrade-request API key and referrer headers
#[allow(
    clippy::result_large_err,
    reason = "tungstenite fixes the handshake callback error type"
)]
async fn accept(listener: &TcpListener) -> (Socket, [String; 2]) {
    let (stream, _) = listener.accept().await.unwrap();
    let headers = Arc::new(Mutex::new([String::new(), String::new()]));
    let captured = Arc::clone(&headers);
    let socket = accept_hdr_async(stream, move |request: &Request, response: Response| {
        *captured.lock().unwrap() = ["x-coinapi-key", "x-referrer"]
            .map(|name| request.headers()[name].to_str().unwrap().to_owned());
        Ok(response)
    })
    .await
    .unwrap();

    let headers = headers.lock().unwrap().clone();
    (socket, headers)
}

async fn next_text(socket: &mut Socket) -> String {
    loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("expected text frame, was {other:?}"),
        }
    }
}

async fn closes_without_text(socket: &mut Socket) -> bool {
    loop {
        match socket.next().await {
            None | Some(Ok(Message::Close(_)) | Err(_)) => return true,
            Some(Ok(Message::Text(_))) => return false,
            Some(Ok(_)) => {}
        }
    }
}

async fn send(socket: &mut Socket, text: &str) {
    socket.send(Message::text(text)).await.unwrap();
}

async fn recv(rx: &mut tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>) -> CoinApiWsMessage {
    recv_within(rx, Duration::from_secs(5)).await
}

async fn recv_within(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>,
    timeout: Duration,
) -> CoinApiWsMessage {
    time::timeout(timeout, rx.recv()).await.unwrap().unwrap()
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn subscribe_pins_upgrade_headers_and_union_scope() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, headers) = accept(&listener).await;
            let mut frames = Vec::new();
            for _ in 0..3 {
                frames.push(next_text(&mut socket).await);
            }

            let extra_frame = time::timeout(Duration::from_millis(200), socket.next())
                .await
                .is_ok();
            let extra_connection = time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_ok();
            (headers, frames, extra_frame, extra_connection)
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, ETH)).await.unwrap();
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        client.subscribe(subscription(TRADES, ETH)).await.unwrap();
        client.subscribe(subscription(ohlcv("1MIN"), BTC)).await.unwrap();

        let (headers, frames, extra_frame, extra_connection) = peer.await.unwrap();
        client.close().await;

        assert_eq!(headers, ["test-api-key", "nautilus"]);
        assert_eq!(
            frames,
            [
                HELLO_ETH,
                HELLO_BTC_ETH,
                r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["ohlcv","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$","COINBASE_SPOT_ETH_USD$"],"subscribe_filter_period_id":["1MIN"]}"#,
            ]
        );
        assert!(!extra_frame);
        assert!(!extra_connection);
        assert!(matches!(rx.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    })
    .await;
}

// CoinAPI retires the whole connection when a scope lists a second book type
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn scope_keeps_one_book_type_and_resubscribes_only_subscribed_pairs() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let hello = next_text(&mut socket).await;
            let extra_frame = time::timeout(Duration::from_millis(200), socket.next())
                .await
                .is_ok();
            (hello, extra_frame)
        });

        let book = |data_type, symbol_id| {
            subscription(
                Channel {
                    data_type,
                    period_id: None,
                },
                symbol_id,
            )
        };

        let (client, _rx) = client(&address);
        client.subscribe(book(DataType::Book, BTC)).await.unwrap();
        let second_type = client.subscribe(book(DataType::Book5, ETH)).await.unwrap_err();
        let unsubscribed = client.resubscribe(book(DataType::Book, ETH), || {}).await.unwrap_err();
        let not_book = client.resubscribe(subscription(TRADES, BTC), || {}).await.unwrap_err();

        let (hello, extra_frame) = peer.await.unwrap();
        client.close().await;

        assert_eq!(
            hello,
            r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#
        );
        assert_eq!(
            second_type.to_string(),
            "Invalid CoinAPI WebSocket request: scope already streams book, so cannot add book5"
        );
        assert_eq!(
            unsubscribed.to_string(),
            "Invalid CoinAPI WebSocket request: book:COINBASE_SPOT_ETH_USD is not subscribed"
        );
        assert_eq!(
            not_book.to_string(),
            "Invalid CoinAPI WebSocket request: trade:COINBASE_SPOT_BTC_USD has no snapshot to restart"
        );
        assert!(!extra_frame);
    })
    .await;
}

// A reconnect's replay waits out the interval since the previous hello, even on a new socket
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_replay_waits_out_hello_interval() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let first = next_text(&mut socket).await;
            let sent = time::Instant::now();
            socket.close(None).await.unwrap();
            drop(socket);

            let (mut socket, _) = accept(&listener).await;
            let replay = next_text(&mut socket).await;
            (first, replay, sent.elapsed())
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        let (first, replay, waited) = peer.await.unwrap();
        let reconnected = recv(&mut rx).await;
        client.close().await;

        assert_eq!(first, HELLO_BTC);
        assert_eq!(replay, HELLO_BTC);
        assert_eq!(reconnected, CoinApiWsMessage::Reconnected);
        assert!(
            waited >= Duration::from_millis(1_900),
            "replayed after {waited:?}"
        );
    })
    .await;
}

// Only a removal scope without the pair's symbol pauses its other channels
#[rstest]
#[case::whole_scope(&[], false)]
#[case::beside_other_symbol(&[ETH], true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn resubscribe_reports_pause_when_symbol_leaves_scope(
    #[case] other_trades: &'static [&'static str],
    #[case] expected: bool,
) {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            for _ in 0..3 {
                next_text(&mut socket).await;
                send(
                    &mut socket,
                    include_str!("../../../test_data/ws_heartbeat.json"),
                )
                .await;
            }
        });

        let book = subscription(
            Channel {
                data_type: DataType::Book,
                period_id: None,
            },
            BTC,
        );

        let mut changes = vec![ScopeChange::Subscribe(book)];
        changes.extend(
            other_trades
                .iter()
                .map(|symbol_id| ScopeChange::Subscribe(subscription(TRADES, symbol_id))),
        );

        let (client, _rx) = client(&address);
        client.apply(&changes).await.unwrap();
        let mut paused = false;
        client.resubscribe(book, || paused = true).await.unwrap();

        peer.await.unwrap();
        client.close().await;

        assert_eq!(paused, expected);
    })
    .await;
}

// A burst of changes becomes one hello, and the next hello waits out the connection's interval
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn apply_sends_one_hello_per_batch_and_paces_batches() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let mut frames = Vec::new();
            for _ in 0..2 {
                frames.push((next_text(&mut socket).await, time::Instant::now()));
                send(&mut socket, include_str!("../../../test_data/ws_heartbeat.json")).await;
            }

            let extra_frame = time::timeout(Duration::from_millis(500), socket.next())
                .await
                .is_ok();
            (frames, extra_frame)
        });

        let book = |data_type| {
            subscription(
                Channel {
                    data_type,
                    period_id: None,
                },
                ETH,
            )
        };

        let (client, _rx) = client(&address);
        let rejected = client
            .apply(&[
                ScopeChange::Subscribe(subscription(TRADES, BTC)),
                ScopeChange::Subscribe(book(DataType::Book)),
                ScopeChange::Subscribe(book(DataType::Book5)),
                ScopeChange::Subscribe(subscription(TRADES, ETH)),
                ScopeChange::Unsubscribe(subscription(TRADES, BTC)),
            ])
            .await
            .unwrap();
        let second = client
            .apply(&[ScopeChange::Subscribe(subscription(TRADES, BTC))])
            .await
            .unwrap();

        let (frames, extra_frame) = peer.await.unwrap();
        client.close().await;

        let texts: Vec<&str> = frames.iter().map(|(frame, _)| frame.as_str()).collect();
        let pause = frames[1].1 - frames[0].1;

        assert_eq!(
            rejected
                .iter()
                .map(|(change, e)| (*change, e.to_string()))
                .collect::<Vec<_>>(),
            [(
                ScopeChange::Subscribe(book(DataType::Book5)),
                "Invalid CoinAPI WebSocket request: scope already streams book, so cannot add book5"
                    .to_string()
            )]
        );
        assert!(second.is_empty());
        assert_eq!(
            texts,
            [
                r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_ETH_USD$"]}"#,
                r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$","COINBASE_SPOT_ETH_USD$"]}"#,
            ]
        );
        assert!(pause >= Duration::from_millis(1_900), "pause {pause:?}");
        assert!(!extra_frame);
    })
    .await;
}

// CoinAPI ignores back-to-back scopes and closes a connection that sends hellos too fast
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn resubscribe_pauses_between_scopes_and_spaces_restarts() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let mut frames = Vec::new();

            // Heartbeats keep the client's idle timeout from reconnecting between restarts
            for _ in 0..5 {
                let frame = next_text(&mut socket).await;
                frames.push((frame, time::Instant::now()));
                send(&mut socket, include_str!("../../../test_data/ws_heartbeat.json")).await;
            }

            frames
        });

        let book = subscription(
            Channel {
                data_type: DataType::Book,
                period_id: None,
            },
            BTC,
        );

        let (client, _rx) = client(&address);
        client.subscribe(book).await.unwrap();
        client.resubscribe(book, || {}).await.unwrap();
        client.resubscribe(book, || {}).await.unwrap();

        let frames = peer.await.unwrap();
        client.close().await;

        let hello_book = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
        let texts: Vec<&str> = frames.iter().map(|(frame, _)| frame.as_str()).collect();
        let at = |index: usize| frames[index].1;

        assert_eq!(
            texts,
            [hello_book, HELLO_BTC, hello_book, HELLO_BTC, hello_book]
        );
        assert!(at(2) - at(1) >= Duration::from_millis(1_900), "pause {:?}", at(2) - at(1));
        assert!(at(3) - at(1) >= Duration::from_millis(4_900), "spacing {:?}", at(3) - at(1));
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn combined_scope_frames_outside_subscribed_pairs_are_dropped() {
    bounded(async {
        let (listener, address) = listener().await;
        let ohlcv_btc = include_str!("../../../test_data/ws_ohlcv.json");
        let ohlcv_eth = ohlcv_btc.replace(BTC, ETH);
        let expected = [trade_message(TRADE), ohlcv_message(&ohlcv_eth)];

        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            next_text(&mut socket).await;
            next_text(&mut socket).await;
            send(&mut socket, &trade_for(ETH)).await;
            send(&mut socket, ohlcv_btc).await;
            send(&mut socket, &ohlcv_eth.replace("\"1SEC\"", "\"2SEC\"")).await;
            send(&mut socket, TRADE).await;
            send(&mut socket, &ohlcv_eth).await;
            socket
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        client
            .subscribe(subscription(ohlcv("1SEC"), ETH))
            .await
            .unwrap();

        assert_eq!([recv(&mut rx).await, recv(&mut rx).await], expected);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let _socket = peer.await.unwrap();
        client.close().await;
    })
    .await;
}

#[rstest]
#[case(DataType::Book, None, "BITSTAMP_SPOT_BTC_USD", include_str!("../../../test_data/ws_book.json"))]
#[case(DataType::Book20, None, "BITSTAMP_SPOT_BTC_USD", include_str!("../../../test_data/ws_book20.json"))]
#[case(DataType::BookL3, None, "COINBASE_SPOT_BCH_USD", include_str!("../../../test_data/ws_book_l3.json"))]
#[case(DataType::Quote, None, "BITSTAMP_SPOT_BTC_USD", include_str!("../../../test_data/ws_quote.json"))]
#[case(DataType::Trade, None, "COINBASE_SPOT_BTC_USD", TRADE)]
#[case(DataType::Ohlcv, Some("1SEC"), "COINBASE_SPOT_BTC_USD", include_str!("../../../test_data/ws_ohlcv.json"))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn routes_each_message_family(
    #[case] data_type: DataType,
    #[case] period_id: Option<&'static str>,
    #[case] symbol_id: &'static str,
    #[case] bytes: &'static str,
) {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            next_text(&mut socket).await;
            send(
                &mut socket,
                include_str!("../../../test_data/ws_heartbeat.json"),
            )
            .await;
            send(&mut socket, bytes).await;
            socket
        });

        let (client, mut rx) = client(&address);

        let channel = Channel {
            data_type,
            period_id: period_id.map(Ustr::from),
        };

        client
            .subscribe(subscription(channel, symbol_id))
            .await
            .unwrap();

        let expected = match data_type {
            DataType::Trade => trade_message(bytes),
            DataType::Quote => CoinApiWsMessage::Quote(serde_json::from_str(bytes).unwrap()),
            DataType::BookL3 => CoinApiWsMessage::BookL3(serde_json::from_str(bytes).unwrap()),
            DataType::Ohlcv => ohlcv_message(bytes),
            _ => CoinApiWsMessage::Book(serde_json::from_str(bytes).unwrap()),
        };

        assert_eq!(recv(&mut rx).await, expected);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let _socket = peer.await.unwrap();
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn drops_malformed_foreign_and_unsubscribed_frames() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            next_text(&mut socket).await;
            send(
                &mut socket,
                include_str!("../../../test_data/malformed.txt"),
            )
            .await;
            send(&mut socket, r#"{"asset_id":"BTC","type":"asset"}"#).await;
            send(
                &mut socket,
                include_str!("../../../test_data/ws_quote.json"),
            )
            .await;
            send(&mut socket, &trade_for(ETH)).await;
            send(&mut socket, TRADE).await;
            socket
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        assert_eq!(recv(&mut rx).await, trade_message(TRADE));
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let _socket = peer.await.unwrap();
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_replaces_scope_and_last_symbol_closes() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let mut frames = Vec::new();
            for _ in 0..3 {
                frames.push(next_text(&mut socket).await);
            }

            send(&mut socket, TRADE).await;
            send(&mut socket, &trade_for(ETH)).await;
            (frames, closes_without_text(&mut socket).await)
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        client.subscribe(subscription(TRADES, ETH)).await.unwrap();
        client.unsubscribe(subscription(TRADES, BTC)).await.unwrap();
        client.unsubscribe(subscription(TRADES, BTC)).await.unwrap();

        assert_eq!(recv(&mut rx).await, trade_message(&trade_for(ETH)));
        client.unsubscribe(subscription(TRADES, ETH)).await.unwrap();

        let (frames, closed) = peer.await.unwrap();
        assert_eq!(frames, [HELLO_BTC, HELLO_BTC_ETH, HELLO_ETH]);
        assert!(closed);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn disconnect_reconnects_and_replays_scope() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut first, _) = accept(&listener).await;
            let initial = [next_text(&mut first).await, next_text(&mut first).await];
            first.close(None).await.unwrap();
            drop(first);

            let (mut second, headers) = accept(&listener).await;
            let replay = next_text(&mut second).await;
            send(&mut second, &trade_for(ETH)).await;
            (initial, headers, replay, second)
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        client.subscribe(subscription(TRADES, ETH)).await.unwrap();

        assert_eq!(recv(&mut rx).await, CoinApiWsMessage::Reconnected);
        assert_eq!(recv(&mut rx).await, trade_message(&trade_for(ETH)));

        let (initial, headers, replay, _second) = peer.await.unwrap();
        assert_eq!(initial, [HELLO_BTC, HELLO_BTC_ETH]);
        assert_eq!(headers, ["test-api-key", "nautilus"]);
        assert_eq!(replay, HELLO_BTC_ETH);
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_notice_keeps_streaming_until_server_close() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut first, _) = accept(&listener).await;
            let initial = next_text(&mut first).await;
            send(
                &mut first,
                include_str!("../../../test_data/ws_reconnect.json"),
            )
            .await;
            let early_connection = time::timeout(Duration::from_millis(500), listener.accept())
                .await
                .is_ok();
            send(&mut first, TRADE).await;
            first.close(None).await.unwrap();
            drop(first);

            let (mut second, _) = accept(&listener).await;
            (
                initial,
                early_connection,
                next_text(&mut second).await,
                second,
            )
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        assert_eq!(recv(&mut rx).await, trade_message(TRADE));
        assert_eq!(recv(&mut rx).await, CoinApiWsMessage::Reconnected);

        let (initial, early_connection, replay, _second) = peer.await.unwrap();
        assert_eq!(initial, HELLO_BTC);
        assert!(!early_connection);
        assert_eq!(replay, HELLO_BTC);
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn error_frame_retires_channel_without_replay() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            next_text(&mut socket).await;
            let initial_at = time::Instant::now();
            send(
                &mut socket,
                include_str!("../../../test_data/ws_error_credential.json"),
            )
            .await;
            socket.close(None).await.unwrap();
            drop(socket);

            // A transport reconnect can race retirement, but must never replay the scope
            let mut replayed = false;

            while let Ok(accepted) =
                time::timeout(Duration::from_millis(500), listener.accept()).await
            {
                let (stream, _) = accepted.unwrap();
                let mut stray = accept_async(stream).await.unwrap();
                replayed |=
                    !time::timeout(Duration::from_millis(200), closes_without_text(&mut stray))
                        .await
                        .unwrap_or(true);
            }

            let (mut resubscribed, _) = accept(&listener).await;
            let hello = next_text(&mut resubscribed).await;
            let pause = initial_at.elapsed();
            (replayed, hello, pause, resubscribed)
        });

        let (client, mut rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        let error = recv(&mut rx).await;
        assert_eq!(
            error,
            CoinApiWsMessage::Error("Invalid key <redacted>".into())
        );
        assert!(!format!("{error:?}").contains("test-api-key"));

        time::sleep(Duration::from_millis(700)).await;
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        let (replayed, hello, pause, _socket) = peer.await.unwrap();
        assert!(!replayed);
        assert_eq!(hello, HELLO_BTC);
        assert!(pause >= Duration::from_millis(1_900), "pause {pause:?}");
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        client.close().await;
    })
    .await;
}

#[rstest]
#[case::close(false)]
#[case::unsubscribe_all(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn closing_and_reopening_preserves_hello_pacing(#[case] unsubscribe_all: bool) {
    bounded(async {
        let (listener, address) = listener().await;
        let (read_tx, read_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let (mut first, _) = accept(&listener).await;
            let initial = next_text(&mut first).await;
            let initial_at = time::Instant::now();
            read_tx.send(()).unwrap();
            let closed = closes_without_text(&mut first).await;
            let (mut second, _) = accept(&listener).await;
            let resubscribed = next_text(&mut second).await;
            let pause = initial_at.elapsed();
            (initial, closed, resubscribed, pause, second)
        });

        let (client, _rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        read_rx.await.unwrap();

        if unsubscribe_all {
            client.unsubscribe(subscription(TRADES, BTC)).await.unwrap();
        } else {
            client.close().await;
            client.close().await;
        }

        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        let (initial, closed, resubscribed, pause, _second) = peer.await.unwrap();
        assert_eq!(initial, HELLO_BTC);
        assert!(closed);
        assert_eq!(resubscribed, HELLO_BTC);
        assert!(pause >= Duration::from_millis(1_900), "pause {pause:?}");
        client.close().await;
    })
    .await;
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn data_config_routes_websocket_through_explicit_proxy(#[case] backend: TransportBackend) {
    bounded(async {
        let (listener, address) = listener().await;
        let config = CoinApiDataClientConfig::builder()
            .base_url_ws("ws://coinapi.invalid/v1/".into())
            .proxy_url(format!("http://user:secret@{address}").into())
            .transport_backend(backend)
            .build();
        let (client, mut messages) =
            CoinApiWebSocketClient::new("test-api-key".into(), config.websocket_config()).unwrap();

        let peer = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }

            let request = String::from_utf8(request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            #[allow(
                clippy::result_large_err,
                reason = "tungstenite fixes the handshake callback error type"
            )]
            let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
                assert_eq!(request.uri().path(), "/v1/");
                assert_eq!(request.headers()["x-coinapi-key"], "test-api-key");
                assert_eq!(request.headers()["x-referrer"], "nautilus");
                assert!(!request.headers().contains_key("proxy-authorization"));
                Ok(response)
            })
            .await
            .unwrap();

            let hello = next_text(&mut socket).await;
            send(&mut socket, TRADE).await;
            (request, hello)
        });

        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        let message = recv(&mut messages).await;
        let (request, hello) = peer.await.unwrap();
        client.close().await;
        let mut lines = request.split("\r\n");
        assert_eq!(lines.next(), Some("CONNECT coinapi.invalid:80 HTTP/1.1"));
        let mut headers: Vec<_> = lines.filter(|line| !line.is_empty()).collect();
        headers.sort_unstable();
        assert_eq!(
            headers,
            vec![
                "Host: coinapi.invalid:80",
                "Proxy-Authorization: Basic dXNlcjpzZWNyZXQ=",
                "Proxy-Connection: Keep-Alive"
            ]
        );
        assert_eq!(hello, HELLO_BTC);
        let expected = trade_message(TRADE);
        assert_eq!(message, expected);
    })
    .await;
}

#[rstest]
#[case("", "ws://127.0.0.1:1/v1/")]
#[case("key\r\nInjected: value", "ws://127.0.0.1:1/v1/")]
#[case("test-api-key", "https://127.0.0.1:1/v1/")]
#[case("test-api-key", "ws://user:secret@127.0.0.1:1/v1/")]
#[case("test-api-key", "ws://127.0.0.1:1/v1/?apikey=secret")]
#[case("test-api-key", "ws://127.0.0.1:1/v1/#secret")]
fn invalid_credentials_or_endpoint_are_rejected(#[case] key: &str, #[case] url: &str) {
    let result = CoinApiWebSocketClient::new(
        SecretString::from(key),
        CoinApiWebSocketConfig {
            url: url.into(),
            backend: TransportBackend::Tungstenite,
            ..CoinApiWebSocketConfig::default()
        },
    );

    let Err(e) = result else {
        panic!("expected validation error");
    };

    assert!(matches!(e, Error::Validation(_)));
    assert!(!format!("{e:?}").contains("secret"));
}

#[rstest]
#[case("")]
#[case("127.0.0.1:3128")]
#[case("proxy:3128")]
#[case("proxy-secret:password@localhost:3128")]
#[case("socks4://localhost:3128")]
#[case("socks4a://localhost:3128")]
#[case("socks5://localhost:3128")]
#[case("socks5h://localhost:3128")]
#[case("ftp://localhost:3128")]
fn invalid_proxy_is_rejected_without_exposure(
    #[case] proxy: &str,
    #[values(TransportBackend::Tungstenite, TransportBackend::Sockudo)] backend: TransportBackend,
) {
    let error = CoinApiWebSocketClient::new(
        "test-api-key".into(),
        CoinApiWebSocketConfig {
            proxy_url: Some(proxy.into()),
            backend,
            ..CoinApiWebSocketConfig::default()
        },
    )
    .unwrap_err();

    assert!(matches!(error, Error::Validation(ref message)
        if message == "proxy URL must be a valid HTTP(S) URL"));
    assert_eq!(
        error.to_string(),
        "Invalid CoinAPI WebSocket request: proxy URL must be a valid HTTP(S) URL"
    );
}

#[rstest]
#[case("http://user:proxy-secret@localhost:3128")]
#[case("https://user:proxy-secret@localhost:3128")]
fn valid_proxy_is_accepted_without_connecting(
    #[case] proxy: &str,
    #[values(TransportBackend::Tungstenite, TransportBackend::Sockudo)] backend: TransportBackend,
) {
    let (client, _rx) = CoinApiWebSocketClient::new(
        "test-api-key".into(),
        CoinApiWebSocketConfig {
            proxy_url: Some(proxy.into()),
            backend,
            ..CoinApiWebSocketConfig::default()
        },
    )
    .unwrap();

    assert!(!format!("{client:?}").contains("proxy-secret"));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn dropping_client_closes_connection() {
    bounded(async {
        let (listener, address) = listener().await;
        let (read_tx, read_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let (mut socket, _) = accept(&listener).await;
            let hello = next_text(&mut socket).await;
            read_tx.send(()).unwrap();
            (hello, closes_without_text(&mut socket).await)
        });

        let (client, _rx) = client(&address);
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();
        read_rx.await.unwrap();
        drop(client);

        let (hello, closed) = peer.await.unwrap();
        assert_eq!(hello, HELLO_BTC);
        assert!(closed);
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn silent_connection_reconnects_and_replays_scope() {
    bounded(async {
        let (listener, address) = listener().await;
        let peer = task::spawn(async move {
            let (mut first, _) = accept(&listener).await;
            let initial = next_text(&mut first).await;
            let (mut second, _) = accept(&listener).await;
            (initial, next_text(&mut second).await, first, second)
        });

        let (client, mut rx) = client(&address);
        let start = time::Instant::now();
        client.subscribe(subscription(TRADES, BTC)).await.unwrap();

        assert_eq!(
            recv_within(&mut rx, Duration::from_secs(8)).await,
            CoinApiWsMessage::Reconnected
        );
        assert!(start.elapsed() >= Duration::from_secs(5));

        let (initial, replay, _first, _second) = peer.await.unwrap();
        assert_eq!(initial, HELLO_BTC);
        assert_eq!(replay, HELLO_BTC);
        client.close().await;
    })
    .await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_connect_leaves_no_subscription() {
    let (listener, address) = listener().await;
    drop(listener);
    let (client, mut rx) = client(&address);

    for _ in 0..2 {
        assert!(matches!(
            client.subscribe(subscription(TRADES, BTC)).await,
            Err(Error::Transport(_))
        ));
    }

    client.unsubscribe(subscription(TRADES, BTC)).await.unwrap();

    assert!(!format!("{client:?}").contains("test-api-key"));
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_subscription_never_connects() {
    let (client, _rx) = client("127.0.0.1:1");

    let ohlcv_without_period = Channel {
        data_type: DataType::Ohlcv,
        period_id: None,
    };

    assert!(matches!(
        client
            .subscribe(subscription(ohlcv_without_period, BTC))
            .await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client.unsubscribe(subscription(TRADES, "BTC$")).await,
        Err(Error::Validation(_))
    ));
}
