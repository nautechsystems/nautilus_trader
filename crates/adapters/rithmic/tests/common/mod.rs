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
    AccountPnLPositionUpdate, BestBidOffer, InstrumentPnLPositionUpdate, LastTrade, MessageType,
    RequestAccountList, RequestLogin, RequestLogout, RequestMarketDataUpdate,
    RequestPnLPositionSnapshot, RequestPnLPositionUpdates, RequestSubscribeForOrderUpdates,
    RequestTimeBarReplay, RequestTimeBarUpdate, ResponseAccountList, ResponseLogin, ResponseLogout,
    ResponseMarketDataUpdate, ResponsePnLPositionSnapshot, ResponsePnLPositionUpdates,
    ResponseSubscribeForOrderUpdates, ResponseTimeBarReplay, ResponseTimeBarUpdate, TimeBar,
    best_bid_offer::PresenceBits,
    request_market_data_update::{Request as MarketDataRequest, UpdateBits},
    request_pn_l_position_updates::Request as PnlPositionRequest,
    request_time_bar_replay::BarType as TimeBarReplayType,
    request_time_bar_update::{BarType as TimeBarUpdateType, Request as TimeBarUpdateRequest},
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

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

fn base_test_gateway_config(url: &str) -> GatewayConfig {
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
}

pub fn test_ticker_only_gateway_config(url: &str) -> GatewayConfig {
    base_test_gateway_config(url)
        .with_ticker(true)
        .with_order(false)
        .with_pnl(false)
        .with_history(false)
}

pub fn test_order_only_gateway_config(url: &str) -> GatewayConfig {
    base_test_gateway_config(url)
        .with_ticker(false)
        .with_order(true)
        .with_pnl(false)
        .with_history(false)
}

pub fn test_pnl_only_gateway_config(url: &str) -> GatewayConfig {
    base_test_gateway_config(url)
        .with_ticker(false)
        .with_order(false)
        .with_pnl(true)
        .with_history(false)
}

pub fn test_history_only_gateway_config(url: &str) -> GatewayConfig {
    base_test_gateway_config(url)
        .with_ticker(false)
        .with_order(false)
        .with_pnl(false)
        .with_history(true)
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

                                send_protobuf(&mut ws, login_response(request_id)).await;
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

                                send_protobuf(&mut ws, logout_response(request_id)).await;
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

pub struct MockOrderPlant {
    pub url: String,
    handle: JoinHandle<()>,
}

impl MockOrderPlant {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

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

                                send_protobuf(&mut ws, login_response(request_id)).await;
                            }
                            302 => {
                                let request = RequestAccountList::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(request.fcm_id.as_deref(), Some("fcm"));
                                assert_eq!(request.ib_id.as_deref(), Some("ib"));

                                send_protobuf(
                                    &mut ws,
                                    ResponseAccountList {
                                        template_id: 303,
                                        user_msg: vec![request_id.clone()],
                                        rq_handler_rp_code: vec!["0".to_string()],
                                        rp_code: vec![],
                                        fcm_id: Some("fcm".to_string()),
                                        ib_id: Some("ib".to_string()),
                                        account_id: Some("account-1".to_string()),
                                        account_name: Some("Primary".to_string()),
                                        account_currency: Some("USD".to_string()),
                                        account_auto_liquidate: None,
                                        auto_liq_threshold_current_value: None,
                                    },
                                )
                                .await;

                                send_protobuf(
                                    &mut ws,
                                    ResponseAccountList {
                                        template_id: 303,
                                        user_msg: vec![request_id],
                                        rq_handler_rp_code: vec![],
                                        rp_code: vec![],
                                        fcm_id: Some("fcm".to_string()),
                                        ib_id: Some("ib".to_string()),
                                        account_id: Some("account-2".to_string()),
                                        account_name: Some("Secondary".to_string()),
                                        account_currency: Some("USD".to_string()),
                                        account_auto_liquidate: None,
                                        auto_liq_threshold_current_value: None,
                                    },
                                )
                                .await;
                            }
                            308 => {
                                let request =
                                    RequestSubscribeForOrderUpdates::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(request.fcm_id.as_deref(), Some("fcm"));
                                assert_eq!(request.ib_id.as_deref(), Some("ib"));
                                assert_eq!(request.account_id.as_deref(), Some("account"));

                                send_protobuf(
                                    &mut ws,
                                    ResponseSubscribeForOrderUpdates {
                                        template_id: 309,
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                    },
                                )
                                .await;
                            }
                            12 => {
                                let request = RequestLogout::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                send_protobuf(&mut ws, logout_response(request_id)).await;
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
            handle,
        }
    }

    pub async fn wait(self) {
        self.handle.await.unwrap();
    }
}

pub struct MockPnlPlant {
    pub url: String,
    snapshot_requests: Arc<AtomicUsize>,
    handle: JoinHandle<()>,
}

impl MockPnlPlant {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let snapshot_requests = Arc::new(AtomicUsize::new(0));
        let snapshot_requests_task = Arc::clone(&snapshot_requests);

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

                                send_protobuf(&mut ws, login_response(request_id)).await;
                            }
                            400 => {
                                let request = RequestPnLPositionUpdates::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(
                                    request.request,
                                    Some(PnlPositionRequest::Subscribe as i32)
                                );
                                assert_eq!(request.fcm_id.as_deref(), Some("fcm"));
                                assert_eq!(request.ib_id.as_deref(), Some("ib"));
                                assert_eq!(request.account_id.as_deref(), Some("account"));

                                send_protobuf(
                                    &mut ws,
                                    ResponsePnLPositionUpdates {
                                        template_id: 401,
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                    },
                                )
                                .await;
                            }
                            402 => {
                                let request = RequestPnLPositionSnapshot::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(request.fcm_id.as_deref(), Some("fcm"));
                                assert_eq!(request.ib_id.as_deref(), Some("ib"));
                                assert_eq!(request.account_id.as_deref(), Some("account"));
                                snapshot_requests_task.fetch_add(1, Ordering::SeqCst);

                                send_protobuf(
                                    &mut ws,
                                    ResponsePnLPositionSnapshot {
                                        template_id: 403,
                                        user_msg: vec![request_id],
                                        rp_code: vec![],
                                    },
                                )
                                .await;

                                send_protobuf(
                                    &mut ws,
                                    AccountPnLPositionUpdate {
                                        template_id: 451,
                                        is_snapshot: Some(true),
                                        fcm_id: Some("fcm".to_string()),
                                        ib_id: Some("ib".to_string()),
                                        account_id: Some("account".to_string()),
                                        fill_buy_qty: None,
                                        fill_sell_qty: None,
                                        order_buy_qty: None,
                                        order_sell_qty: None,
                                        buy_qty: Some(3),
                                        sell_qty: Some(1),
                                        open_long_options_value: None,
                                        open_short_options_value: None,
                                        closed_options_value: None,
                                        option_cash_reserved: None,
                                        rms_account_commission: None,
                                        open_position_pnl: Some("1250.75".to_string()),
                                        open_position_quantity: Some(2),
                                        closed_position_pnl: Some("100.25".to_string()),
                                        closed_position_quantity: Some(1),
                                        net_quantity: Some(2),
                                        excess_buy_margin: None,
                                        margin_balance: Some("25000.25".to_string()),
                                        min_margin_balance: None,
                                        min_account_balance: None,
                                        account_balance: Some("100000.50".to_string()),
                                        cash_on_hand: Some("75000.25".to_string()),
                                        option_closed_pnl: None,
                                        percent_maximum_allowable_loss: None,
                                        option_open_pnl: None,
                                        mtm_account: None,
                                        available_buying_power: None,
                                        used_buying_power: None,
                                        reserved_buying_power: None,
                                        excess_sell_margin: None,
                                        day_open_pnl: None,
                                        day_closed_pnl: None,
                                        day_pnl: None,
                                        day_open_pnl_offset: None,
                                        day_closed_pnl_offset: None,
                                        ssboe: Some(1_700_000_001),
                                        usecs: Some(456_789),
                                    },
                                )
                                .await;

                                send_protobuf(
                                    &mut ws,
                                    InstrumentPnLPositionUpdate {
                                        template_id: 450,
                                        is_snapshot: Some(true),
                                        fcm_id: Some("fcm".to_string()),
                                        ib_id: Some("ib".to_string()),
                                        account_id: Some("account".to_string()),
                                        symbol: Some("ESM6".to_string()),
                                        exchange: Some("CME".to_string()),
                                        product_code: Some("ES".to_string()),
                                        instrument_type: Some("FUT".to_string()),
                                        fill_buy_qty: None,
                                        fill_sell_qty: None,
                                        order_buy_qty: None,
                                        order_sell_qty: None,
                                        buy_qty: Some(3),
                                        sell_qty: Some(1),
                                        avg_open_fill_price: Some(4500.25),
                                        day_open_pnl: Some(300.5),
                                        day_closed_pnl: Some(25.25),
                                        day_pnl: Some(325.75),
                                        day_open_pnl_offset: None,
                                        day_closed_pnl_offset: None,
                                        mtm_security: None,
                                        open_long_options_value: None,
                                        open_short_options_value: None,
                                        closed_options_value: None,
                                        option_cash_reserved: None,
                                        open_position_pnl: Some("300.50".to_string()),
                                        open_position_quantity: Some(2),
                                        closed_position_pnl: Some("25.25".to_string()),
                                        closed_position_quantity: Some(1),
                                        net_quantity: Some(2),
                                        ssboe: Some(1_700_000_001),
                                        usecs: Some(654_321),
                                    },
                                )
                                .await;
                            }
                            12 => {
                                let request = RequestLogout::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                send_protobuf(&mut ws, logout_response(request_id)).await;
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
            snapshot_requests,
            handle,
        }
    }

    pub fn snapshot_requests(&self) -> usize {
        self.snapshot_requests.load(Ordering::SeqCst)
    }

    pub async fn wait(self) {
        self.handle.await.unwrap();
    }
}

pub struct MockHistoryPlant {
    pub url: String,
    bar_requests: Arc<AtomicUsize>,
    live_bar_subscriptions: Arc<AtomicUsize>,
    handle: JoinHandle<()>,
}

impl MockHistoryPlant {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let bar_requests = Arc::new(AtomicUsize::new(0));
        let bar_requests_task = Arc::clone(&bar_requests);
        let live_bar_subscriptions = Arc::new(AtomicUsize::new(0));
        let live_bar_subscriptions_task = Arc::clone(&live_bar_subscriptions);

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

                                send_protobuf(&mut ws, login_response(request_id)).await;
                            }
                            202 => {
                                let request = RequestTimeBarReplay::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(request.symbol.as_deref(), Some("ESM6"));
                                assert_eq!(request.exchange.as_deref(), Some("CME"));
                                assert_eq!(
                                    request.bar_type,
                                    Some(TimeBarReplayType::MinuteBar as i32)
                                );
                                assert_eq!(request.bar_type_period, Some(1));
                                assert_eq!(request.start_index, Some(1_700_000_000));
                                assert_eq!(request.finish_index, Some(1_700_000_060));

                                bar_requests_task.fetch_add(1, Ordering::SeqCst);

                                send_protobuf(
                                    &mut ws,
                                    ResponseTimeBarReplay {
                                        template_id: 203,
                                        request_key: Some("history-req".to_string()),
                                        user_msg: vec![request_id.clone()],
                                        rq_handler_rp_code: vec!["0".to_string()],
                                        rp_code: vec![],
                                        symbol: Some("ESM6".to_string()),
                                        exchange: Some("CME".to_string()),
                                        r#type: Some(TimeBarReplayType::MinuteBar as i32),
                                        period: Some("1".to_string()),
                                        marker: Some(1_700_000_000),
                                        num_trades: Some(12),
                                        volume: Some(100),
                                        bid_volume: Some(45),
                                        ask_volume: Some(55),
                                        open_price: Some(4500.00),
                                        close_price: Some(4500.25),
                                        high_price: Some(4500.50),
                                        low_price: Some(4499.75),
                                        settlement_price: None,
                                        has_settlement_price: Some(false),
                                        must_clear_settlement_price: Some(false),
                                    },
                                )
                                .await;

                                send_protobuf(
                                    &mut ws,
                                    ResponseTimeBarReplay {
                                        template_id: 203,
                                        request_key: Some("history-req".to_string()),
                                        user_msg: vec![request_id],
                                        rq_handler_rp_code: vec![],
                                        rp_code: vec![],
                                        symbol: Some("ESM6".to_string()),
                                        exchange: Some("CME".to_string()),
                                        r#type: Some(TimeBarReplayType::MinuteBar as i32),
                                        period: Some("1".to_string()),
                                        marker: Some(1_700_000_060),
                                        num_trades: Some(10),
                                        volume: Some(80),
                                        bid_volume: Some(30),
                                        ask_volume: Some(50),
                                        open_price: Some(4500.25),
                                        close_price: Some(4500.75),
                                        high_price: Some(4501.00),
                                        low_price: Some(4500.00),
                                        settlement_price: None,
                                        has_settlement_price: Some(false),
                                        must_clear_settlement_price: Some(false),
                                    },
                                )
                                .await;
                            }
                            200 => {
                                let request = RequestTimeBarUpdate::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                assert_eq!(request.symbol.as_deref(), Some("ESM6"));
                                assert_eq!(request.exchange.as_deref(), Some("CME"));
                                assert_eq!(
                                    request.bar_type,
                                    Some(TimeBarUpdateType::MinuteBar as i32)
                                );
                                assert_eq!(request.bar_type_period, Some(1));

                                if request.request == Some(TimeBarUpdateRequest::Subscribe as i32) {
                                    live_bar_subscriptions_task.fetch_add(1, Ordering::SeqCst);

                                    send_protobuf(
                                        &mut ws,
                                        ResponseTimeBarUpdate {
                                            template_id: 201,
                                            user_msg: vec![request_id],
                                            rp_code: vec![],
                                        },
                                    )
                                    .await;

                                    send_protobuf(
                                        &mut ws,
                                        TimeBar {
                                            template_id: 250,
                                            symbol: Some("ESM6".to_string()),
                                            exchange: Some("CME".to_string()),
                                            r#type: Some(TimeBarUpdateType::MinuteBar as i32),
                                            period: Some("1".to_string()),
                                            marker: Some(1_700_000_120),
                                            num_trades: Some(14),
                                            volume: Some(110),
                                            bid_volume: Some(50),
                                            ask_volume: Some(60),
                                            open_price: Some(4500.75),
                                            close_price: Some(4501.25),
                                            high_price: Some(4501.50),
                                            low_price: Some(4500.50),
                                            settlement_price: None,
                                            has_settlement_price: Some(false),
                                            must_clear_settlement_price: Some(false),
                                        },
                                    )
                                    .await;
                                } else {
                                    assert_eq!(
                                        request.request,
                                        Some(TimeBarUpdateRequest::Unsubscribe as i32)
                                    );

                                    send_protobuf(
                                        &mut ws,
                                        ResponseTimeBarUpdate {
                                            template_id: 201,
                                            user_msg: vec![request_id],
                                            rp_code: vec![],
                                        },
                                    )
                                    .await;
                                }
                            }
                            12 => {
                                let request = RequestLogout::decode(payload).unwrap();
                                let request_id = request.user_msg.first().cloned().unwrap();

                                send_protobuf(&mut ws, logout_response(request_id)).await;
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
            bar_requests,
            live_bar_subscriptions,
            handle,
        }
    }

    pub fn bar_requests(&self) -> usize {
        self.bar_requests.load(Ordering::SeqCst)
    }

    pub fn live_bar_subscriptions(&self) -> usize {
        self.live_bar_subscriptions.load(Ordering::SeqCst)
    }

    pub async fn wait(self) {
        self.handle.await.unwrap();
    }
}

fn login_response(request_id: String) -> ResponseLogin {
    ResponseLogin {
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
    }
}

fn logout_response(request_id: String) -> ResponseLogout {
    ResponseLogout {
        template_id: 13,
        user_msg: vec![request_id],
        rp_code: vec![],
    }
}

async fn send_protobuf(
    ws: &mut WebSocketStream<tokio::net::TcpStream>,
    message: impl ProstMessage,
) {
    ws.send(Message::Binary(encode_message(message).into()))
        .await
        .unwrap();
}

fn encode_message(message: impl ProstMessage) -> Vec<u8> {
    let mut bytes = Vec::new();
    let len = message.encoded_len() as u32;
    bytes.extend_from_slice(&len.to_be_bytes());
    message.encode(&mut bytes).unwrap();
    bytes
}
