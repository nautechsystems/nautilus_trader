#![allow(dead_code)]
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

//! Common helpers for Rithmic crate integration tests.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use rithmic_rs::rti::{
    BestBidOffer, LastTrade, MessageType, RequestLogin, RequestLogout, RequestMarketDataUpdate,
    ResponseLogin, ResponseLogout, ResponseMarketDataUpdate,
    best_bid_offer::PresenceBits,
    request_market_data_update::{Request as MarketDataRequest, UpdateBits},
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use nautilus_rithmic::{
    GatewayConfig, RithmicDataClient, RithmicEnv, RithmicError, RithmicGateway,
};

pub fn test_gateway_config() -> GatewayConfig {
    GatewayConfig::new(
        RithmicEnv::Demo,
        "user",
        "pass",
        "system",
        "fcm",
        "ib",
        "account",
    )
    .with_history(true)
}

pub fn test_gateway() -> RithmicGateway {
    RithmicGateway::new(test_gateway_config())
}

pub fn test_gateway_arc() -> Arc<RithmicGateway> {
    Arc::new(test_gateway())
}

pub fn test_data_client() -> RithmicDataClient {
    RithmicDataClient::new(test_gateway_arc())
}

pub fn test_ticker_only_gateway_config(url: &str) -> GatewayConfig {
    GatewayConfig::new(
        RithmicEnv::Demo,
        "user",
        "pass",
        "system",
        "fcm",
        "ib",
        "account",
    )
    .with_url_override(url)
    .with_ticker(true)
    .with_order(false)
    .with_pnl(false)
    .with_history(false)
}

pub fn assert_connection_error(err: RithmicError, expected: &str) {
    match err {
        RithmicError::Connection(message) => assert_eq!(message, expected),
        other => panic!("expected connection error {expected:?}, got {other:?}"),
    }
}

pub struct MockTickerPlant {
    pub url: String,
    subscribe_requests: Arc<AtomicUsize>,
    handle: JoinHandle<()>,
}

impl MockTickerPlant {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let subscribe_requests = Arc::new(AtomicUsize::new(0));
        let subscribe_requests_task = Arc::clone(&subscribe_requests);

        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            while let Some(message) = ws.next().await {
                match message.unwrap() {
                    Message::Binary(data) => {
                        let payload = &data[4..];
                        let message_type = MessageType::decode(payload).unwrap();

                        match message_type.template_id {
                            10 => {
                                let request = RequestLogin::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                ws.send(Message::Binary(
                                    encode_message(ResponseLogin {
                                        template_id: 11,
                                        template_version: Some("5.30".to_string()),
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                        fcm_id: Some("fcm".to_string()),
                                        ib_id: Some("ib".to_string()),
                                        country_code: None,
                                        state_code: None,
                                        unique_user_id: Some("mock-session".to_string()),
                                        heartbeat_interval: Some(60.0),
                                    })
                                    .into(),
                                ))
                                .await
                                .unwrap();
                            }
                            100 => {
                                let request = RequestMarketDataUpdate::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();
                                let symbol = request.symbol.clone().unwrap();
                                let exchange = request.exchange.clone().unwrap();

                                assert_eq!(
                                    request.request,
                                    Some(MarketDataRequest::Subscribe as i32)
                                );
                                assert_eq!(
                                    request.update_bits,
                                    Some(UpdateBits::LastTrade as u32 | UpdateBits::Bbo as u32)
                                );

                                subscribe_requests_task.fetch_add(1, Ordering::SeqCst);

                                ws.send(Message::Binary(
                                    encode_message(ResponseMarketDataUpdate {
                                        template_id: 101,
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                    })
                                    .into(),
                                ))
                                .await
                                .unwrap();

                                ws.send(Message::Binary(
                                    encode_message(BestBidOffer {
                                        template_id: 151,
                                        symbol: Some(symbol.clone()),
                                        exchange: Some(exchange.clone()),
                                        presence_bits: Some(
                                            PresenceBits::Bid as u32 | PresenceBits::Ask as u32,
                                        ),
                                        clear_bits: None,
                                        is_snapshot: Some(false),
                                        bid_price: Some(4500.25),
                                        bid_size: Some(10),
                                        bid_orders: None,
                                        bid_implicit_size: None,
                                        bid_time: None,
                                        ask_price: Some(4500.50),
                                        ask_size: Some(12),
                                        ask_orders: None,
                                        ask_implicit_size: None,
                                        ask_time: None,
                                        lean_price: None,
                                        ssboe: Some(1_700_000_000),
                                        usecs: Some(123_456),
                                    })
                                    .into(),
                                ))
                                .await
                                .unwrap();

                                ws.send(Message::Binary(
                                    encode_message(LastTrade {
                                        template_id: 150,
                                        symbol: Some(symbol),
                                        exchange: Some(exchange),
                                        presence_bits: None,
                                        clear_bits: None,
                                        is_snapshot: Some(false),
                                        trade_price: Some(4500.50),
                                        trade_size: Some(3),
                                        aggressor: Some(1),
                                        exchange_order_id: Some("trade-1".to_string()),
                                        aggressor_exchange_order_id: None,
                                        net_change: None,
                                        percent_change: None,
                                        volume: None,
                                        vwap: None,
                                        trade_time: None,
                                        ssboe: Some(1_700_000_000),
                                        usecs: Some(223_456),
                                        source_ssboe: None,
                                        source_usecs: None,
                                        source_nsecs: None,
                                        jop_ssboe: None,
                                        jop_nsecs: None,
                                    })
                                    .into(),
                                ))
                                .await
                                .unwrap();
                            }
                            12 => {
                                let request = RequestLogout::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                ws.send(Message::Binary(
                                    encode_message(ResponseLogout {
                                        template_id: 13,
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                    })
                                    .into(),
                                ))
                                .await
                                .unwrap();
                                break;
                            }
                            18 => {}
                            other => panic!("unexpected request template id {other}"),
                        }
                    }
                    Message::Ping(payload) => {
                        ws.send(Message::Pong(payload)).await.unwrap();
                    }
                    Message::Close(_) => break,
                    Message::Text(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        });

        Self {
            url: format!("ws://{address}"),
            subscribe_requests,
            handle,
        }
    }

    pub fn subscribe_requests(&self) -> usize {
        self.subscribe_requests.load(Ordering::SeqCst)
    }

    pub async fn wait(self) {
        self.handle.await.unwrap();
    }
}

fn encode_message(message: impl ProstMessage) -> Vec<u8> {
    let mut bytes = Vec::new();
    let len = message.encoded_len() as u32;
    bytes.extend_from_slice(&len.to_be_bytes());
    message.encode(&mut bytes).unwrap();
    bytes
}
