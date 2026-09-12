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

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_model::{data::BarType, identifiers::InstrumentId};
use nautilus_network::{dst::net::TcpListener, websocket::TransportBackend};
use nautilus_okx::websocket::{
    client::OKXWebSocketClient,
    enums::{OKXWsChannel, OKXWsOperation},
    messages::{OKXSubscription, OKXSubscriptionArg},
};
use rstest::rstest;
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
