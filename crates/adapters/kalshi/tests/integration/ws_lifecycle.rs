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

use std::{collections::BTreeMap, time::Duration};

use futures_util::{SinkExt, StreamExt};
use nautilus_common::live::dst::{task, time};
use nautilus_kalshi::{
    config::KalshiDataClientConfig,
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        messages::{
            KalshiChannel, KalshiDataMessage, KalshiSubscription, KalshiUpdateAction,
            KalshiUpdated, KalshiWsMessage,
        },
        parse::decode_frame,
    },
};
use nautilus_network::dst::net::TcpStream;
use rstest::rstest;
use tokio_tungstenite::{
    WebSocketStream, accept_async, accept_hdr_async,
    tungstenite::{Message, handshake::server},
};

use super::{
    http::{assert_auth_request, bind_peer},
    keys,
};

fn frame(name: &str) -> String {
    let frames: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../test_data/websocket.json")).unwrap();
    frames[name].clone()
}

fn replayed_data() -> KalshiWsMessage {
    let wire: serde_json::Value = serde_json::from_str(&frame("replayed_data")).unwrap();
    KalshiWsMessage::OrderbookDelta(KalshiDataMessage {
        id: None,
        sid: 21,
        seq: Some(1),
        sending_ts_ms: Some(1_570_000_000_789),
        msg: wire["msg"].clone(),
    })
}

fn book() -> KalshiSubscription {
    KalshiSubscription {
        channel: KalshiChannel::OrderbookDelta,
        market_tickers: vec!["MARKET-A".into()],
    }
}

fn config(address: std::net::SocketAddr) -> KalshiDataClientConfig {
    KalshiDataClientConfig {
        api_key_id: Some("auth-test-id".into()),
        private_key: Some(keys::rsa_pkcs8().into()),
        base_url_ws: Some(format!("ws://{address}/custom/ws/v3").into()),
        ws_timeout_secs: 2,
        ..Default::default()
    }
}

async fn receive(ws_client: &mut WebSocketStream<TcpStream>) -> Message {
    time::timeout(Duration::from_secs(5), ws_client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

async fn expect_command(ws_client: &mut WebSocketStream<TcpStream>, name: &str) {
    assert_eq!(receive(ws_client).await, Message::text(frame(name)));
}

async fn expect_command_at_id(ws_client: &mut WebSocketStream<TcpStream>, name: &str, id: u64) {
    let Message::Text(actual) = receive(ws_client).await else {
        panic!("Expected text command");
    };

    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let expected: serde_json::Value = serde_json::from_str(&frame_at_id(name, id)).unwrap();
    assert_eq!(actual, expected);
}

async fn send_frame(ws_client: &mut WebSocketStream<TcpStream>, name: &str) {
    ws_client.send(Message::text(frame(name))).await.unwrap();
}

async fn send_frame_at_id(ws_client: &mut WebSocketStream<TcpStream>, name: &str, id: u64) {
    ws_client
        .send(Message::text(frame_at_id(name, id)))
        .await
        .unwrap();
}

async fn event(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
) -> KalshiWsEvent {
    time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn until_ready(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
    generation: u64,
) {
    loop {
        if matches!(event(receiver).await, KalshiWsEvent::Ready { generation: current } if current == generation)
        {
            return;
        }
    }
}

#[tokio::test]
async fn initialization_authentication_control_and_duplicate_references() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18260).await;
        let (handshake_tx, handshake_rx) = tokio::sync::oneshot::channel();
        let (commands_tx, commands_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut handshake_tx = Some(handshake_tx);

            #[allow(
                clippy::result_large_err,
                reason = "Tungstenite callback requires this error type"
            )]
            let callback = move |request: &server::Request, response: server::Response| {
                let raw = request
                    .headers()
                    .iter()
                    .map(|(name, value)| format!("{name}: {}\r\n", value.to_str().unwrap()))
                    .collect::<String>();
                assert_auth_request(&raw, "/custom/ws/v3", true);
                handshake_tx.take().unwrap().send(()).unwrap();
                Ok(response)
            };

            let mut ws_client = accept_hdr_async(stream, callback).await.unwrap();
            commands_rx.await.unwrap();
            expect_command(&mut ws_client, "book_subscribe").await;
            ws_client
                .send(Message::Ping(b"heartbeat".to_vec().into()))
                .await
                .unwrap();
            let frames = [receive(&mut ws_client).await, receive(&mut ws_client).await];
            assert!(frames.contains(&Message::Pong(b"heartbeat".to_vec().into())));
            assert!(frames.contains(&Message::text(frame("ticker_subscribe"))));
            send_frame(&mut ws_client, "book_subscribed").await;
            send_frame(&mut ws_client, "ticker_subscribed").await;
            assert_eq!(
                receive(&mut ws_client).await,
                Message::text(frame("book_unsubscribe_3"))
            );
            ws_client
                .send(Message::text(frame("book_unsubscribed_3")))
                .await
                .unwrap();
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        client.subscribe(book()).unwrap();
        let connecting = client.clone();
        let connect = task::spawn(async move { connecting.connect().await });
        handshake_rx.await.unwrap();
        assert!(!client.is_active());
        client
            .subscribe(KalshiSubscription {
                channel: KalshiChannel::Ticker,
                market_tickers: vec!["MARKET-B".into()],
            })
            .unwrap();

        commands_tx.send(()).unwrap();
        connect.await.unwrap().unwrap();
        assert!(client.is_active());
        until_ready(&mut events, 0).await;
        client.unsubscribe(&book()).unwrap();
        assert!(client.is_active());
        client.unsubscribe(&book()).unwrap();

        loop {
            if let KalshiWsEvent::Message {
                message: KalshiWsMessage::Unsubscribed { sid: 11, .. },
                ..
            } = event(&mut events).await
            {
                break;
            }
        }

        client.stop().await.unwrap();
        client.stop().await.unwrap();
        client.reset().await.unwrap();
        client.dispose().await.unwrap();
        client.dispose().await.unwrap();
        assert!(!client.is_active());
        assert_eq!(
            client.subscribe(book()).unwrap_err().to_string(),
            "Kalshi WebSocket client is disposed"
        );
        assert_eq!(
            client.connect().await.unwrap_err().to_string(),
            "Kalshi WebSocket client is disposed"
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removal_while_subscribing_and_resubscribe_during_unsubscribe() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18261).await;
        let (subscribing_tx, subscribing_rx) = tokio::sync::oneshot::channel();
        let (removed_tx, removed_rx) = tokio::sync::oneshot::channel();
        let (unsubscribing_tx, unsubscribing_rx) = tokio::sync::oneshot::channel();
        let (resubscribe_tx, resubscribe_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "book_subscribe").await;
            send_frame(&mut ws_client, "wrong_channel").await;
            send_frame(&mut ws_client, "wrong_id").await;
            subscribing_tx.send(()).unwrap();
            removed_rx.await.unwrap();
            send_frame(&mut ws_client, "book_subscribed").await;
            expect_command(&mut ws_client, "book_unsubscribe_2").await;
            unsubscribing_tx.send(()).unwrap();
            resubscribe_rx.await.unwrap();
            send_frame(&mut ws_client, "unsubscribed_without_id").await;
            send_frame(&mut ws_client, "book_subscribed").await;
            expect_command(&mut ws_client, "book_resubscribe_3").await;
            send_frame(&mut ws_client, "book_resubscribed_3").await;
            send_frame(&mut ws_client, "replayed_data").await;
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        let connecting = client.clone();
        let connect = task::spawn(async move { connecting.connect().await });
        subscribing_rx.await.unwrap();
        client.unsubscribe(&book()).unwrap();
        removed_tx.send(()).unwrap();
        unsubscribing_rx.await.unwrap();
        assert!(!client.is_active());
        client.subscribe(book()).unwrap();
        resubscribe_tx.send(()).unwrap();
        connect.await.unwrap().unwrap();
        until_ready(&mut events, 0).await;
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Message {
                generation: 0,
                message: replayed_data()
            }
        );
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn updates_correlate_sid_filter_snapshot_and_malformed_recovery() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18262).await;
        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "book_subscribe").await;
            send_frame(&mut ws_client, "book_subscribed").await;
            expect_command(&mut ws_client, "add_market").await;
            send_frame(&mut ws_client, "stale_terminal_id").await;
            send_frame(&mut ws_client, "stale_terminal_sid").await;
            send_frame(&mut ws_client, "mismatched_terminal_sid").await;
            send_frame(&mut ws_client, "wrong_update_sid").await;
            ws_client
                .send(Message::text(frame_at_sequence("wrong_update_markets", 1)))
                .await
                .unwrap();
            send_frame(&mut ws_client, "added_market").await;
            expect_command(&mut ws_client, "delete_market").await;
            send_frame(&mut ws_client, "deleted_market").await;
            expect_command(&mut ws_client, "snapshot").await;
            send_frame(&mut ws_client, "malformed").await;
            send_frame(&mut ws_client, "stale_data").await;
            send_frame(&mut ws_client, "snapshot_ok").await;
            send_frame(&mut ws_client, "snapshot_wrong_id").await;
            send_frame(&mut ws_client, "snapshot_wrong_sid").await;
            ws_client
                .send(Message::text(frame_at_sequence("snapshot_wrong_market", 6)))
                .await
                .unwrap();
            ws_client
                .send(Message::text(frame_at_sequence("snapshot_data", 7)))
                .await
                .unwrap();
            expect_command(&mut ws_client, "add_after_snapshot").await;
            send_frame(&mut ws_client, "added_after_snapshot").await;
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &book(),
                KalshiUpdateAction::AddMarkets,
                vec!["MARKET-C".into()],
            )
            .unwrap();
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Message {
                generation: 0,
                message: KalshiWsMessage::Ok {
                    id: Some(2),
                    sid: Some(11),
                    seq: Some(2),
                    sending_ts_ms: Some(1_570_000_000_321),
                    msg: Some(KalshiUpdated {
                        market_tickers: vec!["MARKET-A".into(), "MARKET-C".into()],
                    }),
                },
            }
        );
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &book(),
                KalshiUpdateAction::DeleteMarkets,
                vec!["MARKET-A".into()],
            )
            .unwrap();
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &book(),
                KalshiUpdateAction::GetSnapshot,
                vec!["MARKET-C".into()],
            )
            .unwrap();

        loop {
            match event(&mut events).await {
                KalshiWsEvent::Message {
                    generation,
                    message,
                } if message
                    == decode_frame(frame_at_sequence("snapshot_data", 7).as_bytes()).unwrap() =>
                {
                    assert_eq!(generation, 0);
                    break;
                }
                KalshiWsEvent::Error { message, .. } => {
                    assert_eq!(message, "Malformed Kalshi WebSocket frame");
                }
                KalshiWsEvent::Message {
                    message: KalshiWsMessage::Ok { .. },
                    ..
                } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }

        client
            .update_subscription(
                &book(),
                KalshiUpdateAction::AddMarkets,
                vec!["MARKET-A".into()],
            )
            .unwrap();
        until_ready(&mut events, 0).await;
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

fn frame_at_id(name: &str, id: u64) -> String {
    let mut value: serde_json::Value = serde_json::from_str(&frame(name)).unwrap();
    value["id"] = id.into();
    value.to_string()
}

fn frame_at_sequence(name: &str, sequence: u64) -> String {
    let mut value: serde_json::Value = serde_json::from_str(&frame(name)).unwrap();
    value["seq"] = sequence.into();
    value.to_string()
}

#[rstest]
#[case("overflow", true)]
#[case("channel_error", true)]
#[case("overflow", false)]
#[tokio::test]
async fn reconnect_replays_active_and_pending_except_removed_intent(
    #[case] error: &str,
    #[case] remove_pending: bool,
) {
    time::timeout(Duration::from_secs(12), async {
        let (listener, address) = bind_peer(18263).await;
        let error = error.to_owned();
        let (pending_tx, pending_rx) = tokio::sync::oneshot::channel();
        let (removed_tx, removed_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let mut timestamps = Vec::new();
            let mut pending_tx = Some(pending_tx);
            let mut removed_rx = Some(removed_rx);

            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();

                #[allow(
                    clippy::result_large_err,
                    reason = "Tungstenite callback requires this error type"
                )]
                let callback = |request: &server::Request, response: server::Response| {
                    let raw = request
                        .headers()
                        .iter()
                        .map(|(name, value)| format!("{name}: {}\r\n", value.to_str().unwrap()))
                        .collect::<String>();
                    timestamps.push(assert_auth_request(&raw, "/custom/ws/v3", true));
                    Ok(response)
                };

                let mut ws_client = accept_hdr_async(stream, callback).await.unwrap();
                if attempt == 0 {
                    expect_command(&mut ws_client, "book_subscribe").await;
                    send_frame(&mut ws_client, "book_subscribed").await;
                    expect_command(&mut ws_client, "ticker_subscribe").await;
                    pending_tx.take().unwrap().send(()).unwrap();
                    removed_rx.take().unwrap().await.unwrap();

                    while nautilus_core::time::get_atomic_clock_realtime().get_time_ms()
                        <= timestamps[0]
                    {
                        task::yield_now().await;
                    }

                    send_frame(&mut ws_client, &error).await;
                } else {
                    expect_command_at_id(&mut ws_client, "replayed_book_3", 1).await;
                    send_frame(&mut ws_client, "wrong_id").await;
                    send_frame_at_id(&mut ws_client, "book_resubscribed_3", 1).await;
                    send_frame(&mut ws_client, "valid_data").await;
                    if !remove_pending {
                        expect_command_at_id(&mut ws_client, "ticker_replay", 2).await;
                        send_frame_at_id(&mut ws_client, "ticker_replayed", 2).await;
                    }

                    send_frame(&mut ws_client, "replayed_data").await;
                    let _ = ws_client.next().await;
                }
            }

            assert!(timestamps[1] > timestamps[0]);
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();

        let ticker = KalshiSubscription {
            channel: KalshiChannel::Ticker,
            market_tickers: vec!["MARKET-B".into()],
        };

        client.subscribe(ticker.clone()).unwrap();
        let connecting = client.clone();
        let connect = task::spawn(async move { connecting.connect().await });
        pending_rx.await.unwrap();

        if remove_pending {
            client.unsubscribe(&ticker).unwrap();
        }

        removed_tx.send(()).unwrap();
        let error = connect.await.unwrap().unwrap_err();
        assert_eq!(
            error.to_string(),
            "Kalshi WebSocket subscription restoration timed out"
        );
        client.stop().await.unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 1).await;
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Message {
                generation: 1,
                message: replayed_data()
            }
        );
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(26)]
#[case(27)]
#[tokio::test]
async fn rejected_subscribe_retains_intent_without_immediate_retry(#[case] code: u32) {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18264).await;
        let (rejected_tx, rejected_rx) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "book_subscribe").await;
            send_frame(&mut ws_client, &format!("limit_{code}")).await;
            rejected_rx.await.unwrap();
            ws_client.close(None).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command_at_id(&mut ws_client, "replay_after_rejection", 1).await;
            send_frame_at_id(&mut ws_client, "ack_after_rejection", 1).await;
            send_frame(&mut ws_client, "replayed_data").await;
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        let connecting = client.clone();
        let connect = task::spawn(async move { connecting.connect().await });
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Error {
                generation: 0,
                message: format!("Kalshi WebSocket protocol error {code}")
            }
        );
        assert!(!client.is_active());
        rejected_tx.send(()).unwrap();
        let error = connect.await.unwrap().unwrap_err();
        assert_eq!(
            error.to_string(),
            "Kalshi WebSocket subscription restoration timed out"
        );
        client.stop().await.unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 1).await;
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Message {
                generation: 1,
                message: replayed_data()
            }
        );
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn update_timeout_recovers_despite_continuous_frames() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18265).await;
        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "book_subscribe").await;
            send_frame(&mut ws_client, "book_subscribed").await;
            expect_command(&mut ws_client, "add_market").await;
            let data = frame("valid_data");
            let traffic = task::spawn(async move {
                while ws_client.send(Message::text(data.clone())).await.is_ok() {
                    task::yield_now().await;
                }
            });

            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            traffic.abort();
            let _ = traffic.await;
            expect_command_at_id(&mut ws_client, "replay_updated_filter", 1).await;
            send_frame_at_id(&mut ws_client, "book_resubscribed_3", 1).await;
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &book(),
                KalshiUpdateAction::AddMarkets,
                vec!["MARKET-C".into()],
            )
            .unwrap();

        loop {
            if let KalshiWsEvent::Error {
                generation,
                message,
            } = event(&mut events).await
            {
                assert_eq!(generation, 0);
                assert_eq!(
                    message,
                    "Kalshi WebSocket command acknowledgement timed out"
                );
                break;
            }
        }

        client.stop().await.unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 1).await;
        assert!(client.is_active());
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reset_clears_references_and_consumer_close_stops_transport() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18266).await;

        let peer = task::spawn(async move {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = accept_async(stream).await.unwrap();
                expect_command(&mut ws_client, "book_subscribe").await;
                send_frame(&mut ws_client, "book_subscribed").await;
                if attempt == 1 {
                    expect_command(&mut ws_client, "book_unsubscribe_2").await;
                    send_frame(&mut ws_client, "book_unsubscribed_2").await;
                }

                assert!(matches!(
                    ws_client.next().await,
                    None | Some(Err(_) | Ok(Message::Close(_)))
                ));
            }
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(book()).unwrap();
        client.subscribe(book()).unwrap();
        client.connect().await.unwrap();
        client.reset().await.unwrap();

        while events.try_recv().is_ok() {}
        client.connect().await.unwrap();
        until_ready(&mut events, 1).await;
        client.subscribe(book()).unwrap();
        until_ready(&mut events, 1).await;
        client.unsubscribe(&book()).unwrap();

        loop {
            if matches!(
                event(&mut events).await,
                KalshiWsEvent::Message {
                    message: KalshiWsMessage::Unsubscribed { sid: 11, .. },
                    ..
                }
            ) {
                break;
            }
        }

        drop(events);
        peer.await.unwrap();
        client.stop().await.unwrap();
        client.dispose().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(KalshiChannel::OrderbookDelta, vec![], "order books require market tickers")]
#[case(KalshiChannel::Ticker, vec![String::new()], "market tickers must not be empty")]
#[case(KalshiChannel::Ticker, vec!["A".into(),"A".into()], "duplicate market ticker")]
#[case(KalshiChannel::MarketLifecycleV2, vec!["A".into()], "channel does not support market filters")]
fn invalid_subscription_arguments_are_rejected(
    #[case] channel: KalshiChannel,
    #[case] market_tickers: Vec<String>,
    #[case] expected: &str,
) {
    let (client, _) = KalshiWebSocketClient::new(KalshiDataClientConfig::default()).unwrap();

    let subscription = KalshiSubscription {
        channel,
        market_tickers,
    };

    assert_eq!(
        client.subscribe(subscription).unwrap_err().to_string(),
        expected
    );
}

#[rstest]
fn positions_accept_documented_market_filters() {
    let (client, _receiver) =
        KalshiWebSocketClient::new(KalshiDataClientConfig::default()).unwrap();

    let subscription = KalshiSubscription {
        channel: KalshiChannel::MarketPositions,
        market_tickers: vec!["MARKET-A".into()],
    };

    assert_eq!(
        client.subscribe(subscription).map_err(|e| e.to_string()),
        Ok(())
    );
}

#[tokio::test]
async fn ticker_updates_request_snapshots_only_for_added_markets() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18270).await;
        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "ticker_subscribe_1").await;
            send_frame(&mut ws_client, "ticker_subscribed_1").await;
            expect_command(&mut ws_client, "ticker_add").await;
            send_frame(&mut ws_client, "ticker_added").await;
            expect_command(&mut ws_client, "ticker_delete").await;
            send_frame(&mut ws_client, "ticker_deleted").await;
            let _ = ws_client.next().await;
        });

        let original = KalshiSubscription {
            channel: KalshiChannel::Ticker,
            market_tickers: vec!["MARKET-B".into()],
        };

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client.subscribe(original.clone()).unwrap();
        client.connect().await.unwrap();
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &original,
                KalshiUpdateAction::AddMarkets,
                vec!["MARKET-C".into()],
            )
            .unwrap();
        until_ready(&mut events, 0).await;
        client
            .update_subscription(
                &original,
                KalshiUpdateAction::DeleteMarkets,
                vec!["MARKET-B".into()],
            )
            .unwrap();
        until_ready(&mut events, 0).await;
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn lifecycle_event_fee_updates_require_subscription_ownership() {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18271).await;
        let peer = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws_client = accept_async(stream).await.unwrap();
            expect_command(&mut ws_client, "lifecycle_subscribe").await;
            send_frame(&mut ws_client, "lifecycle_subscribed").await;
            send_frame(&mut ws_client, "event_fee_wrong_sid").await;
            send_frame(&mut ws_client, "event_fee_update").await;
            let _ = ws_client.next().await;
        });

        let (client, mut events) = KalshiWebSocketClient::new(config(address)).unwrap();
        client
            .subscribe(KalshiSubscription {
                channel: KalshiChannel::MarketLifecycleV2,
                market_tickers: vec![],
            })
            .unwrap();

        client.connect().await.unwrap();
        until_ready(&mut events, 0).await;
        let expected_msg = serde_json::from_str::<serde_json::Value>(&frame("event_fee_update"))
            .unwrap()["msg"]
            .clone();
        assert_eq!(
            event(&mut events).await,
            KalshiWsEvent::Message {
                generation: 0,
                message: KalshiWsMessage::EventFeeUpdate(KalshiDataMessage {
                    id: None,
                    sid: 31,
                    seq: Some(17),
                    sending_ts_ms: Some(1_570_000_000_654),
                    msg: expected_msg,
                }),
            }
        );
        client.stop().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}
