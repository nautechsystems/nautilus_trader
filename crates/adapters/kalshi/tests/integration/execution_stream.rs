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

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    future::Future,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use futures_util::{FutureExt, SinkExt, StreamExt};
use nautilus_common::{
    cache::Cache,
    clients::{DataClient, ExecutionClient},
    clock::VirtualClock,
    live::runner::{replace_data_event_sender, replace_exec_event_sender, set_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent,
        data::{SubscribeBookDeltas, SubscribeQuotes},
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateOrderStatusReports, GeneratePositionStatusReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, TradingCommand,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_execution::engine::ExecutionEngine;
use nautilus_kalshi::{
    common::{
        consts::{KALSHI_CLIENT_ID, KALSHI_VENUE},
        parse::parse_instrument_id,
    },
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    data::KalshiDataClient,
    execution::KalshiExecutionClient,
    http::{
        client::KalshiHttpClient,
        models::{Event, Field, Market},
        parse::parse_instrument,
    },
};
use nautilus_live::{
    ExecutionClientCore,
    book::conformance::BookStreamChecker,
    execution::manager::{ExecutionManager, ExecutionManagerConfig},
    runner::AsyncRunner,
    testing::ExecutionHarness,
};
use nautilus_model::{
    accounts::AccountAny,
    data::Data,
    enums::{
        AccountType, BookType, ContingencyType, LiquiditySide, OmsType, OrderSide, OrderStatus,
        OrderType, PositionSide, TimeInForce,
    },
    events::{
        AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderSubmitted,
    },
    identifiers::{AccountId, ClientOrderId, PositionId, StrategyId, TradeId, VenueOrderId},
    instruments::{BinaryOption, InstrumentAny},
    orders::{Order, OrderAny, builder::OrderTestBuilder},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{http_request::read_headers, keys};

struct Peer {
    config: KalshiExecutionClientConfig,
    balance: Arc<Mutex<Value>>,
    orders: Arc<Mutex<Value>>,
    order_lookup: Arc<Mutex<Option<Value>>>,
    orders_archive: Arc<Mutex<Option<Value>>>,
    fills: Arc<Mutex<Value>>,
    positions: Arc<Mutex<Value>>,
    subscriptions: Arc<Mutex<Vec<String>>>,
    notification: Arc<Mutex<Option<Value>>>,
    notification_ready: Arc<tokio::sync::Notify>,
    balance_reads: Arc<AtomicUsize>,
    balance_gate: Arc<tokio::sync::Semaphore>,
    series_gate: Arc<tokio::sync::Semaphore>,
    series_read: Arc<tokio::sync::Notify>,
    history_position_reads: Arc<AtomicUsize>,
    balance_read: Arc<tokio::sync::Notify>,
    position_read: Arc<tokio::sync::Notify>,
    sessions: Arc<AtomicUsize>,
    submissions: Arc<SubmissionPeer>,
    changes: Arc<MutationPeer>,
    batches: Arc<BatchPeer>,
    limits: Arc<Mutex<Value>>,
    costs: Arc<Mutex<Value>>,
    market: Arc<Mutex<Value>>,
    market_overrides: Arc<Mutex<HashMap<String, Value>>>,
    market_gate: Arc<tokio::sync::Semaphore>,
    market_read: Arc<tokio::sync::Notify>,
    limits_gate: Arc<tokio::sync::Semaphore>,
    limits_read: Arc<tokio::sync::Notify>,
    http: tokio::task::JoinHandle<()>,
    websocket: tokio::task::JoinHandle<()>,
}

impl Peer {
    async fn new(fail: Option<&'static str>) -> Self {
        Self::with_timeout(fail, 2).await
    }

    async fn with_timeout(fail: Option<&'static str>, http_timeout_secs: u64) -> Self {
        Self::with_scope(fail, http_timeout_secs, 0).await
    }

    async fn with_scope(
        fail: Option<&'static str>,
        http_timeout_secs: u64,
        subaccount: u32,
    ) -> Self {
        let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let websocket_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let config = KalshiExecutionClientConfig {
            api_key_id: Some("private-test-key".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            base_url_http: Some(format!("http://{}", http_listener.local_addr().unwrap()).into()),
            base_url_ws: Some(
                format!("ws://{}/ws", websocket_listener.local_addr().unwrap()).into(),
            ),
            ws_timeout_secs: 3,
            http_timeout_secs,
            exchange_index: 3,
            subaccount,
            ..Default::default()
        };

        let mut f: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        f["order"]["order"]["subaccount_number"] = subaccount.into();
        let mut initial = f["balance"].clone();
        initial["balance_dollars"] = "1.0001".into();
        initial["updated_ts"] = 1792800000.into();
        let balance = Arc::new(Mutex::new(initial));
        let current = Arc::clone(&balance);
        let history_position_reads = Arc::new(AtomicUsize::new(0));
        let history_positions = Arc::clone(&history_position_reads);
        let balance_reads = Arc::new(AtomicUsize::new(0));
        let reads = Arc::clone(&balance_reads);

        let balance_gate = Arc::new(tokio::sync::Semaphore::new(
            tokio::sync::Semaphore::MAX_PERMITS,
        ));

        let gate_balance = Arc::clone(&balance_gate);
        let series_gate = Arc::new(tokio::sync::Semaphore::new(1));
        let gate_series = Arc::clone(&series_gate);
        let series_read = Arc::new(tokio::sync::Notify::new());
        let read_series = Arc::clone(&series_read);
        let balance_read = Arc::new(tokio::sync::Notify::new());
        let read = Arc::clone(&balance_read);
        let position_read = Arc::new(tokio::sync::Notify::new());
        let read_position = Arc::clone(&position_read);
        let orders = Arc::new(Mutex::new(f["empty_orders"].clone()));
        let orders_http = Arc::clone(&orders);
        let order_lookup = Arc::new(Mutex::new(None::<Value>));
        let lookup_http = Arc::clone(&order_lookup);
        let orders_archive = Arc::new(Mutex::new(None::<Value>));
        let archive_http = Arc::clone(&orders_archive);
        let fills = Arc::new(Mutex::new(f["empty_fills"].clone()));
        let fills_http = Arc::clone(&fills);
        let positions = Arc::new(Mutex::new(f["empty_positions"].clone()));
        let positions_http = Arc::clone(&positions);
        let mut definitions: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        definitions["definitions"]["live"]["event"]["exchange_index"] = 3.into();
        definitions["definitions"]["live"]["market"]["exchange_index"] = 3.into();
        definitions["series"]["series"]["exchange_index"] = 3.into();

        let submissions = Arc::new(SubmissionPeer {
            writes: Mutex::new(Vec::new()),
            written: tokio::sync::Notify::new(),
            outcome: Mutex::new(String::from("resting")),
            ack_gate: Arc::new(tokio::sync::Semaphore::new(0)),
        });

        let changes = Arc::new(MutationPeer {
            writes: Mutex::new(Vec::new()),
            mode: Mutex::new("success".into()),
            written: tokio::sync::Notify::new(),
            ack_gate: Arc::new(tokio::sync::Semaphore::new(0)),
        });

        let batches = Arc::new(BatchPeer {
            writes: Mutex::new(Vec::new()),
            mode: Mutex::new("success".into()),
            written: tokio::sync::Notify::new(),
            ack_gate: Arc::new(tokio::sync::Semaphore::new(0)),
        });

        let batches_http = Arc::clone(&batches);
        let limits = Arc::new(Mutex::new(f["limits"].clone()));
        let costs = Arc::new(Mutex::new(f["costs"].clone()));
        let limits_http = Arc::clone(&limits);
        let costs_http = Arc::clone(&costs);
        let changes_http = Arc::clone(&changes);
        let submissions_http = Arc::clone(&submissions);

        let market = Arc::new(Mutex::new(
            definitions["definitions"]["live"]["market"].clone(),
        ));

        let market_http = Arc::clone(&market);
        let market_overrides = Arc::new(Mutex::new(HashMap::<String, Value>::new()));
        let overrides_http = Arc::clone(&market_overrides);

        let market_gate = Arc::new(tokio::sync::Semaphore::new(1));
        let gate_http = Arc::clone(&market_gate);
        let market_read = Arc::new(tokio::sync::Notify::new());
        let read_market = Arc::clone(&market_read);
        let limits_gate = Arc::new(tokio::sync::Semaphore::new(1));
        let limits_read = Arc::new(tokio::sync::Notify::new());
        let gate_limits = Arc::clone(&limits_gate);
        let read_limits = Arc::clone(&limits_read);

        let http = tokio::spawn(async move {
            loop {
                let (mut stream, _) = http_listener.accept().await.unwrap();
                if stream.peek(&mut [0_u8]).await.unwrap() == 0 {
                    continue;
                }

                let headers = read_headers(&mut stream).await;
                let target = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let path = target.split('?').next().unwrap();

                if path == "/portfolio/events/orders/batched" {
                    batches_http
                        .respond(stream, &headers, &f, &orders_http, subaccount)
                        .await;
                    continue;
                }

                if path.starts_with("/portfolio/events/orders/") {
                    changes_http.respond(stream, &headers, &orders_http).await;
                    continue;
                }

                if path == "/portfolio/events/orders" {
                    submissions_http
                        .respond(stream, &headers, &f, &orders_http)
                        .await;
                    continue;
                }

                if matches!(
                    path,
                    "/portfolio/balance"
                        | "/portfolio/orders"
                        | "/portfolio/fills"
                        | "/portfolio/positions"
                ) {
                    assert!(
                        target.contains("exchange_index=3"),
                        "unscoped request {target}"
                    );
                    assert!(
                        target.contains(&format!("subaccount={subaccount}")),
                        "unscoped request {target}"
                    );
                }

                let body = match path {
                    "/exchange/status" => {
                        let mut status = f["status"].clone();
                        status["trading_active"] = true.into();
                        status["exchange_index_statuses"][0]["exchange_active"] = true.into();
                        status
                    }
                    "/account/limits" => limits_http.lock().unwrap().clone(),
                    "/account/endpoint_costs" => costs_http.lock().unwrap().clone(),
                    "/portfolio/balance" => {
                        let balance = current.lock().unwrap().clone();
                        reads.fetch_add(1, Ordering::SeqCst);
                        read.notify_one();
                        balance
                    }
                    "/historical/cutoff" => {
                        let mut balance = current.lock().unwrap();
                        let timestamp = balance["updated_ts"].as_i64().unwrap();
                        balance["updated_ts"] = (timestamp + 1).into();
                        let mark = balance["portfolio_value"].as_i64().unwrap();
                        balance["portfolio_value"] = (mark + 1).into();
                        f["cutoff"].clone()
                    }
                    "/portfolio/orders" => orders_http.lock().unwrap().clone(),
                    "/historical/orders" => archive_http
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| orders_http.lock().unwrap().clone()),
                    _ if path.starts_with("/portfolio/orders/") => {
                        let order = lookup_http
                            .lock()
                            .unwrap()
                            .clone()
                            .expect("unexpected order lookup");
                        assert_eq!(
                            path.strip_prefix("/portfolio/orders/").unwrap(),
                            order["order_id"].as_str().unwrap()
                        );
                        json!({"order": order})
                    }
                    "/portfolio/fills" | "/historical/fills" => fills_http.lock().unwrap().clone(),
                    "/portfolio/positions" if target.contains("settlement_status=settled") => {
                        f["empty_positions"].clone()
                    }
                    "/portfolio/positions" => {
                        let positions = positions_http.lock().unwrap().clone();
                        read_position.notify_one();
                        positions
                    }
                    "/historical/positions" => {
                        history_positions.fetch_add(1, Ordering::SeqCst);
                        f["empty_positions"].clone()
                    }
                    _ if path.starts_with("/events/") => {
                        json!({"event": definitions["definitions"]["live"]["event"], "markets": []})
                    }
                    _ if path.starts_with("/markets/") => {
                        let ticker = path.strip_prefix("/markets/").unwrap();
                        let market = overrides_http
                            .lock()
                            .unwrap()
                            .get(ticker)
                            .cloned()
                            .unwrap_or_else(|| market_http.lock().unwrap().clone());
                        json!({"market": market})
                    }
                    _ if path.starts_with("/series/") => definitions["series"].clone(),
                    _ => panic!("unexpected request {target}"),
                };

                let status = if fail == Some(path) { 403 } else { 200 };
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );

                if path == "/portfolio/balance" {
                    let gate = Arc::clone(&gate_balance);
                    tokio::spawn(async move {
                        gate.acquire_owned().await.unwrap().forget();
                        let _ = stream.write_all(response.as_bytes()).await;
                    });

                    continue;
                }

                if path.starts_with("/markets/")
                    || path.starts_with("/series/")
                    || path == "/account/limits"
                {
                    let (gate, read) = if path == "/account/limits" {
                        (Arc::clone(&gate_limits), Arc::clone(&read_limits))
                    } else if path.starts_with("/series/") {
                        (Arc::clone(&gate_series), Arc::clone(&read_series))
                    } else {
                        (Arc::clone(&gate_http), Arc::clone(&read_market))
                    };

                    tokio::spawn(async move {
                        read.notify_one();
                        let _permit = gate.acquire_owned().await.unwrap();
                        let _ = stream.write_all(response.as_bytes()).await;
                    });

                    continue;
                }

                stream.write_all(response.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
            }
        });

        let subscriptions = Arc::new(Mutex::new(Vec::new()));
        let channels = Arc::clone(&subscriptions);
        let sessions = Arc::new(AtomicUsize::new(0));
        let active = Arc::clone(&sessions);

        let notification = Arc::new(Mutex::new(None::<Value>));
        let pending = Arc::clone(&notification);
        let notification_ready = Arc::new(tokio::sync::Notify::new());

        let websocket = tokio::spawn(Self::run_websocket(
            websocket_listener,
            channels,
            active,
            pending,
            Arc::clone(&notification_ready),
        ));

        Self {
            config,
            balance,
            orders,
            order_lookup,
            orders_archive,
            fills,
            positions,
            subscriptions,
            notification,
            notification_ready,
            balance_reads,
            balance_gate,
            series_gate,
            series_read,
            history_position_reads,
            balance_read,
            position_read,
            sessions,
            submissions,
            changes,
            batches,
            limits,
            costs,
            market,
            market_overrides,
            market_gate,
            market_read,
            limits_gate,
            limits_read,
            http,
            websocket,
        }
    }

    async fn run_websocket(
        websocket_listener: tokio::net::TcpListener,
        channels: Arc<Mutex<Vec<String>>>,
        active: Arc<AtomicUsize>,
        pending: Arc<Mutex<Option<Value>>>,
        notification_ready: Arc<tokio::sync::Notify>,
    ) {
        loop {
            let (stream, _) = websocket_listener.accept().await.unwrap();
            active.fetch_add(1, Ordering::SeqCst);
            let mut ws_client = accept_async(stream).await.unwrap();
            let mut sid = 0;
            let mut private_sids = std::collections::HashMap::new();

            loop {
                let notice = tokio::select! {
                    frame = ws_client.next() => {
                let Some(frame) = frame else { break; };
                let text = match frame.unwrap() {
                    Message::Text(text) => text,
                    Message::Close(_) => {
                        let _ = ws_client.close(None).await;
                        break;
                    }
                    _ => continue,
                };

                let value: Value = serde_json::from_str(&text).unwrap();
                if value["cmd"] != "subscribe" {
                    continue;
                }

                let channel = value["params"]["channels"][0].as_str().unwrap().to_string();
                channels.lock().unwrap().push(channel.clone());
                sid += 1;
                ws_client.send(Message::text(json!({"type":"subscribed","id":value["id"],"msg":{"channel":channel,"sid":sid}}).to_string())).await.unwrap();

                private_sids.insert(channel, sid);

                if sid == 3 {
                    pending.lock().unwrap().take()
                } else {
                    None
                }
                    }
                    () = notification_ready.notified() => pending.lock().unwrap().take(),
                };

                if let Some(mut notice) = notice {
                    if notice["type"] == "disconnect" {
                        ws_client.close(None).await.unwrap();
                        break;
                    }

                    let channel = match notice["type"].as_str().unwrap() {
                        "market_position" => "market_positions",
                        "user_order" => "user_orders",
                        _ => "fill",
                    };

                    notice["sid"] = private_sids[channel].into();
                    ws_client
                        .send(Message::text(notice.to_string()))
                        .await
                        .unwrap();
                }
            }

            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.http.abort();
        self.websocket.abort();
    }
}

fn client(
    peer: &Peer,
) -> (
    KalshiExecutionClient,
    Rc<RefCell<Cache>>,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let (sender, output) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(sender);
    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        "TEST-001".into(),
        "KALSHI".into(),
        *KALSHI_VENUE,
        OmsType::Netting,
        peer.config.account_id,
        AccountType::Cash,
        Some(Currency::USD()),
        Rc::clone(&cache),
    );

    let mut client = KalshiExecutionClient::new(core, peer.config.clone()).unwrap();
    client.start().unwrap();
    (client, cache, output)
}

fn account(event: ExecutionEvent, cache: &Rc<RefCell<Cache>>) -> AccountState {
    let ExecutionEvent::Account(state) = event else {
        panic!("expected account state");
    };

    cache
        .borrow_mut()
        .add_account(AccountAny::from(state.clone()))
        .unwrap();
    state
}

fn account_or_recovered_order(
    event: ExecutionEvent,
    cache: &Rc<RefCell<Cache>>,
) -> Option<AccountState> {
    match event {
        ExecutionEvent::Account(_) => Some(account(event, cache)),
        ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, _)) => None,
        event => panic!("unexpected recovery event {event:?}"),
    }
}

#[tokio::test]
async fn bootstrap_accepts_advancing_timestamps_and_marks_and_reconnect_uses_a_new_session() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        let (mut client, cache, mut output) = client(&peer);
        let mut prior = None;

        for _ in 0..2 {
            {
                let connect = client.connect();
                tokio::pin!(connect);
                let event = tokio::select! {
                    biased;
                    result = &mut connect => panic!("connect returned before account observation: {result:?}"),
                    event = output.recv() => event.unwrap(),
                };
                std::future::poll_fn(|cx| {
                    assert!(connect.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                }).await;

                let state = account(event, &cache);
                let session = state.info.as_ref().unwrap()["kalshi_session"].clone();
                assert_ne!(prior.as_ref(), Some(&session));
                prior = Some(session);
                connect.await.unwrap();
                assert_eq!(state.balances.len(), 1);
                let balance = state.balances[0];
                assert_eq!(balance.total.as_decimal(), "1.0001".parse().unwrap());
                assert_eq!(balance.locked.as_decimal(), "0".parse().unwrap());
                assert_eq!(balance.free.as_decimal(), "1.0001".parse().unwrap());
                assert_eq!(balance.currency, Currency::USD());
                let timestamp = peer.balance.lock().unwrap()["updated_ts"].as_u64().unwrap();
                assert_eq!(state.ts_event, UnixNanos::from(timestamp * 1_000_000_000));
            }

            assert!(client.is_connected());
            client.disconnect().await.unwrap();
            assert!(!client.is_connected());
        }

        assert_eq!(*peer.subscriptions.lock().unwrap(), vec!["fill", "market_positions", "user_orders", "fill", "market_positions", "user_orders"]);
        tokio::time::timeout(Duration::from_secs(1), async {
            while peer.sessions.load(Ordering::SeqCst) != 0 { tokio::task::yield_now().await; }
        }).await.unwrap();

        client.dispose().unwrap();
        assert!(client.connect().await.unwrap_err().to_string().contains("disposed"));
    }).await.unwrap();
}

#[rstest]
#[case::aggregate("aggregate")]
#[case::cents("cents")]
#[case::dollars("dollars")]
#[tokio::test]
async fn recovery_fences_scoped_cash_and_ignores_sibling_balances(
    #[case] changed: &str,
    #[values(0, 7)] subaccount: u32,
) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let peer = Peer::with_scope(None, 2, subaccount).await;
        peer.balance_gate
            .forget_permits(peer.balance_gate.available_permits());
        let (mut client, cache, mut output) = client(&peer);
        {
            let connect = client.connect();
            tokio::pin!(connect);
            tokio::select! {
                result = &mut connect => panic!("connect returned before balance collection: {result:?}"),
                () = peer.balance_read.notified() => {},
            }

            {
                let mut balance = peer.balance.lock().unwrap();
                balance["balance_breakdown"] =
                    json!([{"exchange_index": 3, "balance": "987.6543"}]);

                match changed {
                    "cents" => {
                        let cents = balance["balance"].as_i64().unwrap();
                        balance["balance"] = (cents + 1).into();
                    }
                    "dollars" => balance["balance_dollars"] = "1.0002".into(),
                    "aggregate" => {},
                    _ => unreachable!(),
                }
            }

            peer.balance_gate.add_permits(2);
            tokio::select! {
                result = &mut connect => panic!("connect returned before second balance read: {result:?}"),
                () = peer.balance_read.notified() => {},
            }

            if changed == "aggregate" {
                let event = tokio::select! {
                    result = &mut connect => panic!("connect returned before account observation: {result:?}"),
                    () = peer.balance_read.notified() => panic!("sibling balance changes restarted recovery"),
                    event = output.recv() => event.unwrap(),
                };
                let state = account(event, &cache);
                connect.await.unwrap();
                let total = Money::from_decimal_exact(dec("1.0001"), Currency::USD()).unwrap();
                assert_eq!(
                    state.balances,
                    vec![AccountBalance::new(total, Money::zero(Currency::USD()), total)],
                );
            } else {
                tokio::select! {
                    result = &mut connect => panic!("changed cash completed recovery: {result:?}"),
                    event = output.recv() => panic!("changed cash published an event: {event:?}"),
                    () = peer.balance_read.notified() => {},
                }
                assert_eq!(peer.balance_reads.load(Ordering::SeqCst), 3);
            }
        }

        assert_eq!(client.is_connected(), changed == "aggregate");
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case::funded(false, false)]
#[case::insufficient(true, false)]
#[case::inconsistent_prices(false, true)]
#[tokio::test]
async fn external_resting_orders_reserve_both_sides_without_command_metadata(
    #[case] insufficient: bool,
    #[case] inconsistent_prices: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = if insufficient { "2.0000" } else { "3.0001" }.into();
        let fixtures: Value = serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let definitions: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut bid = fixtures["order"]["order"].clone();
        bid["ticker"] = definitions["definitions"]["live"]["market"]["ticker"].clone();
        bid["client_order_id"] = "".into();
        bid["exchange_index"] = 3.into();
        bid["book_side"] = "bid".into();
        bid["outcome_side"] = "yes".into();
        bid["fill_count_fp"] = "0.00".into();
        bid["remaining_count_fp"] = "2.00".into();
        bid["initial_count_fp"] = "2.00".into();
        bid["yes_price_dollars"] = "0.6001".into();
        bid["no_price_dollars"] = "0.3999".into();
        let mut ask = bid.clone();
        ask["order_id"] = "external-ask".into();
        ask["book_side"] = "ask".into();
        ask["outcome_side"] = "no".into();
        ask["remaining_count_fp"] = "3.00".into();
        ask["initial_count_fp"] = "3.00".into();
        ask["yes_price_dollars"] = "0.7000".into();
        ask["no_price_dollars"] = if inconsistent_prices { "0.9000" } else { "0.3000" }.into();
        *peer.orders.lock().unwrap() = json!({"orders": [bid.clone(), ask], "cursor": ""});
        let (mut client, cache, mut output) = client(&peer);

        if insufficient || inconsistent_prices {
            assert!(client.connect().await.is_err());
            assert!(!client.is_connected());
            assert!(output.try_recv().is_err());
            peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
            peer.orders.lock().unwrap()["orders"][1]["no_price_dollars"] = "0.3000".into();
        }

        for _ in 0..2 {
            {
                let connect = client.connect();
                tokio::pin!(connect);

                loop {
                    tokio::select! {
                        biased;
                        result = &mut connect => { result.unwrap(); break; }
                        event = output.recv() => {
                            let Some(state) = account_or_recovered_order(event.unwrap(), &cache) else { continue; };
                            assert_eq!(state.account_id, peer.config.account_id);
                            assert_eq!(state.balances.len(), 1);
                            assert_eq!(state.balances[0].total.as_decimal(), "3.0001".parse().unwrap());
                            assert_eq!(state.balances[0].locked.as_decimal(), "2.2382".parse().unwrap());
                            assert_eq!(state.balances[0].free.as_decimal(), "0.7619".parse().unwrap());
                            assert_eq!(state.balances[0].currency, Currency::USD());
                        }
                    }
                }
            }

            assert!(client.is_connected());
            let before = client.get_account().unwrap().last_event().unwrap();
            let mass = client.generate_mass_status(Some(60)).await.unwrap().unwrap();
            let reports = client.generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(), UnixNanos::from(1), true, None, None, None, None, None,
            )).await.unwrap();
            assert!(!mass.reports_complete());
            assert!(mass.order_reports().is_empty());
            assert!(mass.fill_reports().is_empty());
            assert!(mass.position_reports().is_empty());
            assert!(reports.is_empty());
            assert_eq!(client.get_account().unwrap().last_event().unwrap(), before);
            assert!(client.is_connected());
            assert!(cache.borrow().orders(None, None, None, None, None).is_empty());
            client.disconnect().await.unwrap();

            while let Ok(event) = output.try_recv() {
                let Some(state) = account_or_recovered_order(event, &cache) else { continue; };
                assert_eq!(state.balances, before.balances);
                assert_eq!(state.info, before.info);
            }
        }

        let mut canceled = peer.orders.lock().unwrap()["orders"][1].clone();
        canceled["status"] = "canceled".into();
        canceled["remaining_count_fp"] = "0.00".into();
        *peer.order_lookup.lock().unwrap() = Some(canceled);
        *peer.orders.lock().unwrap() = json!({"orders": [bid], "cursor": ""});
        {
            let connect = client.connect();
            tokio::pin!(connect);

            loop {
                tokio::select! {
                    biased;
                    result = &mut connect => { result.unwrap(); break; }
                    event = output.recv() => {
                        let Some(state) = account_or_recovered_order(event.unwrap(), &cache) else { continue; };
                        assert_eq!(state.balances[0].total.as_decimal(), "3.0001".parse().unwrap());
                        assert_eq!(state.balances[0].locked.as_decimal(), "1.2554".parse().unwrap());
                        assert_eq!(state.balances[0].free.as_decimal(), "1.7447".parse().unwrap());
                    }
                }
            }
        }

        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[rstest]
#[case::unknown_status("new-terminal-status", "0.00", "0.00")]
#[case::invalid_execution("executed", "0.00", "1.00")]
#[case::delayed_offsetting_fills("executed", "1.00", "0.00")]
#[tokio::test]
async fn external_order_uncertainty_cannot_release_existing_reservations(
    #[case] status: &str,
    #[case] filled: &str,
    #[case] remaining: &str,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let fixtures: Value = serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let definitions: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut bid = fixtures["order"]["order"].clone();
        bid["ticker"] = definitions["definitions"]["live"]["market"]["ticker"].clone();
        bid["client_order_id"] = "".into();
        bid["exchange_index"] = 3.into();
        bid["book_side"] = "bid".into();
        bid["outcome_side"] = "yes".into();
        bid["fill_count_fp"] = "0.00".into();
        bid["remaining_count_fp"] = "1.00".into();
        bid["initial_count_fp"] = "1.00".into();
        bid["yes_price_dollars"] = "0.6001".into();
        bid["no_price_dollars"] = "0.3999".into();
        let mut ask = bid.clone();
        ask["order_id"] = "external-ask".into();
        ask["book_side"] = "ask".into();
        ask["outcome_side"] = "no".into();
        *peer.orders.lock().unwrap() = json!({"orders": [bid, ask], "cursor": ""});
        let (mut client, cache, mut output) = client(&peer);
        let mut published = false;
        {
            let connect = client.connect();
            tokio::pin!(connect);

            loop {
                tokio::select! {
                    biased;
                    result = &mut connect => { result.unwrap(); break; }
                    event = output.recv() => {
                        let Some(state) = account_or_recovered_order(event.unwrap(), &cache) else { continue; };
                        assert_eq!(state.balances[0].total.as_decimal(), "3.0001".parse().unwrap());
                        assert_eq!(state.balances[0].locked.as_decimal(), "1.0552".parse().unwrap());
                        assert_eq!(state.balances[0].free.as_decimal(), "1.9449".parse().unwrap());
                        published = true;
                    }
                }
            }
        }

        assert!(published);
        assert!(client.is_connected());
        client.disconnect().await.unwrap();

        while output.try_recv().is_ok() {}

        for order in peer.orders.lock().unwrap()["orders"].as_array_mut().unwrap() {
            order["status"] = status.into();
            order["last_update_time"] = jiff::Timestamp::now().to_string().into();
            order["fill_count_fp"] = filled.into();
            order["remaining_count_fp"] = remaining.into();
        }

        assert!(peer.fills.lock().unwrap()["fills"].as_array().unwrap().is_empty());
        assert!(peer.positions.lock().unwrap()["market_positions"].as_array().unwrap().is_empty());

        assert!(client.connect().await.is_err());
        assert!(!client.is_connected());
        assert!(output.try_recv().is_err(), "uncertain external orders published account effects");
        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn canceling_an_old_external_order_does_not_replay_unrelated_fills() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let instrument = bootstrap_instrument();
        let mut resting = fixtures["order"]["order"].clone();
        resting["ticker"] = instrument.id.symbol.as_str().into();
        resting["order_id"] = "old-resting".into();
        resting["client_order_id"] = "".into();
        resting["created_time"] = "2026-01-01T00:00:00Z".into();
        resting["exchange_index"] = 3.into();
        resting["book_side"] = "bid".into();
        resting["outcome_side"] = "yes".into();
        resting["fill_count_fp"] = "0.00".into();
        resting["remaining_count_fp"] = "2.00".into();
        resting["initial_count_fp"] = "2.00".into();
        resting["yes_price_dollars"] = "0.6001".into();
        resting["no_price_dollars"] = "0.3999".into();
        let mut historical_orders = Vec::new();
        let mut historical_fills = Vec::new();

        for (index, side, outcome) in [(0, "bid", "yes"), (1, "ask", "no")] {
            let mut order = resting.clone();
            order["order_id"] = format!("historical-{index}").into();
            order["book_side"] = side.into();
            order["outcome_side"] = outcome.into();
            order["status"] = "executed".into();
            order["fill_count_fp"] = "1.00".into();
            order["remaining_count_fp"] = "0.00".into();
            order["initial_count_fp"] = "1.00".into();
            order["last_update_time"] = "2026-10-16T00:00:01Z".into();
            let mut fill = fixtures["fills"]["fills"][0].clone();
            fill["fill_id"] = format!("historical-fill-{index}").into();
            fill["order_id"] = order["order_id"].clone();
            fill["ticker"] = order["ticker"].clone();
            fill["market_ticker"] = order["ticker"].clone();
            fill["book_side"] = side.into();
            fill["outcome_side"] = outcome.into();
            fill["exchange_index"] = 3.into();
            fill["count_fp"] = "1.00".into();
            fill["created_time"] = "2026-10-16T00:00:01Z".into();
            historical_orders.push(order);
            historical_fills.push(fill);
        }

        let mut orders = historical_orders.clone();
        orders.push(resting.clone());
        *peer.orders.lock().unwrap() = json!({"orders": orders, "cursor": ""});
        *peer.fills.lock().unwrap() = json!({"fills": historical_fills, "cursor": ""});
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        let cache = harness.cache();
        let queued = connect_consuming_accounts(&mut client, &mut output).await;
        replay_bootstrap(&queued);
        assert!(client.is_connected());
        let state = client.get_account().unwrap().last_event().unwrap();
        assert_eq!(
            state.balances[0].locked.as_decimal(),
            "1.2554".parse().unwrap()
        );
        resting["status"] = "canceled".into();
        resting["remaining_count_fp"] = "0.00".into();
        *peer.order_lookup.lock().unwrap() = Some(resting);
        *peer.orders.lock().unwrap() = json!({"orders": historical_orders, "cursor": ""});
        let notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_historical"]
                .clone();
        *peer.notification.lock().unwrap() = Some(notification);
        peer.notification_ready.notify_one();
        let mut queued = Vec::new();

        loop {
            match output.recv().await.unwrap() {
                ExecutionEvent::Account(state) => {
                    account(ExecutionEvent::Account(state.clone()), cache);
                    if state.balances[0].locked.as_decimal().is_zero() {
                        assert_eq!(
                            state.balances[0].total.as_decimal(),
                            "3.0001".parse().unwrap()
                        );
                        assert_eq!(
                            state.balances[0].free.as_decimal(),
                            "3.0001".parse().unwrap()
                        );
                        break;
                    }
                }
                event => queued.push(event),
            }
        }

        replay_bootstrap(&queued);

        while !client.is_connected() {
            tokio::task::yield_now().await;
        }

        client.disconnect().await.unwrap();
        let queued = connect_consuming_accounts(&mut client, &mut output).await;
        replay_bootstrap(&queued);
        assert!(client.is_connected());
        let state = client.get_account().unwrap().last_event().unwrap();
        assert_eq!(state.balances[0].locked.as_decimal(), Decimal::ZERO);
        assert_eq!(
            state.balances[0].free.as_decimal(),
            "3.0001".parse().unwrap()
        );
        {
            let cache = cache.borrow();
            let orders = cache.orders(None, None, None, None, None);
            assert_eq!(orders.len(), 1);
            assert_eq!(
                orders[0].client_order_id(),
                ClientOrderId::from("old-resting")
            );
            assert_eq!(orders[0].status(), OrderStatus::Canceled);
            assert!(orders[0].trade_ids().is_empty());
        }

        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn late_fill_on_recovered_external_order_rewinds_fill_history() {
    late_fill_on_recovered_external_order(false).await;
}

#[tokio::test]
async fn late_fill_on_claimed_external_order_rewinds_fill_history() {
    late_fill_on_recovered_external_order(true).await;
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
async fn late_fill_on_recovered_external_order(claimed: bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, claimed);
        filled_bootstrap(&peer, &instrument, false, [Decimal::ZERO; 2]);
        let claimant = claimed.then(|| bootstrap_ownership(&harness, &instrument, false, true));
        peer.orders.lock().unwrap()["orders"][0]["status"] = "resting".into();
        peer.orders.lock().unwrap()["orders"][0]["initial_count_fp"] = "1.00".into();
        peer.orders.lock().unwrap()["orders"][0]["remaining_count_fp"] = "0.25".into();
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        harness
            .clock()
            .borrow_mut()
            .as_any_mut()
            .downcast_mut::<VirtualClock>()
            .unwrap()
            .set_time(nautilus_core::time::get_atomic_clock_realtime().get_time_ns());
        replay_bootstrap(&queued);
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let id = ClientOrderId::from("order-a");
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().filled_qty(),
            Quantity::from("0.75")
        );
        let mut canceled = peer.orders.lock().unwrap()["orders"][0].clone();
        canceled["status"] = "canceled".into();
        canceled["remaining_count_fp"] = "0.00".into();
        let canceled_ms = canceled["last_update_time"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        *peer.order_lookup.lock().unwrap() = Some(canceled.clone());
        *peer.orders.lock().unwrap() = json!({"orders": [canceled], "cursor": ""});

        if let Some(claimant) = claimant {
            let cached = harness.cache().borrow().order(&id).unwrap().clone();
            peer.changes.ack_gate.add_permits(1);

            let cancel = CancelOrder::new(
                cached.trader_id(),
                Some(harness.client_id()),
                claimant,
                instrument.id,
                id,
                cached.venue_order_id(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            );

            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .cancel_order(cancel)
                .unwrap();
        } else {
            let mut notification = serde_json::from_str::<Value>(include_str!(
                "../../test_data/websocket_messages.json"
            ))
            .unwrap()["order_canceled_partial"]
                .clone();
            notification["msg"]["last_updated_ts_ms"] = serde_json::json!(canceled_ms);
            *peer.notification.lock().unwrap() = Some(notification);

            peer.notification_ready.notify_one();
        }

        let mut canceled_events = Vec::new();

        loop {
            match output.recv().await.unwrap() {
                ExecutionEvent::Account(state) => {
                    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state));
                    let connected = harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected();

                    if connected {
                        break;
                    }
                }
                event => canceled_events.push(event),
            }
        }

        replay_bootstrap(&canceled_events);
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().status(),
            OrderStatus::Canceled
        );
        let late = peer.fills.lock().unwrap()["fills"][1]["created_time"]
            .as_str()
            .unwrap()
            .to_string();
        let late_ms = late.parse::<jiff::Timestamp>().unwrap().as_millisecond();
        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        source["fill_count_fp"] = "1.00".into();
        source["initial_count_fp"] = "1.00".into();
        source["last_update_time"] = late.clone().into();
        *peer.order_lookup.lock().unwrap() = Some(source.clone());
        *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
        let mut fill = peer.fills.lock().unwrap()["fills"][1].clone();
        fill["fill_id"] = "fill-c".into();
        fill["count_fp"] = "0.25".into();
        fill["created_time"] = late.clone().into();
        peer.fills.lock().unwrap()["fills"]
            .as_array_mut()
            .unwrap()
            .push(fill);
        peer.positions.lock().unwrap()["market_positions"][0]["position_fp"] = "1.00".into();
        peer.positions.lock().unwrap()["market_positions"][0]["last_updated_ts"] = late.into();
        peer.balance.lock().unwrap()["balance_dollars"] = "0.8500".into();
        let mut notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_late_fill"]
                .clone();
        notification["msg"]["last_updated_ts_ms"] = serde_json::json!(late_ms);
        *peer.notification.lock().unwrap() = Some(notification);

        peer.notification_ready.notify_one();
        let mut queued = Vec::new();

        loop {
            match output.recv().await.unwrap() {
                ExecutionEvent::Account(state) => {
                    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state));
                    let connected = harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected();

                    if connected {
                        break;
                    }
                }
                event => queued.push(event),
            }
        }

        replay_bootstrap(&queued);
        let cache = harness.cache();
        let cached = cache.borrow().order(&id).unwrap().clone();
        assert_eq!(cached.filled_qty(), Quantity::from("1.00"));
        assert_eq!(
            cached.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            vec![
                TradeId::from("fill-a"),
                TradeId::from("fill-b"),
                TradeId::from("fill-c")
            ]
        );
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("/portfolio/balance")]
#[case("/portfolio/orders")]
#[case("/portfolio/positions")]
#[tokio::test]
async fn failed_bootstrap_closes_the_started_private_transport(#[case] path: &'static str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(Some(path)).await;
        let (mut client, _, mut output) = client(&peer);
        assert!(client.connect().await.is_err());
        assert!(!client.is_connected());
        assert!(output.try_recv().is_err());
        assert_eq!(peer.subscriptions.lock().unwrap().len(), 3);
        tokio::time::timeout(Duration::from_secs(1), async {
            while peer.sessions.load(Ordering::SeqCst) != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn query_account_accepts_authoritative_non_fill_cash_changes_and_rejects_wrong_identity() {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::new(None).await;
        let (mut client, cache, mut output) = client(&peer);
        {
            let connect = client.connect();
            tokio::pin!(connect);

            loop {
                tokio::select! {
                    biased;
                    result = &mut connect => { result.unwrap(); break; }
                    event = output.recv() => { account(event.unwrap(), &cache); }
                }
            }
        }

        let command = QueryAccount::new(
            "TEST-001".into(),
            Some(client.client_id()),
            client.account_id(),
            UUID4::new(),
            UnixNanos::from(432),
            None,
            None,
        );

        let mut wrong = command.clone();
        wrong.account_id = AccountId::from("KALSHI-OTHER");
        assert!(
            client
                .query_account(wrong)
                .unwrap_err()
                .to_string()
                .contains("identity")
        );
        peer.balance.lock().unwrap()["balance_dollars"] = "2.0002".into();
        client.query_account(command).unwrap();

        loop {
            let state = account(output.recv().await.unwrap(), &cache);
            if state.balances[0].total.as_decimal() == "2.0002".parse().unwrap() {
                assert_eq!(
                    state.balances[0].free.as_decimal(),
                    "2.0002".parse().unwrap()
                );
                break;
            }
        }

        assert!(client.is_connected());
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false, false, false)]
#[case(true, false, false)]
#[case(true, true, false)]
#[case(false, false, true)]
#[tokio::test]
async fn venue_binding_prevents_a_stale_cached_order_from_overwriting_snapshot_holds(
    #[case] stale: bool,
    #[case] terminal: bool,
    #[case] insufficient: bool,
) {
    use nautilus_kalshi::http::{
        models::{Event, Field, Market},
        parse::parse_instrument,
    };
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        events::{OrderAccepted, OrderEventAny, OrderPendingCancel},
        identifiers::{ClientOrderId, VenueOrderId},
        instruments::InstrumentAny,
        orders::{Order, builder::OrderTestBuilder},
        types::{Price, Quantity},
    };
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let fixtures: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market: Market = serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        let mut event: Event = serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        event.exchange_index = Field::Value(3);
        market.exchange_index = Field::Value(3);
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let (mut client, cache, mut output) = client(&peer);
        cache.borrow_mut().add_instrument(InstrumentAny::BinaryOption(instrument.clone())).unwrap();
        let mut order = OrderTestBuilder::new(OrderType::Limit).instrument_id(instrument.id).trader_id("TEST-001".into()).client_order_id(ClientOrderId::from("client-a")).side(OrderSide::Buy).quantity(if stale { Quantity::from("2.00") } else { Quantity::from("1.00") }).price(if stale { Price::from("0.600100") } else { Price::from("0.300000") }).build();
        order.apply(OrderEventAny::Accepted(OrderAccepted::new(order.trader_id(), order.strategy_id(), instrument.id, order.client_order_id(), VenueOrderId::from("order-a"), client.account_id(), UUID4::new(), if stale { nautilus_core::datetime::try_datetime_to_unix_nanos("2026-10-16T09:09:09Z".parse().unwrap()).unwrap() } else { UnixNanos::from(2) }, UnixNanos::from(3), false))).unwrap();
        if terminal {
            order.apply(OrderEventAny::PendingCancel(OrderPendingCancel::new(order.trader_id(), order.strategy_id(), instrument.id, order.client_order_id(), order.account_id(), UUID4::new(), UnixNanos::from(order.ts_last().as_u64()+1), UnixNanos::from(4), false, order.venue_order_id()))).unwrap();
        }

        cache.borrow_mut().add_order(order, None, Some(client.client_id()), false).unwrap();
        let sources: Value = serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = sources["order"]["order"].clone();
        source["ticker"] = instrument.id.symbol.as_str().into();
        source["client_order_id"] = "".into();
        source["exchange_index"] = 3.into();
        source["book_side"] = "bid".into();
        source["outcome_side"] = "yes".into();
        source["fill_count_fp"] = "0.00".into();
        source["remaining_count_fp"] = "2.00".into();
        source["yes_price_dollars"] = "0.6001".into();
        source["no_price_dollars"] = "0.3999".into();
        source["initial_count_fp"] = "2.00".into();
        if stale {
            source["remaining_count_fp"] = "1.00".into();
            source["yes_price_dollars"] = "0.3000".into();
            source["no_price_dollars"] = "0.7000".into();
        }

        if terminal {
            source["status"] = "canceled".into();
            source["remaining_count_fp"] = "0.00".into();
        }

        *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
        if insufficient {
            peer.balance.lock().unwrap()["balance_dollars"] = "0.5000".into();
            assert!(client.connect().await.unwrap_err().to_string().contains("bootstrap timed out"));
            assert!(output.try_recv().is_err(), "failed financial preview emitted execution effects");
            assert!(!client.is_connected());
            peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        }

        let mut updates = Vec::new();
        let mut balances = Vec::new();
        {
            let connect = client.connect();
            tokio::pin!(connect);

            loop {
                tokio::select! {
                    biased;
                    result = &mut connect => { result.unwrap(); break; }
                    event = output.recv() => match event.unwrap() {
                        ExecutionEvent::Account(state) => { balances.push(state.balances[0]); account(ExecutionEvent::Account(state), &cache); }
                        ExecutionEvent::Order(OrderEventAny::Updated(event)) => updates.push(event),
                        event => panic!("unexpected event {event:?}"),
                    }
                }
            }
        }

        assert_eq!(updates.len(), usize::from(!stale));

        if !stale {
            assert_eq!(updates[0].quantity, Quantity::from("2.00"));
            assert_eq!(updates[0].price, Some(Price::from("0.600100")));
        }

        let balance = balances.last().unwrap();
        assert_eq!(balance.total.as_decimal(), "3.0001".parse().unwrap());
        assert_eq!(balance.locked.as_decimal(), "1.2554".parse().unwrap());
        assert_eq!(balance.free.as_decimal(), "1.7447".parse().unwrap());
        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[rstest]
#[case(false, 0)]
#[case(true, 0)]
#[case(false, 1)]
#[case(false, 2)]
#[tokio::test]
async fn persisted_close_and_received_fill_evidence_fence_account_recovery(
    #[case] closed: bool,
    #[case] delay: u8,
) {
    use nautilus_kalshi::http::{
        models::{Event, Field, Market},
        parse::parse_instrument,
    };
    use nautilus_model::{
        data::InstrumentClose,
        enums::{InstrumentCloseType, OrderSide, OrderType},
        events::{OrderAccepted, OrderEventAny},
        instruments::InstrumentAny,
        orders::{Order, builder::OrderTestBuilder},
        types::{Price, Quantity},
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut peer = Peer::new(None).await;
        let delayed = delay != 0;
        if delayed {
            peer.config.ws_timeout_secs = 8;
            peer.balance_gate.forget_permits(peer.balance_gate.available_permits());
            peer.balance_gate.add_permits(2);
        }

        let definitions: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market: Market =
            serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let mut event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        market.exchange_index = Field::Value(3);
        event.exchange_index = Field::Value(3);
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let (mut client, cache, mut output) = client(&peer);
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::BinaryOption(instrument.clone()))
            .unwrap();
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .trader_id("TEST-001".into())
            .client_order_id("client-a".into())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.600100"))
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                "order-a".into(),
                client.account_id(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(3),
                false,
            )))
            .unwrap();
        cache
            .borrow_mut()
            .add_order(order, None, Some(client.client_id()), false)
            .unwrap();

        if closed {
        cache
            .borrow_mut()
            .add_instrument_close(InstrumentClose::new(
                instrument.id,
                Price::from("1.000000"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(100),
                UnixNanos::from(101),
            ))
            .unwrap();
        }

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = fixtures["order"]["order"].clone();
        source["ticker"] = instrument.id.symbol.as_str().into();
        source["client_order_id"] = "client-a".into();
        source["order_id"] = "order-a".into();
        source["exchange_index"] = 3.into();
        source["book_side"] = "bid".into();
        source["outcome_side"] = "yes".into();
        source["status"] = "canceled".into();
        source["fill_count_fp"] = "0.75".into();
        source["remaining_count_fp"] = "0.00".into();
        source["yes_price_dollars"] = "0.6001".into();
        source["no_price_dollars"] = "0.3999".into();
        source["initial_count_fp"] = "2.00".into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["ticker"] = instrument.id.symbol.as_str().into();
        fill["market_ticker"] = instrument.id.symbol.as_str().into();
        fill["order_id"] = "order-a".into();
        fill["exchange_index"] = 3.into();
        fill["book_side"] = "bid".into();
        fill["outcome_side"] = "yes".into();
        fill["count_fp"] = "0.75".into();
        fill["yes_price_dollars"] = "0.6001".into();
        fill["no_price_dollars"] = "0.3999".into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = instrument.id.symbol.as_str().into();
        position["exchange_index"] = 3.into();
        position["position_fp"] = "0.75".into();
        *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
        *peer.fills.lock().unwrap() = json!({"fills": [fill], "cursor": ""});
        *peer.positions.lock().unwrap() = json!({"market_positions": [position], "event_positions": [], "cursor": ""});
        let eventual = if delayed {
            let orders = peer.orders.lock().unwrap().clone();
            let fills = peer.fills.lock().unwrap().clone();
            let positions = peer.positions.lock().unwrap().clone();
            let mut initial = orders.clone();
            initial["orders"][0]["fill_count_fp"] = "0.00".into();
            *peer.orders.lock().unwrap() = initial;
            *peer.fills.lock().unwrap() = fixtures["empty_fills"].clone();
            *peer.positions.lock().unwrap() = fixtures["empty_positions"].clone();
            let mut notice = fills["fills"][0].clone();
            notice["trade_id"] = notice["fill_id"].clone();
            notice["subaccount"] = 0.into();
            notice["fee_cost"] = "malformed".into();

            let notification = if delay == 2 {
                let position = &positions["market_positions"][0];
                let mut notification = serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json")).unwrap()["position_notice"].clone();
                notification["msg"]["market_ticker"] = serde_json::json!(position["ticker"]);
                notification["msg"]["position_fp"] = serde_json::json!(position["position_fp"]);
                notification["msg"]["realized_pnl_dollars"] = serde_json::json!((position["realized_pnl_dollars"].as_str().unwrap().parse::<Decimal>().unwrap() - position["fees_paid_dollars"].as_str().unwrap().parse::<Decimal>().unwrap()).to_string());
                notification["msg"]["fees_paid_dollars"] = serde_json::json!(position["fees_paid_dollars"]);
                notification
            } else {json!({"type":"fill", "sid":0, "msg":notice})};

            *peer.notification.lock().unwrap() = Some(notification);

            Some((orders, fills, positions))
        } else { None };

        if !closed {
            let mut fills = Vec::new();
            let mut account_seen = false;
            let mut history_reads_before = 0;
            {
                let connect = client.connect();
                tokio::pin!(connect);

                if let Some((orders, source_fills, positions)) = eventual {
                    for reads in [3, 4] {
                        loop {
                            let observed = peer.balance_read.notified();
                            if peer.balance_reads.load(Ordering::SeqCst) == reads { break; }
                            tokio::select! {
                                biased;
                                result = &mut connect => panic!("connect covered an unavailable notified fill: {result:?}"),
                                event = output.recv() => panic!("execution escaped before notified fill became available: {event:?}"),
                                () = observed => {},
                            }
                        }

                        if reads == 3 {
                            // The notice can reach recovery before or during the first collection
                            history_reads_before = peer.history_position_reads.load(Ordering::SeqCst);
                            peer.balance_gate.add_permits(1);
                        }
                    }

                    assert!(output.try_recv().is_err());
                    *peer.orders.lock().unwrap() = orders;
                    *peer.fills.lock().unwrap() = source_fills;
                    *peer.positions.lock().unwrap() = positions;
                    peer.balance_gate.add_permits(tokio::sync::Semaphore::MAX_PERMITS);
                }

                loop {
                    tokio::select! {
                        biased;
                        result = &mut connect => {result.unwrap(); break;}
                        event = output.recv() => match event.unwrap() {
                            ExecutionEvent::Account(state) => {
                                if delayed { assert_eq!(fills.len(), 1, "account recovery overtook notified fill"); }
                                account_seen = true;
                                account(ExecutionEvent::Account(state), &cache);
                            }
                            ExecutionEvent::Order(event) => {
                                if let OrderEventAny::Filled(fill) = &event {fills.push(fill.clone());}
                                cache.borrow_mut().update_order(&event).unwrap();
                            }
                            ExecutionEvent::Report(_) => {},
                            event => panic!("unexpected event {event:?}"),
                        }
                    }
                }
            }

            assert!(account_seen);
            assert_eq!(peer.history_position_reads.load(Ordering::SeqCst) - history_reads_before, usize::from(delay == 2), "only an uncovered position notice needs settled-history coverage for a flat cached scope after the first collection");
            assert_eq!(fills.len(), 1);
            assert_eq!(fills[0].trade_id.as_str(), fixtures["fills"]["fills"][0]["fill_id"].as_str().unwrap());
            assert_eq!(fills[0].last_qty, Quantity::from("0.75"));
            assert!(client.is_connected());
            client.disconnect().await.unwrap();
            return;
        }

        let result = client.connect().await;
        assert!(output.try_recv().is_err(), "late fill escaped cached-close protection");
        assert!(result.unwrap_err().to_string().contains("bootstrap timed out"));
        assert!(!client.is_connected());
        assert_eq!(
            cache
                .borrow()
                .order_ref(&"client-a".into())
                .unwrap()
                .filled_qty(),
            Quantity::from("0.00")
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_filled_order_recovers_from_flat_without_cached_command_metadata() {
    use nautilus_common::messages::ExecutionReport;
    use nautilus_kalshi::http::{
        models::{Event, Field, Market},
        parse::parse_instrument,
    };
    use nautilus_model::{
        enums::OrderStatus,
        types::{Price, Quantity},
    };

    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        let definitions: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market: Market = serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let mut event: Event = serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        market.exchange_index = Field::Value(3);
        event.exchange_index = Field::Value(3);
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let (mut client, cache, mut output) = client(&peer);
        cache.borrow_mut().add_instrument(instrument.clone().into()).unwrap();
        let fixtures: Value = serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let now = jiff::Timestamp::now().to_string();
        let mut source = fixtures["order"]["order"].clone();
        source["ticker"] = instrument.id.symbol.as_str().into();
        source["client_order_id"] = "".into();
        source["exchange_index"] = 3.into();
        source["book_side"] = "bid".into();
        source["outcome_side"] = "yes".into();
        source["status"] = "executed".into();
        source["fill_count_fp"] = "0.75".into();
        source["remaining_count_fp"] = "0.00".into();
        source["yes_price_dollars"] = "0.6001".into();
        source["no_price_dollars"] = "0.3999".into();
        source["last_update_time"] = now.clone().into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["ticker"] = instrument.id.symbol.as_str().into();
        fill["market_ticker"] = instrument.id.symbol.as_str().into();
        fill["exchange_index"] = 3.into();
        fill["book_side"] = "bid".into();
        fill["outcome_side"] = "yes".into();
        fill["count_fp"] = "0.75".into();
        fill["yes_price_dollars"] = "0.6001".into();
        fill["no_price_dollars"] = "0.3999".into();
        fill["created_time"] = now.clone().into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = instrument.id.symbol.as_str().into();
        position["exchange_index"] = 3.into();
        position["position_fp"] = "0.75".into();
        position["last_updated_ts"] = now.into();
        *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
        *peer.fills.lock().unwrap() = json!({"fills": [fill], "cursor": ""});
        *peer.positions.lock().unwrap() = json!({"market_positions": [position], "event_positions": [], "cursor": ""});

        let mut fills = Vec::new();
        let mut statuses = Vec::new();
        {
            let connect = client.connect();
            tokio::pin!(connect);

            loop {
                tokio::select! {
                    biased;
                    result = &mut connect => { result.unwrap(); break; }
                    event = output.recv() => match event.unwrap() {
                        ExecutionEvent::Account(state) => { account(ExecutionEvent::Account(state), &cache); }
                        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => fills.push(fill),
                        ExecutionEvent::Report(ExecutionReport::OrderWithFills(report, fills)) => {
                            assert!(fills.is_empty()); statuses.push(report);
                        }
                        ExecutionEvent::Report(ExecutionReport::Position(report)) => assert_eq!(report.signed_decimal_qty, Decimal::new(75, 2)),
                        event => panic!("unexpected event {event:?}"),
                    }
                }
            }
        }

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id.as_str(), "fill-a");
        assert_eq!(fills[0].client_order_id.unwrap().as_str(), "order-a");
        assert_eq!(fills[0].last_qty, Quantity::from("0.75"));
        assert_eq!(fills[0].last_px, Price::from("0.600100"));
        assert_eq!(fills[0].commission.as_decimal(), Decimal::new(1, 6));
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].order_status, OrderStatus::Accepted);
        assert_eq!(statuses[0].filled_qty, Quantity::from("0.00"));
        assert_eq!(statuses[1].order_status, OrderStatus::Filled);
        assert_eq!(statuses[1].filled_qty, Quantity::from("0.75"));
        assert!(client.is_connected());
        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[rstest]
#[case(false, false, false, false)]
#[case(true, false, true, false)]
#[case(false, false, true, true)]
#[case(false, true, false, false)]
#[tokio::test]
async fn startup_reconciliation_preserves_exact_execution(
    #[case] short: bool,
    #[case] owned: bool,
    #[case] claimed: bool,
    #[case] retained: bool,
    #[values("0", "0.01", "0.000001")] fee: &str,
) {
    let fee = fee.parse::<Decimal>().unwrap();
    reconcile_bootstrap(short, owned, claimed, retained, [fee, fee * Decimal::TWO]).await;
}

#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn startup_rejects_missing_fills_or_opening_exposure(#[case] missing_fill: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        filled_bootstrap(&peer, &instrument, false, [Decimal::ZERO; 2]);
        if missing_fill {
            peer.fills.lock().unwrap()["fills"]
                .as_array_mut()
                .unwrap()
                .pop();
        } else {
            peer.positions.lock().unwrap()["market_positions"][0]["position_fp"] = "1.75".into();
        }

        let error = client.connect().await.unwrap_err();

        if missing_fill {
            assert!(error.to_string().contains("bootstrap timed out"), "{error}");
        } else {
            assert_eq!(
                error.to_string(),
                "KALSHI_OPENING_CONTEXT_REQUIRED: position cycle lacks cached opening authority; restore complete cached orders and positions"
            );
        }

        assert!(!client.is_connected());
        assert!(output.try_recv().is_err());
        assert!(
            harness
                .cache()
                .borrow()
                .account(&harness.account_id())
                .is_none()
        );
        assert!(
            harness
                .cache()
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        assert!(
            harness
                .cache()
                .borrow()
                .positions(None, None, None, None, None)
                .is_empty()
        );
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
#[tokio::test]
async fn startup_recovers_resting_external_order_for_mass_status() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let instrument = bootstrap_instrument();
        resting_bootstrap(&peer, &instrument);
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let id = ClientOrderId::from("order-a");
        let cache = harness.cache();
        let cached = cache.borrow().order(&id).unwrap().clone();
        assert_eq!(cached.venue_order_id(), Some(VenueOrderId::from("order-a")));
        assert_eq!(cached.strategy_id(), StrategyId::external());
        assert_eq!(cached.status(), OrderStatus::Accepted);
        assert_eq!(cached.quantity(), Quantity::from("2.00"));
        assert_eq!(cached.filled_qty(), Quantity::from("0.00"));
        assert_eq!(cached.price(), Some(Price::from("0.600100")));
        assert_eq!(cached.time_in_force(), TimeInForce::Gtc);
        assert!(!cached.is_post_only());
        assert!(!cached.is_reduce_only());
        let mass = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_mass_status(Some(60))
            .await
            .unwrap()
            .unwrap();
        assert!(mass.reports_complete());
        assert_eq!(mass.order_reports().len(), 1);
        assert!(mass.fill_reports().is_empty());
        assert!(mass.position_reports().is_empty());
        let report = &mass.order_reports()[&VenueOrderId::from("order-a")];
        assert_eq!(report.client_order_id, Some(id));
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from("2.00"));
        assert_eq!(report.filled_qty, Quantity::from("0.00"));
        assert_eq!(report.price, Some(Price::from("0.600100")));
        assert_eq!(cache.borrow().client_id(&id), Some(&harness.client_id()));
        peer.changes.ack_gate.add_permits(1);

        let cancel = CancelOrder::new(
            cached.trader_id(),
            Some(harness.client_id()),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            cached.venue_order_id(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .cancel_order(cancel)
            .unwrap();

        loop {
            let connected = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();

            if cache.borrow().order(&id).unwrap().status() == OrderStatus::Canceled && connected {
                break;
            }

            let event = output.recv().await.unwrap();
            AsyncRunner::handle_exec_event(event);
        }

        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        assert!(peer.orders.lock().unwrap()["orders"][0]["status"] == "canceled");
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
#[tokio::test]
async fn startup_filtering_omits_unclaimed_resting_order() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let instrument = bootstrap_instrument();
        resting_bootstrap(&peer, &instrument);
        let (harness, mut client, mut output, _data) = bootstrap_consumer(&peer, &instrument, true);
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        assert!(
            harness
                .cache()
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        let mass = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_mass_status(Some(60))
            .await
            .unwrap()
            .unwrap();
        assert!(!mass.reports_complete());
        assert!(mass.order_reports().is_empty());
        assert!(mass.fill_reports().is_empty());
        assert!(mass.position_reports().is_empty());
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let mut canceled = peer.orders.lock().unwrap()["orders"][0].clone();
        canceled["status"] = "canceled".into();
        canceled["remaining_count_fp"] = "0.00".into();
        canceled["last_update_time"] = jiff::Timestamp::now().to_string().into();
        let canceled_ms = canceled["last_update_time"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        *peer.order_lookup.lock().unwrap() = Some(canceled.clone());
        *peer.orders.lock().unwrap() = json!({"orders": [canceled], "cursor": ""});
        let mut notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_canceled_unfilled"]
                .clone();
        notification["msg"]["last_updated_ts_ms"] = serde_json::json!(canceled_ms);
        *peer.notification.lock().unwrap() = Some(notification);

        peer.notification_ready.notify_one();
        let mut queued = Vec::new();

        loop {
            match output.recv().await.unwrap() {
                ExecutionEvent::Account(state) => {
                    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state));
                    let connected = harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected();

                    if connected {
                        break;
                    }
                }
                event => queued.push(event),
            }
        }

        replay_bootstrap(&queued);
        let mass = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_mass_status(Some(60))
            .await
            .unwrap()
            .unwrap();
        assert!(!mass.reports_complete());
        assert!(mass.order_reports().is_empty());
        assert!(mass.fill_reports().is_empty());
        assert!(
            harness
                .cache()
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness connects one client while applying cached accounts"
)]
#[tokio::test]
async fn startup_claimed_amend_rejects_newer_private_terms() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.market.lock().unwrap()["open_time"] = "2020-01-01T00:00:00Z".into();
        peer.market.lock().unwrap()["close_time"] = "2100-01-01T00:00:00Z".into();
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let instrument = bootstrap_instrument();
        resting_bootstrap(&peer, &instrument);
        let (harness, mut client, mut output, _data) = bootstrap_consumer(&peer, &instrument, true);
        let claimant = bootstrap_ownership(&harness, &instrument, false, true);
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        let id = ClientOrderId::from("order-a");
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let session = harness
            .cache()
            .borrow()
            .account(&harness.account_id())
            .unwrap()
            .last_event()
            .unwrap()
            .info;
        {
            let mut rows = peer.orders.lock().unwrap();
            rows["orders"][0]["yes_price_dollars"] = "0.5500".into();
            rows["orders"][0]["no_price_dollars"] = "0.4500".into();
            rows["orders"][0]["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }

        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_native"]
                .clone();
        notice["msg"]["order_id"] = "order-a".into();
        *peer.notification.lock().unwrap() = Some(notice);
        peer.notification_ready.notify_one();
        let mut recovered = Vec::new();

        loop {
            let event = output.recv().await.unwrap();
            let observed = matches!(&event, ExecutionEvent::Account(account) if account.info != session);
            recovered.push(event);

            if observed {
                break;
            }
        }

        assert_eq!(harness.cache().borrow().order(&id).unwrap().price(), cached.price());
        let command_id = native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        native_dispatch(recovered);

        let rejected = loop {
            let _ = harness.exec_engine().borrow().get_client(&harness.client_id()).unwrap().is_connected();
            let event = tokio::select! {
                () = peer.changes.written.notified() => panic!("claimed stale terms reached the venue"),
                event = output.recv() => event.unwrap(),
            };

            if let ExecutionEvent::Order(OrderEventAny::ModifyDenied(rejected)) = &event {
                let rejected = *rejected;
                AsyncRunner::handle_exec_event(event);
                break rejected;
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert_eq!(rejected.strategy_id, claimant);
        assert_eq!(rejected.causation_id, Some(command_id));
        assert_eq!(rejected.client_order_id, id);
        assert_eq!(rejected.reason.as_str(), "VALIDATION_FAILED: Kalshi order changed before admission");
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        let current = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(current.strategy_id(), claimant);
        assert_eq!(current.price(), Some(Price::from("0.550000")));
        assert_eq!(current.quantity(), cached.quantity());
        assert_eq!(current.filled_qty(), cached.filled_qty());
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn startup_claimed_resting_order_cancels_with_claimant() {
    claimed_resting_order_cancels_with_claimant(false).await;
}

#[tokio::test]
async fn startup_claimed_resting_order_cancel_all_with_claimant() {
    claimed_resting_order_cancels_with_claimant(true).await;
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
#[tokio::test]
async fn startup_claimed_resting_order_cancel_rejection_preserves_claimant() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let instrument = bootstrap_instrument();
        resting_bootstrap(&peer, &instrument);
        let (harness, mut client, mut output, _data) = bootstrap_consumer(&peer, &instrument, true);
        let claimant = bootstrap_ownership(&harness, &instrument, false, true);
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        let id = ClientOrderId::from("order-a");
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(cached.strategy_id(), claimant);
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let command_id = UUID4::new();

        let mut pending = OrderPendingCancel::new(
            cached.trader_id(),
            claimant,
            instrument.id,
            id,
            Some(harness.account_id()),
            UUID4::new(),
            now,
            now,
            false,
            cached.venue_order_id(),
        );

        pending.causation_id = Some(command_id);
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::PendingCancel(
            pending,
        )));
        *peer.changes.mode.lock().unwrap() = "reject".into();

        let cancel = CancelOrder::new(
            cached.trader_id(),
            Some(harness.client_id()),
            claimant,
            instrument.id,
            id,
            cached.venue_order_id(),
            command_id,
            UnixNanos::default(),
            None,
            None,
        );

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .cancel_order(cancel)
            .unwrap();
        peer.changes.written.notified().await;
        let newer = jiff::Timestamp::now();
        let newer_ms = newer.as_millisecond();
        let newer_ns = nautilus_core::datetime::try_datetime_to_unix_nanos(newer).unwrap();
        peer.orders.lock().unwrap()["orders"][0]["fill_count_fp"] = "0.25".into();
        peer.orders.lock().unwrap()["orders"][0]["remaining_count_fp"] = "1.75".into();
        peer.orders.lock().unwrap()["orders"][0]["last_update_time"] = newer.to_string().into();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["fill_id"] = "fill-c".into();
        fill["order_id"] = "order-a".into();
        fill["ticker"] = instrument.id.symbol.as_str().into();
        fill["market_ticker"] = instrument.id.symbol.as_str().into();
        fill["book_side"] = "bid".into();
        fill["outcome_side"] = "yes".into();
        fill["count_fp"] = "0.25".into();
        fill["yes_price_dollars"] = "0.6001".into();
        fill["no_price_dollars"] = "0.3999".into();
        fill["created_time"] = newer.to_string().into();
        fill["fee_cost"] = "0.000075".into();
        *peer.fills.lock().unwrap() = json!({"fills": [fill], "cursor": ""});
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = instrument.id.symbol.as_str().into();
        position["position_fp"] = "0.25".into();
        position["last_updated_ts"] = newer.to_string().into();
        *peer.positions.lock().unwrap() =
            json!({"market_positions": [position], "event_positions": [], "cursor": ""});
        peer.balance.lock().unwrap()["balance_dollars"] = "2.8500".into();
        let mut notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_newer_fill"]
                .clone();
        notification["msg"]["last_updated_ts_ms"] = serde_json::json!(newer_ms);
        *peer.notification.lock().unwrap() = Some(notification);

        peer.notification_ready.notify_one();
        let mut queued = Vec::new();

        loop {
            match output.recv().await.unwrap() {
                ExecutionEvent::Account(state) => {
                    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state));
                    let events = std::mem::take(&mut queued);
                    replay_bootstrap(&events);

                    if harness.cache().borrow().order(&id).unwrap().ts_last() == newer_ns {
                        break;
                    }
                }
                event => queued.push(event),
            }
        }

        peer.changes.ack_gate.add_permits(1);

        let rejected = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
                AsyncRunner::handle_exec_event(ExecutionEvent::Order(
                    OrderEventAny::CancelRejected(rejected),
                ));
                break rejected;
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert_eq!(rejected.client_order_id, id);
        assert_eq!(rejected.strategy_id, claimant);
        assert_eq!(rejected.causation_id, Some(command_id));
        assert_eq!(rejected.venue_order_id, cached.venue_order_id());
        assert_eq!(
            rejected.ts_event,
            nautilus_core::datetime::try_datetime_to_unix_nanos(newer).unwrap()
        );
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().ts_last(),
            rejected.ts_event
        );
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().status(),
            OrderStatus::PartiallyFilled
        );
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
async fn claimed_resting_order_cancels_with_claimant(cancel_all: bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        let instrument = bootstrap_instrument();
        resting_bootstrap(&peer, &instrument);
        let (harness, mut client, mut output, _data) = bootstrap_consumer(&peer, &instrument, true);
        let claimant = bootstrap_ownership(&harness, &instrument, false, true);
        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        let id = ClientOrderId::from("order-a");
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(cached.strategy_id(), claimant);
        assert_eq!(cached.status(), OrderStatus::Accepted);
        assert_eq!(cached.venue_order_id(), Some(VenueOrderId::from("order-a")));

        if cancel_all {
            peer.batches.ack_gate.add_permits(1);

            let cancel = CancelAllOrders::new(
                cached.trader_id(),
                Some(harness.client_id()),
                claimant,
                instrument.id,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            );

            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .cancel_all_orders(cancel)
                .unwrap();
        } else {
            peer.changes.ack_gate.add_permits(1);

            let cancel = CancelOrder::new(
                cached.trader_id(),
                Some(harness.client_id()),
                claimant,
                instrument.id,
                id,
                cached.venue_order_id(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            );

            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .cancel_order(cancel)
                .unwrap();
        }

        loop {
            let connected = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();

            if harness.cache().borrow().order(&id).unwrap().status() == OrderStatus::Canceled
                && connected
            {
                break;
            }

            let event = output.recv().await.unwrap();
            AsyncRunner::handle_exec_event(event);
        }

        if cancel_all {
            assert_eq!(peer.batches.writes.lock().unwrap().len(), 1);
        } else {
            assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        }

        assert!(peer.orders.lock().unwrap()["orders"][0]["status"] == "canceled");
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness drives one client and cache consumer at a time"
)]
#[tokio::test]
async fn startup_retries_cached_position_mismatch() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut peer = Peer::with_timeout(None, 10).await;
        peer.config.ws_timeout_secs = 10;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        bootstrap_opening_position(&harness, &instrument, OrderSide::Buy, "OPEN-001".into());
        filled_bootstrap(&peer, &instrument, false, [Decimal::ZERO; 2]);
        peer.positions.lock().unwrap()["market_positions"][0]["position_fp"] = "0.25".into();
        let positions = Arc::clone(&peer.positions);
        let position_read = Arc::clone(&peer.position_read);
        let update = tokio::spawn(async move {
            position_read.notified().await;
            positions.lock().unwrap()["market_positions"][0]["position_fp"] = "1.00".into();
        });

        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        replay_bootstrap(&queued);
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        update.await.unwrap();
        let mut engine = harness.exec_engine().borrow_mut();
        let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[tokio::test]
async fn startup_rejects_cold_open_position_without_readiness_timeout(
    #[values("0.75", "-0.75")] net: &str,
    #[values(false, true)] cached_order: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = instrument.id.symbol.as_str().into();
        position["position_fp"] = net.into();
        *peer.positions.lock().unwrap() =
            json!({"market_positions": [position], "event_positions": [], "cursor": ""});
        let (harness, mut client, _output, _data) = bootstrap_consumer(&peer, &instrument, false);
        if cached_order {
            bootstrap_ownership(&harness, &instrument, true, false);
            let mut source = fixtures["order"]["order"].clone();
            source["ticker"] = instrument.id.symbol.as_str().into();
            source["client_order_id"] = "order-a".into();
            source["book_side"] = "bid".into();
            source["outcome_side"] = "yes".into();
            source["status"] = "resting".into();
            source["fill_count_fp"] = "0.00".into();
            source["remaining_count_fp"] = "0.75".into();
            source["initial_count_fp"] = "0.75".into();
            source["yes_price_dollars"] = "0.6004".into();
            source["no_price_dollars"] = "0.3996".into();
            *peer.order_lookup.lock().unwrap() = Some(source.clone());
            *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
        }

        let error = tokio::time::timeout(Duration::from_secs(2), client.connect())
            .await
            .expect("cold position recovery waited for the connection timeout")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "KALSHI_OPENING_CONTEXT_REQUIRED: position cycle lacks cached opening authority; restore complete cached orders and positions"
        );
        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[rstest]
#[case::long(false)]
#[case::short(true)]
#[tokio::test]
async fn startup_rejects_unrecoverable_filled_order_without_readiness_timeout(#[case] short: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (_harness, mut client, _output, _data) = bootstrap_consumer(&peer, &instrument, false);
        filled_bootstrap(&peer, &instrument, short, [Decimal::ZERO; 2]);
        peer.orders.lock().unwrap()["orders"][0]["status"] = "canceled".into();

        let error = tokio::time::timeout(Duration::from_secs(2), client.connect())
            .await
            .expect("missing order authority waited for the connection timeout")
            .unwrap_err();
        assert_eq!(error.to_string(), "KALSHI_ORDER_CONTEXT_REQUIRED: filled quantity lacks command metadata; restore persisted orders");
        assert!(!client.is_connected());
    }).await.unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn startup_filtering_rejects_unclaimed_exposure(#[case] retained: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) = bootstrap_consumer(&peer, &instrument, true);
        let opening = retained.then(|| {
            bootstrap_opening_position(&harness, &instrument, OrderSide::Buy, "OPEN-001".into())
        });

        filled_bootstrap(&peer, &instrument, false, [Decimal::ZERO; 2]);
        if retained {
            peer.positions.lock().unwrap()["market_positions"][0]["position_fp"] = "1.00".into();
        }

        let queued = connect_consuming_accounts(&mut client, &mut output).await;
        assert_eq!(
            queued
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, _))
                ))
                .count(),
            2
        );
        replay_bootstrap(&queued);
        assert!(client.is_connected());
        let result = client.generate_mass_status(Some(60)).await;
        client.disconnect().await.unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "KALSHI_ORDER_CONTEXT_REQUIRED: filled quantity lacks command metadata; restore persisted orders"
        );
        let cache = harness.cache().borrow();
        assert!(cache.orders(None, None, None, None, None).is_empty());
        assert_eq!(
            cache.positions(None, None, None, None, None).len(),
            usize::from(retained)
        );

        if let Some(opening) = opening {
            let current = cache.position_owned(&opening.id).unwrap();
            assert_eq!(
                serde_json::to_value(current).unwrap(),
                serde_json::to_value(opening).unwrap()
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn startup_mass_before_recovery_replay_fails_closed() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        filled_bootstrap(&peer, &instrument, false, [Decimal::ZERO; 2]);
        let _queued = connect_consuming_accounts(&mut client, &mut output).await;
        assert!(client.is_connected());
        let error = client.generate_mass_status(Some(60)).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "KALSHI_ORDER_CONTEXT_REQUIRED: filled quantity lacks command metadata; restore persisted orders"
        );
        assert!(
            harness
                .cache()
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        assert!(
            harness
                .cache()
                .borrow()
                .positions(None, None, None, None, None)
                .is_empty()
        );
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded harness queues order reports while client awaits route only account events"
)]
async fn reconcile_bootstrap(
    short: bool,
    owned: bool,
    claimed: bool,
    retained: bool,
    fees: [Decimal; 2],
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let peer = Peer::new(None).await;
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, claimed || owned);
        let opening = retained.then(|| {
            bootstrap_opening_position(&harness, &instrument, OrderSide::Buy, "OPEN-001".into())
        });

        filled_bootstrap(&peer, &instrument, short, fees);
        if retained {
            peer.positions.lock().unwrap()["market_positions"][0]["position_fp"] = "1.00".into();
        }

        let strategy = bootstrap_ownership(&harness, &instrument, owned, claimed);
        let mut queued = Vec::new();
        let mut previous_session = None;

        let config = ExecutionManagerConfig {
            trader_id: harness.trader_id(),
            filter_unclaimed_external: claimed || owned,
            ..Default::default()
        };

        let mut manager =
            ExecutionManager::new(harness.clock().clone(), harness.cache().clone(), config)
                .unwrap();

        client.start().unwrap();
        harness.register_client(Box::new(client)).unwrap();

        for generation in 0..2 {
            {
                let mut engine = harness.exec_engine().borrow_mut();
                let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
                queued.extend(connect_consuming_accounts(client.as_mut(), &mut output).await);
            }

            let engine = harness.exec_engine().borrow();
            let client = engine.get_client(&harness.client_id()).unwrap();
            assert!(client.is_connected());
            let state = client.get_account().unwrap().last_event().unwrap();
            let session = state.info.as_ref().unwrap()["kalshi_session"].clone();
            assert_ne!(previous_session.as_ref(), Some(&session));
            previous_session = Some(session);

            if generation == 0 {
                let cache = harness.cache().borrow();
                assert_eq!(
                    cache.orders(None, None, None, None, None).len(),
                    usize::from(owned)
                );
                assert_eq!(
                    cache.positions(None, None, None, None, None).len(),
                    usize::from(retained)
                );
            }

            drop(engine);

            replay_bootstrap(&queued);

            let engine = harness.exec_engine().borrow();
            let client = engine.get_client(&harness.client_id()).unwrap();
            let mass = client
                .generate_mass_status(Some(60))
                .await
                .unwrap()
                .unwrap();
            assert!(mass.reports_complete());
            assert_eq!(mass.order_reports().len(), 1);
            assert_eq!(mass.fill_reports()[&VenueOrderId::from("order-a")].len(), 2);
            assert_eq!(mass.position_reports()[&instrument.id].len(), 1);

            drop(engine);
            let result = manager.reconcile_execution_mass_status(&mass, harness.exec_engine());
            assert!(
                result.unresolved_positions.is_empty(),
                "{:?}",
                result.unresolved_positions
            );
            assert!(result.events.is_empty());
            assert!(result.external_orders.is_empty());

            queued.retain(|event| {
                matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Fill(_))
                        | ExecutionEvent::Order(OrderEventAny::Filled(_))
                )
            });

            assert_eq!(queued.len(), if generation == 0 { 2 } else { 0 });
            replay_bootstrap(&queued);
            queued.clear();

            let replay = manager.reconcile_execution_mass_status(&mass, harness.exec_engine());
            assert!(replay.events.is_empty());
            assert!(replay.external_orders.is_empty());
            assert!(replay.unresolved_positions.is_empty());
            assert!(
                harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            );
            assert_bootstrap_economics(&harness, strategy, short, owned, retained, fees);
            if let Some(opening) = &opening {
                let current = harness
                    .cache()
                    .borrow()
                    .position_owned(&opening.id)
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(current).unwrap(),
                    serde_json::to_value(opening).unwrap()
                );
            }

            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            client.disconnect().await.unwrap();
            assert!(!client.is_connected());
        }

        let core = ExecutionClientCore::new(
            harness.trader_id(),
            harness.client_id(),
            *KALSHI_VENUE,
            OmsType::Netting,
            harness.account_id(),
            AccountType::Cash,
            Some(Currency::USD()),
            harness.cache().clone(),
        );

        let mut restarted = KalshiExecutionClient::new(core, peer.config.clone()).unwrap();
        let queued = connect_consuming_accounts(&mut restarted, &mut output).await;
        assert!(
            queued
                .iter()
                .all(|event| !matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))))
        );
        replay_bootstrap(&queued);
        assert_bootstrap_economics(&harness, strategy, short, owned, retained, fees);
        assert!(restarted.is_connected());
        restarted.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

fn bootstrap_ownership(
    harness: &ExecutionHarness,
    instrument: &BinaryOption,
    owned: bool,
    claimed: bool,
) -> StrategyId {
    let owner = StrategyId::from("OWNER-001");
    let claimant = StrategyId::from("CLAIM-002");
    let id = ClientOrderId::from("order-a");

    if claimed || owned {
        harness
            .cache()
            .borrow_mut()
            .register_external_order_claims(claimant, &[instrument.id])
            .unwrap();
    }

    if owned {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(harness.trader_id())
            .strategy_id(owner)
            .instrument_id(instrument.id)
            .client_order_id(id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.75"))
            .price(Price::from("0.600400"))
            .time_in_force(TimeInForce::Gtc)
            .post_only(true)
            .build();
        harness
            .cache()
            .borrow_mut()
            .add_order(order, None, Some(harness.client_id()), false)
            .unwrap();
        let accepted = OrderEventAny::Accepted(OrderAccepted::new(
            harness.trader_id(),
            owner,
            instrument.id,
            id,
            VenueOrderId::from("order-a"),
            harness.account_id(),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
        ));
        harness
            .cache()
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();
    }

    if owned {
        owner
    } else if claimed {
        claimant
    } else {
        StrategyId::external()
    }
}

async fn connect_consuming_accounts(
    client: &mut dyn ExecutionClient,
    output: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut queued = Vec::new();
    let connect = client.connect();
    tokio::pin!(connect);

    loop {
        tokio::select! {
            biased;
            result = &mut connect => { result.unwrap(); break; }
            event = output.recv() => match event.unwrap() {
                ExecutionEvent::Account(state) => {
                    std::future::poll_fn(|cx| {
                        assert!(connect.as_mut().poll(cx).is_pending());
                        Poll::Ready(())
                    }).await;
                    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state));
                },
                event => queued.push(event),
            }
        }
    }

    queued
}

fn replay_bootstrap(events: &[ExecutionEvent]) {
    let (reports, orders): (Vec<_>, Vec<_>) = events
        .iter()
        .partition(|event| matches!(event, ExecutionEvent::Report(_)));

    for event in reports.into_iter().chain(orders) {
        let event = match event {
            ExecutionEvent::Report(report) => ExecutionEvent::Report(report.clone()),
            ExecutionEvent::Order(order) => ExecutionEvent::Order(order.clone()),
            event => panic!("unexpected bootstrap event {event:?}"),
        };

        AsyncRunner::handle_exec_event(event);
    }
}

fn bootstrap_opening_position(
    harness: &ExecutionHarness,
    instrument: &BinaryOption,
    side: OrderSide,
    position_id: PositionId,
) -> Position {
    let fill = OrderFilled::new(
        harness.trader_id(),
        StrategyId::from("OWNER-001"),
        instrument.id,
        ClientOrderId::from("opening-order"),
        VenueOrderId::from("opening-venue"),
        harness.account_id(),
        TradeId::from("opening-fill"),
        side,
        OrderType::Limit,
        Quantity::from("0.25"),
        Price::from("0.420000"),
        Currency::USD(),
        LiquiditySide::Maker,
        UUID4::new(),
        UnixNanos::from(1),
        UnixNanos::from(1),
        false,
        Some(position_id),
        Some(Money::from_decimal_exact(Decimal::new(7, 6), Currency::USD()).unwrap()),
        None,
    );

    let instrument = InstrumentAny::BinaryOption(instrument.clone());
    let position = Position::new(&instrument, fill);
    harness
        .cache()
        .borrow_mut()
        .add_position_without_order(&position, OmsType::Netting)
        .unwrap();
    position
}

fn bootstrap_instrument() -> BinaryOption {
    let definitions: Value =
        serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
    let mut market: Market =
        serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
    let mut event: Event =
        serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
    market.exchange_index = Field::Value(3);
    event.exchange_index = Field::Value(3);
    parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
}

fn resting_bootstrap(peer: &Peer, instrument: &BinaryOption) {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let mut source = fixtures["order"]["order"].clone();
    source["ticker"] = instrument.id.symbol.as_str().into();
    source["client_order_id"] = "".into();
    source["exchange_index"] = 3.into();
    source["book_side"] = "bid".into();
    source["outcome_side"] = "yes".into();
    source["status"] = "resting".into();
    source["fill_count_fp"] = "0.00".into();
    source["remaining_count_fp"] = "2.00".into();
    source["initial_count_fp"] = "2.00".into();
    source["yes_price_dollars"] = "0.6001".into();
    source["no_price_dollars"] = "0.3999".into();
    *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
}

fn filled_bootstrap(peer: &Peer, instrument: &BinaryOption, short: bool, fees: [Decimal; 2]) {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let now = jiff::Timestamp::now();
    let earlier = now.to_string();
    let later = now
        .checked_add(jiff::SignedDuration::from_nanos(1))
        .unwrap()
        .to_string();
    let mut source = fixtures["order"]["order"].clone();
    source["ticker"] = instrument.id.symbol.as_str().into();
    source["client_order_id"] = "".into();
    source["book_side"] = if short { "ask" } else { "bid" }.into();
    source["outcome_side"] = if short { "no" } else { "yes" }.into();
    source["status"] = "executed".into();
    source["fill_count_fp"] = "0.75".into();
    source["remaining_count_fp"] = "0.00".into();
    source["initial_count_fp"] = "0.75".into();
    source["yes_price_dollars"] = "0.6004".into();
    source["no_price_dollars"] = "0.3996".into();
    source["created_time"] = earlier.clone().into();
    source["last_update_time"] = later.clone().into();
    let mut fills = Vec::new();

    for (id, qty, yes, no, fee, time) in [
        ("fill-a", "0.25", "0.6001", "0.3999", fees[0], earlier),
        ("fill-b", "0.50", "0.6004", "0.3996", fees[1], later.clone()),
    ] {
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["fill_id"] = id.into();
        fill["ticker"] = instrument.id.symbol.as_str().into();
        fill["market_ticker"] = instrument.id.symbol.as_str().into();
        fill["book_side"] = source["book_side"].clone();
        fill["outcome_side"] = source["outcome_side"].clone();
        fill["count_fp"] = qty.into();
        fill["yes_price_dollars"] = yes.into();
        fill["no_price_dollars"] = no.into();
        fill["fee_cost"] = fee.to_string().into();
        fill["created_time"] = time.into();
        fills.push(fill);
    }

    let mut position = fixtures["positions"]["market_positions"][0].clone();
    position["ticker"] = instrument.id.symbol.as_str().into();
    position["position_fp"] = if short { "-0.75" } else { "0.75" }.into();
    position["last_updated_ts"] = later.into();
    *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});
    *peer.fills.lock().unwrap() = json!({"fills": fills, "cursor": ""});
    *peer.positions.lock().unwrap() =
        json!({"market_positions": [position], "event_positions": [], "cursor": ""});
}

fn bootstrap_consumer(
    peer: &Peer,
    instrument: &BinaryOption,
    filter_unclaimed: bool,
) -> (
    ExecutionHarness,
    KalshiExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let harness = ExecutionHarness::new(
        "TEST-001".into(),
        "KALSHI".into(),
        peer.config.account_id,
        instrument.clone().into(),
    );

    let mut config = harness.exec_engine().borrow().config().clone();
    config.filter_unclaimed_external_orders = filter_unclaimed;

    *harness.exec_engine().borrow_mut() = ExecutionEngine::new(
        harness.clock().clone(),
        harness.cache().clone(),
        Some(config),
    );

    let (sender, output) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(sender);
    let (sender, data) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(sender);

    let core = ExecutionClientCore::new(
        harness.trader_id(),
        harness.client_id(),
        *KALSHI_VENUE,
        OmsType::Netting,
        peer.config.account_id,
        AccountType::Cash,
        Some(Currency::USD()),
        harness.cache().clone(),
    );

    let client = KalshiExecutionClient::new(core, peer.config.clone()).unwrap();
    (harness, client, output, data)
}

fn assert_bootstrap_economics(
    harness: &ExecutionHarness,
    strategy: StrategyId,
    short: bool,
    owned: bool,
    retained: bool,
    fees: [Decimal; 2],
) {
    let cache = harness.cache().borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.client_order_id(), ClientOrderId::from("order-a"));
    assert_eq!(order.venue_order_id(), Some(VenueOrderId::from("order-a")));
    assert_eq!(order.strategy_id(), strategy);
    assert_eq!(
        cache.client_id(&order.client_order_id()),
        Some(&harness.client_id())
    );
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(
        order.order_side(),
        if short {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    );
    assert_eq!(order.quantity(), Quantity::from("0.75"));
    assert_eq!(order.filled_qty(), Quantity::from("0.75"));
    assert_eq!(order.avg_px(), Some(Decimal::new(6003, 4)));
    assert_eq!(order.is_post_only(), owned);
    assert!(!order.is_reduce_only());
    assert_eq!(order.time_in_force(), TimeInForce::Gtc);
    assert_eq!(
        order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
        vec![TradeId::from("fill-a"), TradeId::from("fill-b")]
    );
    let events = order.events();

    let fills: Vec<_> = events
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 2);
    assert_eq!(fills[1].ts_event.as_u64(), fills[0].ts_event.as_u64() + 1);

    for (fill, id, qty, price, fee) in [
        (fills[0], "fill-a", "0.25", "0.600100", fees[0]),
        (fills[1], "fill-b", "0.50", "0.600400", fees[1]),
    ] {
        assert_eq!(fill.trade_id, TradeId::from(id));
        assert_eq!(fill.client_order_id, order.client_order_id());
        assert_eq!(fill.venue_order_id, VenueOrderId::from("order-a"));
        assert_eq!(fill.account_id, harness.account_id());
        assert_eq!(fill.instrument_id, harness.instrument_id());
        assert_eq!(fill.strategy_id, strategy);
        assert_eq!(fill.order_side, order.order_side());
        assert_eq!(fill.last_qty, Quantity::from(qty));
        assert_eq!(fill.last_px, Price::from(price));
        assert_eq!(
            fill.commission,
            Some(Money::from_decimal_exact(fee, Currency::USD()).unwrap())
        );
    }

    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(positions.len(), 1 + usize::from(retained));
    let position = positions
        .iter()
        .find(|position| position.strategy_id == strategy)
        .unwrap();
    assert_eq!(position.strategy_id, strategy);
    assert_eq!(
        position.side,
        if short {
            PositionSide::Short
        } else {
            PositionSide::Long
        }
    );
    assert_eq!(
        position.signed_decimal_qty(),
        Decimal::new(if short { -75 } else { 75 }, 2)
    );
    assert_eq!(position.quantity, Quantity::from("0.75"));
    assert_eq!(position.avg_px_open, Decimal::new(6003, 4));
    assert_eq!(position.cost_open, Decimal::new(450225, 6));
    assert_eq!(
        position.commissions(),
        vec![Money::from_decimal_exact(fees[0] + fees[1], Currency::USD()).unwrap()]
    );
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&TradeId::from("fill-a")));
    assert!(position.trade_ids.contains(&TradeId::from("fill-b")));
    let account = cache.account(&harness.account_id()).unwrap();
    let balance = account.last_event().unwrap().balances[0];
    assert_eq!(balance.total.as_decimal(), Decimal::new(10001, 4));
    assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
    assert_eq!(balance.free.as_decimal(), Decimal::new(10001, 4));
    assert_eq!(balance.currency, Currency::USD());
}

struct SubmissionPeer {
    writes: Mutex<Vec<Value>>,
    written: tokio::sync::Notify,
    outcome: Mutex<String>,
    ack_gate: Arc<tokio::sync::Semaphore>,
}

impl SubmissionPeer {
    async fn respond(
        &self,
        mut stream: tokio::net::TcpStream,
        headers: &str,
        fixtures: &Value,
        orders: &Mutex<Value>,
    ) {
        assert!(headers.starts_with("POST "));

        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|value| value.trim().parse().unwrap())
            })
            .unwrap();

        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        let mode = self.outcome.lock().unwrap().clone();

        let count = {
            let mut writes = self.writes.lock().unwrap();
            writes.push(request.clone());
            writes.len()
        };

        let id = format!("native-{count}");
        let ack = Self::create(&request, fixtures, orders, &mode, &id);

        self.written.notify_one();

        let (status, body) = match mode.as_str() {
            "unknown" => (503, fixtures["error"].to_string()),
            "post_only" => (400, fixtures["reject_post_only"].to_string()),
            "reject" => (400, fixtures["reject_balance"].to_string()),
            "duplicate" => (409, fixtures["reject_duplicate"].to_string()),
            _ => (201, ack.to_string()),
        };

        let gate = Arc::clone(&self.ack_gate);
        tokio::spawn(async move {
            let permit = gate.acquire().await.unwrap();
            permit.forget();
            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }

    fn create(
        request: &Value,
        fixtures: &Value,
        orders: &Mutex<Value>,
        mode: &str,
        id: &str,
    ) -> Value {
        let now = jiff::Timestamp::now();
        let mut ack = fixtures["create"].clone();
        ack["order_id"] = id.into();
        ack["client_order_id"] = request["client_order_id"].clone();
        ack["fill_count"] = "0.00".into();
        ack["remaining_count"] = request["count"].clone();
        ack["ts_ms"] = now.as_millisecond().into();
        if mode == "identity" {
            ack["client_order_id"] = "some-other-order".into();
        }

        if matches!(mode, "resting" | "fill-first" | "immediate" | "post_only") {
            let mut order = fixtures["order"]["order"].clone();
            order["order_id"] = id.into();
            order["client_order_id"] = request["client_order_id"].clone();
            order["ticker"] = request["ticker"].clone();
            order["book_side"] = request["side"].clone();
            order["outcome_side"] = if request["side"] == "bid" {
                "yes"
            } else {
                "no"
            }
            .into();

            order["fill_count_fp"] = "0.00".into();
            order["remaining_count_fp"] = request["count"].clone();
            order["initial_count_fp"] = request["count"].clone();
            order["time_in_force"] = request["time_in_force"].clone();
            order["expiration_time"] =
                request
                    .get("expiration_time")
                    .map_or(Value::Null, |seconds| {
                        jiff::Timestamp::from_second(seconds.as_i64().unwrap())
                            .unwrap()
                            .to_string()
                            .into()
                    });

            if matches!(mode, "immediate" | "post_only") {
                order["status"] = "canceled".into();
                order["remaining_count_fp"] = "0.00".into();
                ack["remaining_count"] = "0.00".into();
            }

            order["yes_price_dollars"] = request["price"].clone();
            let price: Decimal = request["price"].as_str().unwrap().parse().unwrap();
            order["no_price_dollars"] = (Decimal::ONE - price).to_string().into();
            order["created_time"] = now.to_string().into();
            order["last_update_time"] = now.to_string().into();
            for key in [
                "taker_fees_dollars",
                "maker_fees_dollars",
                "taker_fill_cost_dollars",
                "maker_fill_cost_dollars",
            ] {
                order[key] = "0.000000".into();
            }

            orders.lock().unwrap()["orders"]
                .as_array_mut()
                .unwrap()
                .push(order);
        }

        ack
    }
}

async fn native_consumer(
    peer: &Peer,
) -> (
    ExecutionHarness,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    peer.market.lock().unwrap()["open_time"] = "2020-01-01T00:00:00Z".into();
    peer.market.lock().unwrap()["close_time"] = "2100-01-01T00:00:00Z".into();
    let instrument = bootstrap_instrument();
    let (harness, mut client, mut output, _data) = bootstrap_consumer(peer, &instrument, false);
    client.start().unwrap();
    let queued = connect_consuming_accounts(&mut client, &mut output).await;
    replay_bootstrap(&queued);
    harness.register_client(Box::new(client)).unwrap();
    (harness, output, _data)
}

fn native_order(
    harness: &ExecutionHarness,
    id: &str,
    side: OrderSide,
    post_only: bool,
) -> OrderAny {
    native_order_builder(harness, id, side)
        .post_only(post_only)
        .time_in_force(TimeInForce::Gtc)
        .build()
}

fn native_order_builder(harness: &ExecutionHarness, id: &str, side: OrderSide) -> OrderTestBuilder {
    let mut builder = OrderTestBuilder::new(OrderType::Limit);
    builder
        .trader_id(harness.trader_id())
        .strategy_id(StrategyId::from("NATIVE-001"))
        .instrument_id(bootstrap_instrument().id)
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(Quantity::from("1.25"))
        .price(Price::from("0.600000"));
    builder
}

fn native_submit(
    harness: &ExecutionHarness,
    order: &OrderAny,
    params: Option<nautilus_core::Params>,
) {
    harness
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(harness.client_id()), false)
        .unwrap();
    let mut cmd = SubmitOrder::from_order(
        order,
        harness.trader_id(),
        Some(harness.client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );
    cmd.params = params;
    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .submit_order(cmd)
        .unwrap();
}

async fn native_wait(
    output: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    done: impl Fn() -> bool,
) -> Vec<OrderEventAny> {
    let mut events = Vec::new();

    while !done() {
        let event = output.recv().await.unwrap();
        if let ExecutionEvent::Order(order) = &event {
            events.push(order.clone());
        }

        AsyncRunner::handle_exec_event(event);
    }

    events
}

fn native_dispatch(events: impl IntoIterator<Item = ExecutionEvent>) -> Vec<OrderEventAny> {
    let mut orders = Vec::new();

    for event in events {
        if let ExecutionEvent::Order(order) = &event {
            orders.push(order.clone());
        }

        AsyncRunner::handle_exec_event(event);
    }

    orders
}

fn native_status(harness: &ExecutionHarness, id: ClientOrderId) -> OrderStatus {
    harness.cache().borrow().order(&id).unwrap().status()
}

fn native_balance(harness: &ExecutionHarness) -> AccountBalance {
    let cache = harness.cache().borrow();
    let account = cache.account(&harness.account_id()).unwrap();
    account.last_event().unwrap().balances[0]
}

#[rstest]
#[case(OrderSide::Buy, "bid", "0.784500", "0.215600")]
#[case(OrderSide::Sell, "ask", "0.534500", "0.465600")]
#[tokio::test]
async fn native_gtc_reserves_exact_funds_and_reconnects_without_replay(
    #[case] side: OrderSide,
    #[case] wire_side: &str,
    #[case] locked: &str,
    #[case] free: &str,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-gtc", side, false);

        let params = nautilus_core::Params::new(
            [
                ("self_trade_prevention_type".into(), "maker".into()),
                ("cancel_order_on_pause".into(), true.into()),
            ]
            .into(),
        );

        native_submit(&harness, &order, Some(params));
        let id = order.client_order_id();
        let mut events = native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Submitted
        })
        .await;

        let held = native_balance(&harness);
        assert_eq!(held.total.as_decimal(), dec("1.0001"));
        assert_eq!(held.locked.as_decimal(), dec(locked));
        assert_eq!(held.free.as_decimal(), dec(free));
        let duplicate = SubmitOrder::from_order(
            &order,
            harness.trader_id(),
            Some(harness.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .submit_order(duplicate)
                .is_err()
        );
        peer.submissions.ack_gate.add_permits(1);
        events.extend(
            native_wait(&mut output, || {
                native_status(&harness, id) == OrderStatus::Accepted
                    && harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
            })
            .await,
        );

        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, OrderEventAny::Submitted(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, OrderEventAny::Accepted(_)))
                .count(),
            1
        );
        assert_eq!(events.len(), 2);
        let writes = peer.submissions.writes.lock().unwrap().clone();
        let mut expected =
            serde_json::from_str::<Value>(include_str!("../../test_data/rest_requests.json"))
                .unwrap()["native_gtc"]
                .clone();
        expected["ticker"] = serde_json::json!(order.instrument_id().symbol.as_str());
        expected["side"] = serde_json::json!(wire_side);
        assert_eq!(writes, vec![expected]);
        {
            let replay = native_reconnect(&harness, &mut output).await;
            assert!(replay.is_empty());
        }

        assert_eq!(native_balance(&harness), held);
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

fn dec(value: &str) -> Decimal {
    value.parse().unwrap()
}

#[rstest]
#[case("unknown", OrderStatus::Submitted, "0.784500")]
#[case("duplicate", OrderStatus::Submitted, "0.784500")]
#[case("reject", OrderStatus::Rejected, "0")]
#[case("post_only", OrderStatus::Rejected, "0")]
#[case("identity", OrderStatus::Submitted, "0.784500")]
#[tokio::test]
async fn native_gtc_failures_preserve_the_evidence_authority(
    #[case] mode: &str,
    #[case] expected: OrderStatus,
    #[case] locked: &str,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        *peer.submissions.outcome.lock().unwrap() = mode.into();
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(
            &harness,
            "native-outcome",
            OrderSide::Buy,
            mode == "post_only",
        );
        native_submit(&harness, &order, None);
        peer.submissions.ack_gate.add_permits(1);
        let id = order.client_order_id();
        let mut submitted = false;
        let mut events = Vec::new();

        loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(order) = &event {
                submitted |= matches!(order, OrderEventAny::Submitted(_));
                events.push(order.clone());
            }

            let recovered = submitted
                && matches!(&event, ExecutionEvent::Account(_))
                && !peer.submissions.writes.lock().unwrap().is_empty();
            AsyncRunner::handle_exec_event(event);

            if recovered
                && native_status(&harness, id) == expected
                && native_balance(&harness).locked.as_decimal() == dec(locked)
            {
                break;
            }
        }

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Rejected(_)))
                .count(),
            usize::from(mode == "reject" || mode == "post_only")
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, OrderEventAny::Denied(_)))
        );

        for event in &events {
            if let OrderEventAny::Rejected(rejected) = event {
                assert_eq!(rejected.due_post_only, mode == "post_only");
                assert_eq!(
                    rejected.reason.as_str(),
                    if mode == "post_only" {
                        "Post-only order would cross"
                    } else {
                        "Insufficient balance"
                    }
                );
            }
        }

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
        assert_eq!(
            native_balance(&harness).free.as_decimal(),
            dec("1.0001") - dec(locked)
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded harness holds the client borrow only while collecting mass reports"
)]
#[rstest]
#[tokio::test]
async fn native_post_only_rejection_recovers_and_allows_following_order() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        *peer.submissions.outcome.lock().unwrap() = "post_only".into();
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let rejected = native_order(&harness, "post-only-rejected", OrderSide::Buy, true);
        native_submit(&harness, &rejected, None);
        peer.submissions.ack_gate.add_permits(1);

        let mut rejected_events = Vec::new();
        let mut rejected_seen = false;
        let mut accounts_after_rejection = 0;

        loop {
            let event = output.recv().await.unwrap();

            if matches!(&event, ExecutionEvent::Order(OrderEventAny::Rejected(_))) {
                rejected_seen = true;
            }

            if rejected_seen && matches!(&event, ExecutionEvent::Account(_)) {
                accounts_after_rejection += 1;
            }

            let recovered = accounts_after_rejection >= 2;

            if let ExecutionEvent::Order(order) = &event {
                rejected_events.push(order.clone());
            }

            AsyncRunner::handle_exec_event(event);

            if recovered
                && native_status(&harness, rejected.client_order_id()) == OrderStatus::Rejected
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            {
                break;
            }
        }

        assert_eq!(rejected_events.len(), 2);
        assert!(matches!(rejected_events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(rejected_events[1], OrderEventAny::Rejected(_)));
        assert_eq!(
            rejected_events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Rejected(_)))
                .count(),
            1
        );
        assert!(
            !rejected_events
                .iter()
                .any(|event| matches!(event, OrderEventAny::Denied(_)))
        );

        let rejection = rejected_events
            .iter()
            .find_map(|event| match event {
                OrderEventAny::Rejected(event) => Some(event),
                _ => None,
            })
            .unwrap();

        assert!(rejection.due_post_only);
        assert_eq!(rejection.reason.as_str(), "Post-only order would cross");

        let venue_update =
            jiff::Timestamp::from_nanosecond(i128::from(rejection.ts_event.as_u64()) + 1).unwrap();
        peer.orders.lock().unwrap()["orders"][0]["last_update_time"] =
            venue_update.to_string().into();
        assert!(native_reconnect(&harness, &mut output).await.is_empty());

        let mass = {
            let engine = harness.exec_engine().borrow();
            let client = engine.get_client(&harness.client_id()).unwrap();
            client.generate_mass_status(Some(1)).await.unwrap().unwrap()
        };

        let report = &mass.order_reports()[&VenueOrderId::from("native-1")];
        assert!(mass.reports_complete());
        assert_eq!(mass.order_reports().len(), 1);
        assert!(mass.fill_reports().is_empty());
        assert!(mass.position_reports().is_empty());
        assert_eq!(report.client_order_id, Some(rejected.client_order_id()));
        assert_eq!(report.venue_order_id, VenueOrderId::from("native-1"));
        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(report.quantity, Quantity::from("1.25"));
        assert_eq!(report.filled_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(report.price, Some(Price::from("0.600000")));
        assert_eq!(report.ts_last.as_u64(), rejection.ts_event.as_u64() + 1);
        assert!(report.post_only);

        let balance = native_balance(&harness);

        let config = ExecutionManagerConfig {
            trader_id: harness.trader_id(),
            ..Default::default()
        };

        let mut manager =
            ExecutionManager::new(harness.clock().clone(), harness.cache().clone(), config)
                .unwrap();
        let result = manager.reconcile_execution_mass_status(&mass, harness.exec_engine());
        assert!(result.events.is_empty());
        assert!(result.external_orders.is_empty());
        assert!(result.unresolved_positions.is_empty());
        assert_eq!(native_balance(&harness), balance);
        {
            let cache = harness.cache().borrow();
            let cached = cache.order(&rejected.client_order_id()).unwrap();
            assert_eq!(cached.status(), OrderStatus::Rejected);
            assert_eq!(cached.quantity(), Quantity::from("1.25"));
            assert_eq!(cached.filled_qty().as_decimal(), Decimal::ZERO);
            assert!(cache.orders_open(None, None, None, None, None).is_empty());
            assert!(
                cache
                    .positions_open(None, None, None, Some(&harness.account_id()), None)
                    .is_empty()
            );
        }

        let canceled = peer.orders.lock().unwrap()["orders"][0].clone();
        for mode in ["resting", "filled", "identity"] {
            let mut source = canceled.clone();

            match mode {
                "resting" => {
                    source["status"] = "resting".into();
                    source["remaining_count_fp"] = "1.25".into();
                }
                "filled" => {
                    source["status"] = "executed".into();
                    source["fill_count_fp"] = "1.25".into();
                }
                "identity" => source["client_order_id"] = "identity-conflict".into(),
                _ => unreachable!(),
            }

            *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});

            let error = {
                let engine = harness.exec_engine().borrow();
                let client = engine.get_client(&harness.client_id()).unwrap();
                client.generate_mass_status(Some(1)).await.unwrap_err()
            };

            let expected = if mode == "identity" {
                "cached client order identity mismatch"
            } else {
                "venue evidence conflicts with an unsent Kalshi order"
            };

            assert_eq!(error.to_string(), expected);
            assert_eq!(
                native_status(&harness, rejected.client_order_id()),
                OrderStatus::Rejected
            );
            assert_eq!(native_balance(&harness), balance);
        }

        *peer.orders.lock().unwrap() = json!({"orders": [canceled], "cursor": ""});
        assert!(native_reconnect(&harness, &mut output).await.is_empty());

        *peer.submissions.outcome.lock().unwrap() = "resting".into();
        let following = native_order(&harness, "post-only-following", OrderSide::Buy, false);
        native_submit(&harness, &following, None);
        peer.submissions.ack_gate.add_permits(1);

        let following_events = native_wait(&mut output, || {
            native_status(&harness, following.client_order_id()) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(following_events.len(), 2);
        assert!(matches!(following_events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(following_events[1], OrderEventAny::Accepted(_)));
        {
            let writes = peer.submissions.writes.lock().unwrap();
            assert_eq!(writes.len(), 2);
            assert_eq!(writes[0]["post_only"], true);
            assert_eq!(writes[1]["post_only"], false);
        }

        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded harness holds the client borrow only while collecting mass reports"
)]
#[rstest]
#[tokio::test]
async fn native_unsent_rejection_is_not_recovered_by_mass_status() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let instrument = bootstrap_instrument();
        let mut order = native_order(&harness, "unsent-rejection", OrderSide::Buy, false);
        let submitted_at = get_atomic_clock_realtime().get_time_ns();
        let rejected_at = UnixNanos::from(submitted_at.as_u64() + 1);
        order
            .apply(OrderEventAny::Submitted(OrderSubmitted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                harness.account_id(),
                UUID4::new(),
                submitted_at,
                submitted_at,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Rejected(OrderRejected::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                harness.account_id(),
                "Order was not sent".into(),
                UUID4::new(),
                rejected_at,
                rejected_at,
                false,
                false,
            )))
            .unwrap();
        harness
            .cache()
            .borrow_mut()
            .add_order(order, None, Some(harness.client_id()), false)
            .unwrap();
        assert!(native_reconnect(&harness, &mut output).await.is_empty());
        let balance = native_balance(&harness);

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = fixtures["order"]["order"].clone();
        let venue_update = jiff::Timestamp::from_nanosecond(i128::from(rejected_at.as_u64()))
            .unwrap()
            .to_string();
        source["order_id"] = "unsent-venue".into();
        source["client_order_id"] = "unsent-rejection".into();
        source["ticker"] = instrument.id.symbol.as_str().into();
        source["book_side"] = "bid".into();
        source["outcome_side"] = "yes".into();
        source["status"] = "canceled".into();
        source["fill_count_fp"] = "0.00".into();
        source["remaining_count_fp"] = "0.00".into();
        source["initial_count_fp"] = "1.25".into();
        source["yes_price_dollars"] = "0.6000".into();
        source["no_price_dollars"] = "0.4000".into();
        source["created_time"] = venue_update.clone().into();
        source["last_update_time"] = venue_update.into();
        *peer.orders.lock().unwrap() = json!({"orders": [source], "cursor": ""});

        let error = {
            let engine = harness.exec_engine().borrow();
            let client = engine.get_client(&harness.client_id()).unwrap();
            client.generate_mass_status(Some(1)).await.unwrap_err()
        };

        assert_eq!(
            error.to_string(),
            "venue evidence conflicts with an unsent Kalshi order"
        );
        assert_eq!(native_balance(&harness), balance);
        {
            let cache = harness.cache().borrow();
            let cached = cache
                .order(&ClientOrderId::from("unsent-rejection"))
                .unwrap();
            assert_eq!(cached.status(), OrderStatus::Rejected);
            assert_eq!(cached.quantity(), Quantity::from("1.25"));
            assert_eq!(cached.filled_qty().as_decimal(), Decimal::ZERO);
            assert!(cached.events().iter().any(|event| matches!(
                event,
                OrderEventAny::Rejected(event)
                    if event.reason.as_str() == "Order was not sent" && !event.due_post_only
            )));
            assert!(cache.orders_open(None, None, None, None, None).is_empty());
            assert!(
                cache
                    .positions_open(None, None, None, Some(&harness.account_id()), None)
                    .is_empty()
            );
        }

        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_private_notice_fences_submissions_until_balance_consumed() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.000100".into();
        peer.submissions.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;

        let execute = |order: &OrderAny| {
            let command = SubmitOrder::from_order(
                order,
                harness.trader_id(),
                Some(harness.client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
            );
            harness
                .exec_engine()
                .borrow()
                .execute(TradingCommand::SubmitOrder(command));
        };

        let first = native_order(&harness, "notice-first", OrderSide::Buy, false);
        execute(&first);
        let events = native_wait(&mut output, || {
            native_status(&harness, first.client_order_id()) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));
        let session = harness
            .cache()
            .borrow()
            .account(&harness.account_id())
            .unwrap()
            .last_event()
            .unwrap()
            .info;
        let notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_native"]
                .clone();
        *peer.notification.lock().unwrap() = Some(notification);
        peer.notification_ready.notify_one();

        loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Account(account) = &event
                && account.info != session
            {
                assert!(
                    !harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
                );

                let blocked = native_order(&harness, "notice-blocked", OrderSide::Buy, false);
                execute(&blocked);
                let cached = harness
                    .cache()
                    .borrow()
                    .order_owned(&blocked.client_order_id())
                    .unwrap();

                let OrderEventAny::Denied(denied) = cached.last_event() else {
                    panic!("expected readiness denial");
                };

                assert_eq!(
                    denied.reason.as_str(),
                    "SUBMIT_FAILED: Kalshi submission requires current-session readiness"
                );

                assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
                assert_eq!(
                    native_balance(&harness).locked.as_decimal(),
                    dec("0.784500")
                );
                AsyncRunner::handle_exec_event(event);
                break;
            }

            AsyncRunner::handle_exec_event(event);
        }

        native_wait(&mut output, || {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let second = native_order(&harness, "notice-second", OrderSide::Sell, false);
        execute(&second);
        let events = native_wait(&mut output, || {
            native_status(&harness, second.client_order_id()) == OrderStatus::Accepted
        })
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));
        let balance = native_balance(&harness);
        assert_eq!(balance.total.as_decimal(), dec("3.000100"));
        assert_eq!(balance.locked.as_decimal(), dec("1.319000"));
        assert_eq!(balance.free.as_decimal(), dec("1.681100"));
        assert_eq!(
            native_status(&harness, first.client_order_id()),
            OrderStatus::Accepted
        );
        let writes = peer.submissions.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0]["client_order_id"], "notice-first");
        assert_eq!(writes[0]["side"], "bid");
        assert_eq!(writes[1]["client_order_id"], "notice-second");
        assert_eq!(writes[1]["side"], "ask");
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_gtc_competing_orders_cannot_reuse_cash() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let a = native_order(&harness, "competing-a", OrderSide::Buy, false);
        let b = native_order(&harness, "competing-b", OrderSide::Buy, false);
        native_submit(&harness, &a, None);
        native_submit(&harness, &b, None);

        let events = native_wait(&mut output, || {
            let statuses = [
                native_status(&harness, a.client_order_id()),
                native_status(&harness, b.client_order_id()),
            ];

            statuses.contains(&OrderStatus::Submitted) && statuses.contains(&OrderStatus::Denied)
        })
        .await;

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Denied(_)))
                .count(),
            1
        );
        assert_eq!(
            native_balance(&harness).locked.as_decimal(),
            dec("0.784500")
        );
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("0.215600"));
        peer.submissions.ack_gate.add_permits(1);
        native_wait(&mut output, || {
            native_status(&harness, a.client_order_id()) == OrderStatus::Accepted
                || native_status(&harness, b.client_order_id()) == OrderStatus::Accepted
        })
        .await;

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("ioc_post_only")]
#[case("fok_post_only")]
#[case("gtd")]
#[case("day")]
#[case("market")]
#[case("reduce_only")]
#[case("quote_qty")]
#[case("display")]
#[case("fraction")]
#[case("precision")]
#[case("off_grid")]
#[case("closed")]
#[case("shard")]
#[case("market_identity")]
#[case("stp")]
#[case("pause")]
#[case("params")]
#[tokio::test]
async fn native_gtc_invalid_instructions_never_submit(#[case] scenario: &str) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;

        let mut builder = OrderTestBuilder::new(if scenario == "market" {
            OrderType::Market
        } else {
            OrderType::Limit
        });

        builder
            .trader_id(harness.trader_id())
            .strategy_id(StrategyId::from("NATIVE-001"))
            .instrument_id(bootstrap_instrument().id)
            .client_order_id(ClientOrderId::from("invalid-gtc"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(if scenario == "fraction" {
                "0.001"
            } else {
                "1.25"
            }))
            .price(Price::from(match scenario {
                "precision" => "0.600001",
                "off_grid" => "0.600100",
                _ => "0.600000",
            }))
            .time_in_force(match scenario {
                "ioc_post_only" => TimeInForce::Ioc,
                "fok_post_only" => TimeInForce::Fok,
                "gtd" => TimeInForce::Gtd,
                "day" => TimeInForce::Day,
                _ => TimeInForce::Gtc,
            })
            .reduce_only(scenario == "reduce_only")
            .post_only(matches!(scenario, "ioc_post_only" | "fok_post_only"))
            .quote_quantity(scenario == "quote_qty");

        if scenario == "gtd" {
            builder.expire_time(UnixNanos::from(u64::MAX));
        }

        if scenario == "display" {
            builder.display_qty(Quantity::from("0.25"));
        }

        let order = builder.build();

        match scenario {
            "closed" => peer.market.lock().unwrap()["status"] = "inactive".into(),
            "shard" => peer.market.lock().unwrap()["exchange_index"] = 2.into(),
            "market_identity" => peer.market.lock().unwrap()["ticker"] = "OTHER-MARKET".into(),
            _ => (),
        }

        let params = match scenario {
            "stp" => Some(nautilus_core::Params::new(
                [("self_trade_prevention_type".into(), "none".into())].into(),
            )),
            "pause" => Some(nautilus_core::Params::new(
                [("cancel_order_on_pause".into(), "true".into())].into(),
            )),
            "params" => Some(nautilus_core::Params::new(
                [("subaccount".into(), 1.into())].into(),
            )),
            _ => None,
        };

        native_submit(&harness, &order, params);
        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected Denied");
        };

        assert_eq!(denied.client_order_id, order.client_order_id());
        assert_eq!(denied.strategy_id, order.strategy_id());
        assert_eq!(denied.trader_id, harness.trader_id());
        assert_eq!(denied.instrument_id, order.instrument_id());

        let expected = match scenario {
            "ioc_post_only" | "fok_post_only" => {
                "VALIDATION_FAILED: Kalshi post-only requires GTC or GTD"
            }
            "gtd" => "VALIDATION_FAILED: Kalshi GTD expiry must use whole Unix seconds",
            "day" => "UNSUPPORTED_TIME_IN_FORCE: DAY",
            "market" => "UNSUPPORTED_ORDER_TYPE: MARKET",
            "reduce_only" => "UNSUPPORTED_REDUCE_ONLY",
            "quote_qty" => "UNSUPPORTED_QUOTE_QUANTITY",
            "display" => "VALIDATION_FAILED: unsupported Kalshi limit instruction combination",
            "fraction" | "precision" => "VALIDATION_FAILED: invalid exact Kalshi quantity or price",
            "off_grid" => "VALIDATION_FAILED: Kalshi price is off the market grid",
            "closed" => "VALIDATION_FAILED: Kalshi market is not open for trading",
            "shard" | "market_identity" => {
                "VALIDATION_FAILED: Kalshi submission market identity mismatch"
            }
            "stp" => "VALIDATION_FAILED: unsupported Kalshi self-trade prevention",
            "pause" => "VALIDATION_FAILED: invalid Kalshi cancel-on-pause",
            "params" => "VALIDATION_FAILED: unsupported Kalshi submission parameter",
            _ => unreachable!(),
        };

        assert_eq!(denied.reason.as_str(), expected);
        assert_eq!(
            *peer.submissions.writes.lock().unwrap(),
            Vec::<Value>::new()
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("1.0001"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("trader")]
#[case("client")]
#[case("strategy")]
#[case("instrument")]
#[case("order_init")]
#[tokio::test]
async fn native_gtc_command_identity_cannot_replace_cached_terms(#[case] field: &str) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "identity-gtc", OrderSide::Buy, false);
        harness
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(harness.client_id()), false)
            .unwrap();
        let mut cmd = SubmitOrder::from_order(
            &order,
            harness.trader_id(),
            Some(harness.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        match field {
            "trader" => cmd.trader_id = "OTHER-001".into(),
            "client" => cmd.client_id = Some("OTHER".into()),
            "strategy" => cmd.strategy_id = "OTHER-001".into(),
            "instrument" => cmd.instrument_id = "OTHER.KALSHI".into(),
            "order_init" => cmd.order_init.quantity = Quantity::from("1.00"),
            _ => unreachable!(),
        }

        let error = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .submit_order(cmd)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Kalshi submission command or cached identity mismatch"
        );
        assert_eq!(
            native_status(&harness, order.client_order_id()),
            OrderStatus::Initialized
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert!(output.try_recv().is_err());
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("command")]
#[case("cached")]
#[case("order")]
#[tokio::test]
async fn native_position_binding_cannot_select_another_strategy(#[case] binding: &str) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let mut order = native_order(&harness, "position-binding", OrderSide::Buy, false);
        let position_id = PositionId::from(format!("{}-OTHER-002", order.instrument_id()));
        if binding == "order" {
            order.set_position_id(Some(position_id));
        }

        harness
            .cache()
            .borrow_mut()
            .add_order(
                order.clone(),
                (binding == "cached").then_some(position_id),
                Some(harness.client_id()),
                false,
            )
            .unwrap();
        let cmd = SubmitOrder::from_order(
            &order,
            harness.trader_id(),
            Some(harness.client_id()),
            (binding == "command").then_some(position_id),
            UUID4::new(),
            UnixNanos::default(),
        );
        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .submit_order(cmd)
            .unwrap();

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected Denied")
        };

        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi NETTING position binding must match the submitting strategy"
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    false,
    false,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    false,
    false,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    true,
    false,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    true,
    false,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    true,
    true,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    true,
    true,
    TimeInForce::Gtc,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    false,
    false,
    TimeInForce::Ioc,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    false,
    false,
    TimeInForce::Ioc,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    false,
    false,
    TimeInForce::Fok,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    false,
    false,
    TimeInForce::Fok,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.250000",
    "1.25",
    false,
    false,
    TimeInForce::Gtd,
    "1.25"
)]
#[case(
    OrderSide::Sell,
    "0.500000",
    "-1.25",
    false,
    false,
    TimeInForce::Gtd,
    "1.25"
)]
#[case(
    OrderSide::Buy,
    "0.700000",
    "0.50",
    false,
    false,
    TimeInForce::Ioc,
    "0.50"
)]
#[case(
    OrderSide::Sell,
    "0.800000",
    "-0.50",
    false,
    false,
    TimeInForce::Ioc,
    "0.50"
)]
#[tokio::test]
async fn native_limit_fill_before_ack_and_duplicate_delivery_apply_once(
    #[case] side: OrderSide,
    #[case] cash: &str,
    #[case] net: &str,
    #[case] reset_queued: bool,
    #[case] tied_timestamps: bool,
    #[case] tif: TimeInForce,
    #[case] fill_qty: &str,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let mut builder = native_order_builder(&harness, "native-filled", side);
        builder.time_in_force(tif);
        if tif == TimeInForce::Gtd {
            builder.expire_time(UnixNanos::from(4_102_444_800_000_000_000));
        }

        let order = builder.build();
        native_submit(&harness, &order, None);
        peer.submissions.written.notified().await;
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        let now = if tied_timestamps {
            source["created_time"]
                .as_str()
                .unwrap()
                .parse::<jiff::Timestamp>()
                .unwrap()
        } else {
            jiff::Timestamp::now()
        };

        source["status"] = if fill_qty == "1.25" {
            "executed"
        } else {
            "canceled"
        }
        .into();

        source["fill_count_fp"] = fill_qty.into();
        source["remaining_count_fp"] = "0.00".into();
        source["last_update_time"] = now.to_string().into();
        source["taker_fees_dollars"] = "0.000100".into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["order_id"] = source["order_id"].clone();
        fill["ticker"] = source["ticker"].clone();
        fill["market_ticker"] = source["ticker"].clone();
        fill["outcome_side"] = source["outcome_side"].clone();
        fill["book_side"] = source["book_side"].clone();
        fill["count_fp"] = fill_qty.into();
        fill["yes_price_dollars"] = "0.600000".into();
        fill["no_price_dollars"] = "0.400000".into();
        fill["fee_cost"] = "0.000100".into();
        fill["created_time"] = now.to_string().into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = source["ticker"].clone();
        position["position_fp"] = net.into();
        position["last_updated_ts"] = now.to_string().into();
        *peer.orders.lock().unwrap() = json!({"orders":[source],"cursor":""});
        *peer.fills.lock().unwrap() = json!({"fills":[fill],"cursor":""});
        *peer.positions.lock().unwrap() =
            json!({"market_positions":[position],"event_positions":[],"cursor":""});
        peer.balance.lock().unwrap()["balance_dollars"] = cash.into();
        let id = order.client_order_id();
        let mut events = Vec::new();
        if reset_queued {
            native_reset_on_fill(&harness, &mut output, &mut events, id).await;
        }

        events.extend(
            native_wait(&mut output, || {
                native_status(&harness, id)
                    == if fill_qty == "1.25" {
                        OrderStatus::Filled
                    } else {
                        OrderStatus::Canceled
                    }
                    && native_balance(&harness).total.as_decimal() == dec(cash)
            })
            .await,
        );

        assert_eq!(events.len(), if fill_qty == "1.25" { 3 } else { 4 });

        if fill_qty != "1.25" {
            assert!(matches!(events[3], OrderEventAny::Canceled(_)));
        }

        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));

        let OrderEventAny::Filled(fill) = &events[2] else {
            panic!("expected Filled");
        };

        assert_eq!(fill.client_order_id, id);
        assert_eq!(fill.venue_order_id, VenueOrderId::from("native-1"));
        assert_eq!(fill.instrument_id, order.instrument_id());
        assert_eq!(fill.strategy_id, order.strategy_id());
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.order_type, OrderType::Limit);
        assert_eq!(fill.last_qty, Quantity::from(fill_qty));
        assert_eq!(fill.last_px, Price::from("0.600000"));
        assert_eq!(fill.commission.unwrap().as_decimal(), dec("0.000100"));
        assert_eq!(fill.commission.unwrap().currency, Currency::USD());
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);

        if tied_timestamps {
            let OrderEventAny::Accepted(accepted) = &events[1] else {
                unreachable!();
            };

            assert_eq!(fill.ts_event, accepted.ts_event);
        }

        let balance = native_balance(&harness);
        assert_eq!(balance.total.as_decimal(), dec(cash));
        assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
        assert_eq!(balance.free.as_decimal(), dec(cash));
        {
            let cache = harness.cache().borrow();
            let positions = cache.positions_open(
                None,
                Some(&order.instrument_id()),
                None,
                Some(&harness.account_id()),
                None,
            );
            assert_eq!(positions.len(), 1);
            assert_eq!(positions[0].signed_decimal_qty(), dec(net));
        }

        peer.submissions.ack_gate.add_permits(1);
        native_wait(&mut output, || {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let replay = native_reconnect(&harness, &mut output).await;

        assert!(
            replay
                .iter()
                .all(|event| matches!(event, ExecutionEvent::Report(ExecutionReport::Position(_))))
        );
        replay_bootstrap(&replay);
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().trade_ids(),
            vec![&TradeId::from("fill-a")]
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case(TimeInForce::Ioc)]
#[case(TimeInForce::Fok)]
#[tokio::test]
async fn native_notified_fill_readiness_admits_reduce_only_close(#[case] tif: TimeInForce) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order_builder(&harness, "native-current", OrderSide::Buy)
            .quantity(Quantity::from("0.01"))
            .time_in_force(tif)
            .build();
        peer.submissions.ack_gate.add_permits(1);
        native_submit(&harness, &order, None);
        let accepted = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(accepted.len(), 2);

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        let now = jiff::Timestamp::now();
        source["status"] = "executed".into();
        source["fill_count_fp"] = "0.01".into();
        source["remaining_count_fp"] = "0.00".into();
        source["last_update_time"] = now.to_string().into();
        source["taker_fees_dollars"] = "0.000000".into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["fill_id"] = "current-fill".into();
        fill["trade_id"] = "current-fill".into();
        fill["order_id"] = source["order_id"].clone();
        fill["ticker"] = source["ticker"].clone();
        fill["market_ticker"] = source["ticker"].clone();
        fill["outcome_side"] = source["outcome_side"].clone();
        fill["book_side"] = source["book_side"].clone();
        fill["count_fp"] = "0.01".into();
        fill["yes_price_dollars"] = "0.600000".into();
        fill["no_price_dollars"] = "0.400000".into();
        fill["fee_cost"] = "0.000000".into();
        fill["created_time"] = now.to_string().into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = source["ticker"].clone();
        position["position_fp"] = "0.01".into();
        position["realized_pnl_dollars"] = "0.000000".into();
        position["fees_paid_dollars"] = "0.000000".into();
        position["last_updated_ts"] = now.to_string().into();

        let mut notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["fill_native"]
                .clone();
        notification["msg"]["order_id"] = serde_json::json!(source["order_id"]);
        notification["msg"]["market_ticker"] = serde_json::json!(source["ticker"]);
        notification["msg"]["ts_ms"] = serde_json::json!(now.as_millisecond());
        *peer.notification.lock().unwrap() = Some(notification);

        peer.notification_ready.notify_one();
        wait_until_async(
            || async { peer.notification.lock().unwrap().is_none() },
            Duration::from_secs(2),
        )
        .await;
        let read_target = peer.balance_reads.load(Ordering::SeqCst) + 2;
        wait_until_async(
            || async { peer.balance_reads.load(Ordering::SeqCst) >= read_target },
            Duration::from_secs(4),
        )
        .await;

        *peer.orders.lock().unwrap() = json!({"orders":[source],"cursor":""});
        *peer.fills.lock().unwrap() = json!({"fills":[fill],"cursor":""});
        *peer.positions.lock().unwrap() =
            json!({"market_positions":[position],"event_positions":[],"cursor":""});
        peer.balance.lock().unwrap()["balance_dollars"] = "0.994100".into();

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Filled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
                && native_balance(&harness).total.as_decimal() == dec("0.9941")
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Filled(current) = &events[0] else {
            panic!("expected current fill");
        };

        assert_eq!(current.client_order_id, order.client_order_id());
        assert_eq!(current.venue_order_id, VenueOrderId::from("native-1"));
        assert_eq!(current.instrument_id, order.instrument_id());
        assert_eq!(current.strategy_id, order.strategy_id());
        assert_eq!(current.order_side, OrderSide::Buy);
        assert_eq!(current.order_type, OrderType::Limit);
        assert_eq!(current.trade_id, TradeId::from("current-fill"));
        assert_eq!(current.last_qty, Quantity::from("0.01"));
        assert_eq!(current.last_px, Price::from("0.600000"));
        assert_eq!(current.commission, Some(Money::from("0 USD")));

        let session = harness
            .cache()
            .borrow()
            .account(&harness.account_id())
            .unwrap()
            .last_event()
            .unwrap()
            .info;
        let mut notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_native"]
                .clone();
        notification["msg"]["market_ticker"] = serde_json::json!(position["ticker"]);
        notification["msg"]["position_fp"] = serde_json::json!(position["position_fp"]);
        notification["msg"]["realized_pnl_dollars"] =
            serde_json::json!(position["realized_pnl_dollars"]);
        notification["msg"]["fees_paid_dollars"] = serde_json::json!(position["fees_paid_dollars"]);
        *peer.notification.lock().unwrap() = Some(notification);

        peer.notification_ready.notify_one();

        let late = native_wait(&mut output, || {
            let current_session = harness
                .cache()
                .borrow()
                .account(&harness.account_id())
                .unwrap()
                .last_event()
                .unwrap()
                .info;
            current_session != session
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert!(late.is_empty());

        let replay = native_reconnect(&harness, &mut output).await;
        assert!(
            replay
                .iter()
                .all(|event| !matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))))
        );
        replay_bootstrap(&replay);

        *peer.submissions.outcome.lock().unwrap() = "immediate".into();
        peer.submissions.ack_gate.add_permits(1);
        let close = native_order_builder(&harness, "native-close", OrderSide::Sell)
            .quantity(Quantity::from("0.01"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        native_submit(&harness, &close, None);
        let closed = native_wait(&mut output, || {
            native_status(&harness, close.client_order_id()) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert!(
            closed.iter().all(|event| !matches!(
                event,
                OrderEventAny::Denied(_) | OrderEventAny::Rejected(_)
            ))
        );
        assert_eq!(
            closed
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            1
        );
        let writes = peer.submissions.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert_eq!(
            writes[0]["time_in_force"],
            if tif == TimeInForce::Ioc {
                "immediate_or_cancel"
            } else {
                "fill_or_kill"
            }
        );
        assert_eq!(writes[1]["client_order_id"], "native-close");
        assert_eq!(writes[1]["side"], "ask");
        assert_eq!(writes[1]["count"], "0.01");
        assert_eq!(writes[1]["time_in_force"], "immediate_or_cancel");
        assert_eq!(writes[1]["reduce_only"], true);
        assert_eq!(native_balance(&harness).total.as_decimal(), dec("0.9941"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

async fn native_reset_on_fill(
    harness: &ExecutionHarness,
    output: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    events: &mut Vec<OrderEventAny>,
    id: ClientOrderId,
) {
    loop {
        let event = output.recv().await.unwrap();
        if let ExecutionEvent::Order(order) = &event {
            events.push(order.clone());
        }

        if matches!(&event, ExecutionEvent::Order(OrderEventAny::Filled(_))) {
            assert_eq!(native_status(harness, id), OrderStatus::Accepted);
            harness
                .exec_engine()
                .borrow_mut()
                .get_client_adapter_mut(&harness.client_id())
                .unwrap()
                .reset()
                .unwrap();
            let replay = native_reconnect(harness, output).await;
            assert!(replay.iter().all(|event| matches!(
                event,
                ExecutionEvent::Report(ExecutionReport::Position(_))
            )));
            AsyncRunner::handle_exec_event(event);
            replay_bootstrap(&replay);
            break;
        }

        AsyncRunner::handle_exec_event(event);
    }
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn native_gtc_queued_denial_keeps_the_submission_claim(#[case] upfront: bool) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;

        let params = if upfront {
            Some(nautilus_core::Params::new(
                [("subaccount".into(), 1.into())].into(),
            ))
        } else {
            peer.market.lock().unwrap()["status"] = "inactive".into();
            None
        };

        let order = native_order(&harness, "denied-duplicate", OrderSide::Buy, false);
        native_submit(&harness, &order, params);
        let mut denied = None;
        while denied.is_none() {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(OrderEventAny::Denied(value)) = event {
                denied = Some(value);
            } else {
                AsyncRunner::handle_exec_event(event);
            }
        }

        assert_eq!(
            native_status(&harness, order.client_order_id()),
            OrderStatus::Initialized
        );
        let cmd = SubmitOrder::from_order(
            &order,
            harness.trader_id(),
            Some(harness.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        assert_eq!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .submit_order(cmd)
                .unwrap_err()
                .to_string(),
            "Kalshi submission already claimed"
        );

        let query = QueryOrder::new(
            harness.trader_id(),
            Some(harness.client_id()),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .query_order(query)
            .unwrap();

        let replay = native_wait(&mut output, || {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        assert_eq!(replay, Vec::<OrderEventAny>::new());
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("1.0001"));
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::Denied(
            denied.unwrap(),
        )));
        assert_eq!(
            native_status(&harness, order.client_order_id()),
            OrderStatus::Denied
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "lifecycle owns the client borrow and does not consume order events"
)]
async fn native_disconnect(harness: &ExecutionHarness) {
    harness
        .exec_engine()
        .borrow_mut()
        .get_client_adapter_mut(&harness.client_id())
        .unwrap()
        .disconnect()
        .await
        .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "bootstrap consumes accounts and queues order events until the client borrow ends"
)]
async fn native_reconnect(
    harness: &ExecutionHarness,
    output: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut engine = harness.exec_engine().borrow_mut();
    let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
    client.stop().unwrap();
    connect_consuming_accounts(client.as_mut(), output).await
}

#[rstest]
#[case("stop")]
#[case("reset")]
#[case("dispose")]
#[case("disconnect")]
#[tokio::test]
async fn native_gtc_shutdown_during_admission_denies_without_a_phantom_hold(
    #[case] operation: &str,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let order = native_order(&harness, "shutdown-admission", OrderSide::Buy, false);
        let balance = native_balance(&harness);
        native_submit(&harness, &order, None);
        peer.market_read.notified().await;

        if operation == "disconnect" {
            native_disconnect(&harness).await;
        } else {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            match operation {
                "stop" => client.stop(),
                "reset" => client.reset(),
                "dispose" => client.dispose(),
                _ => unreachable!(),
            }
            .unwrap();
        }

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected Denied");
        };

        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi submission canceled before dispatch"
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(native_balance(&harness), balance);
        drop(blocked);

        if operation != "dispose" {
            let replay = native_reconnect(&harness, &mut output).await;
            assert!(replay.is_empty());
            assert_eq!(native_balance(&harness), balance);
            assert_eq!(
                native_status(&harness, order.client_order_id()),
                OrderStatus::Denied
            );
        }

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_gtc_reset_keeps_pending_write_authority_for_late_recovery() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        *peer.submissions.outcome.lock().unwrap() = "unknown".into();
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "reset-pending", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        let events = native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Submitted
        })
        .await;

        assert_eq!(events.len(), 1);
        peer.submissions.written.notified().await;
        let held = native_balance(&harness);
        harness
            .exec_engine()
            .borrow_mut()
            .get_client_adapter_mut(&harness.client_id())
            .unwrap()
            .reset()
            .unwrap();
        native_disconnect(&harness).await;
        peer.submissions.ack_gate.add_permits(1);
        let replay = native_reconnect(&harness, &mut output).await;
        assert!(replay.is_empty());
        assert_eq!(native_status(&harness, id), OrderStatus::Submitted);
        assert_eq!(native_balance(&harness), held);
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source = fixtures["order"]["order"].clone();
        let now = jiff::Timestamp::now().to_string();
        source["order_id"] = "native-1".into();
        source["client_order_id"] = id.as_str().into();
        source["ticker"] = order.instrument_id().symbol.as_str().into();
        source["book_side"] = "bid".into();
        source["outcome_side"] = "yes".into();
        source["fill_count_fp"] = "0.00".into();
        source["remaining_count_fp"] = "1.25".into();
        source["initial_count_fp"] = "1.25".into();
        source["yes_price_dollars"] = "0.600000".into();
        source["no_price_dollars"] = "0.400000".into();
        source["created_time"] = now.clone().into();
        source["last_update_time"] = now.into();
        for key in [
            "taker_fees_dollars",
            "maker_fees_dollars",
            "taker_fill_cost_dollars",
            "maker_fill_cost_dollars",
        ] {
            source[key] = "0.000000".into();
        }

        *peer.orders.lock().unwrap() = json!({"orders":[source],"cursor":""});

        let events = native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert_eq!(
            harness
                .cache()
                .borrow()
                .order(&id)
                .unwrap()
                .venue_order_id(),
            Some(VenueOrderId::from("native-1"))
        );
        assert_eq!(native_balance(&harness), held);
        assert_eq!(held.total.as_decimal(), dec("1.0001"));
        assert_eq!(held.locked.as_decimal(), dec("0.784500"));
        assert_eq!(held.free.as_decimal(), dec("0.215600"));
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case(TimeInForce::Ioc, "immediate_or_cancel", OrderStatus::Canceled)]
#[case(TimeInForce::Fok, "fill_or_kill", OrderStatus::Canceled)]
#[case(TimeInForce::Gtd, "good_till_canceled", OrderStatus::Accepted)]
#[tokio::test]
async fn native_limit_matrix_recovers_final_state_and_exact_wire(
    #[case] tif: TimeInForce,
    #[case] wire_tif: &str,
    #[case] status: OrderStatus,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        if tif != TimeInForce::Gtd {
            *peer.submissions.outcome.lock().unwrap() = "immediate".into();
        }

        let (harness, mut output, _data) = native_consumer(&peer).await;
        let mut builder = native_order_builder(&harness, "matrix-native", side);
        builder
            .time_in_force(tif)
            .post_only(tif == TimeInForce::Gtd);
        if tif == TimeInForce::Gtd {
            builder.expire_time(UnixNanos::from(4_102_444_800_000_000_000));
        }

        let order = builder.build();
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        peer.submissions.ack_gate.add_permits(1);

        let events = native_wait(&mut output, || {
            native_status(&harness, id) == status
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let mut expected =
            serde_json::from_str::<Value>(include_str!("../../test_data/rest_requests.json"))
                .unwrap()["native_limit_matrix"]
                .clone();
        expected["ticker"] = serde_json::json!(order.instrument_id().symbol.as_str());
        expected["side"] = serde_json::json!(if side == OrderSide::Buy { "bid" } else { "ask" });
        expected["time_in_force"] = serde_json::json!(wire_tif);
        expected["post_only"] = serde_json::json!(tif == TimeInForce::Gtd);
        if tif == TimeInForce::Gtd {
            expected["expiration_time"] = 4_102_444_800_i64.into();
        }

        assert_eq!(*peer.submissions.writes.lock().unwrap(), vec![expected]);
        assert_eq!(
            events.len(),
            if status == OrderStatus::Canceled {
                3
            } else {
                2
            }
        );
        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));

        if status == OrderStatus::Canceled {
            assert!(matches!(events[2], OrderEventAny::Canceled(_)));
        }

        let locked = if status == OrderStatus::Canceled {
            Decimal::ZERO
        } else if side == OrderSide::Buy {
            dec("0.784500")
        } else {
            dec("0.534500")
        };

        assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
        assert_eq!(native_balance(&harness).locked.as_decimal(), locked);
        assert_eq!(
            native_balance(&harness).free.as_decimal(),
            dec("1.0001") - locked
        );
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().filled_qty(),
            Quantity::from("0.00")
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::flat_buy(
    None,
    OrderSide::Buy,
    "0.25",
    false,
    Some("VALIDATION_FAILED: Kalshi reduce-only side does not reduce the account net")
)]
#[case::flat_sell(
    None,
    OrderSide::Sell,
    "0.25",
    false,
    Some("VALIDATION_FAILED: Kalshi reduce-only side does not reduce the account net")
)]
#[case::wrong_side(
    Some(OrderSide::Buy),
    OrderSide::Buy,
    "0.25",
    false,
    Some("VALIDATION_FAILED: Kalshi reduce-only side does not reduce the account net")
)]
#[case::oversize(
    Some(OrderSide::Buy),
    OrderSide::Sell,
    "0.26",
    false,
    Some("VALIDATION_FAILED: Kalshi reduce-only quantity exceeds the unreserved account net")
)]
#[case::long(Some(OrderSide::Buy), OrderSide::Sell, "0.25", false, None)]
#[case::short(Some(OrderSide::Sell), OrderSide::Buy, "0.25", false, None)]
#[case::short_oversize(
    Some(OrderSide::Sell),
    OrderSide::Buy,
    "0.26",
    false,
    Some("VALIDATION_FAILED: Kalshi reduce-only quantity exceeds the unreserved account net")
)]
#[case::external(
    Some(OrderSide::Buy),
    OrderSide::Sell,
    "0.25",
    true,
    Some("VALIDATION_FAILED: Kalshi reduce-only quantity exceeds the unreserved account net")
)]
#[case::external_with_capacity(
    Some(OrderSide::Buy),
    OrderSide::Sell,
    "0.10",
    true,
    Some(concat!(
        "VALIDATION_FAILED: Kalshi reduce-only requires resolved external orders ",
        "on its reducing side"
    ))
)]
#[tokio::test]
async fn native_reduce_only_ioc_uses_account_net_across_strategies(
    #[case] opening: Option<OrderSide>,
    #[case] side: OrderSide,
    #[case] qty: &str,
    #[case] external_order: bool,
    #[case] reason: Option<&str>,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.market.lock().unwrap()["open_time"] = "2020-01-01T00:00:00Z".into();
        peer.market.lock().unwrap()["close_time"] = "2100-01-01T00:00:00Z".into();
        *peer.submissions.outcome.lock().unwrap() = "immediate".into();
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);

        if let Some(opening) = opening {
            let position_id = PositionId::from(format!("{}-OWNER-001", instrument.id));
            bootstrap_opening_position(&harness, &instrument, opening, position_id);
            let fixtures: Value =
                serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
            let mut position = fixtures["positions"]["market_positions"][0].clone();
            position["ticker"] = instrument.id.symbol.as_str().into();
            position["position_fp"] = if opening == OrderSide::Sell {
                "-0.25"
            } else {
                "0.25"
            }
            .into();

            *peer.positions.lock().unwrap() =
                json!({"market_positions":[position],"event_positions":[],"cursor":""});
        }

        if external_order {
            let fixtures: Value =
                serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
            let mut external = fixtures["order"]["order"].clone();
            external["order_id"] = "external-competing".into();
            external["client_order_id"] = "external-competing".into();
            external["ticker"] = instrument.id.symbol.as_str().into();
            external["book_side"] = "ask".into();
            external["outcome_side"] = "no".into();
            external["status"] = "resting".into();
            external["fill_count_fp"] = "0.00".into();
            external["remaining_count_fp"] = "0.10".into();
            external["initial_count_fp"] = "0.10".into();
            external["yes_price_dollars"] = "0.600000".into();
            external["no_price_dollars"] = "0.400000".into();
            external["expiration_time"] = Value::Null;
            *peer.orders.lock().unwrap() = json!({"orders":[external],"cursor":""});
        }

        client.start().unwrap();
        let queued = connect_consuming_accounts(&mut client, &mut output).await;
        assert!(queued.iter().all(|event| matches!(
            event,
            ExecutionEvent::Report(
                ExecutionReport::OrderWithFills(_, _) | ExecutionReport::Position(_)
            )
        )));
        assert_eq!(
            queued
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, _))
                ))
                .count(),
            usize::from(external_order) * 2
        );
        replay_bootstrap(&queued);
        harness.register_client(Box::new(client)).unwrap();

        let order = native_order_builder(&harness, "reduce-native", side)
            .strategy_id("OWNER-001".into())
            .quantity(Quantity::from(qty))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        peer.submissions.ack_gate.add_permits(1);

        let expected = if reason.is_none() {
            OrderStatus::Canceled
        } else {
            OrderStatus::Denied
        };

        let events = native_wait(&mut output, || {
            native_status(&harness, id) == expected
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        if let Some(reason) = reason {
            assert_eq!(events.len(), 1);

            let OrderEventAny::Denied(denied) = &events[0] else {
                panic!("expected Denied")
            };

            assert_eq!(denied.reason.as_str(), reason);
            assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        } else {
            assert_eq!(events.len(), 3);
            assert!(matches!(events[0], OrderEventAny::Submitted(_)));
            assert!(matches!(events[1], OrderEventAny::Accepted(_)));
            assert!(matches!(events[2], OrderEventAny::Canceled(_)));

            let mut expected =
                serde_json::from_str::<Value>(include_str!("../../test_data/rest_requests.json"))
                    .unwrap()["native_reduce"]
                    .clone();
            expected["ticker"] = serde_json::json!(instrument.id.symbol.as_str());
            expected["side"] =
                serde_json::json!(if side == OrderSide::Buy { "bid" } else { "ask" });

            assert_eq!(*peer.submissions.writes.lock().unwrap(), vec![expected]);
        }

        assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
        assert_eq!(
            native_balance(&harness).locked.as_decimal(),
            if external_order {
                dec("0.042760")
            } else {
                Decimal::ZERO
            }
        );
        assert_eq!(
            native_balance(&harness).free.as_decimal(),
            if external_order {
                dec("0.957340")
            } else {
                dec("1.0001")
            }
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_gtd_expiring_during_preparation_never_submits() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let expires = (now.as_u64() / 1_000_000_000 + 3) * 1_000_000_000;
        let order = native_order_builder(&harness, "expiry-preparation", OrderSide::Buy)
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(expires))
            .build();
        native_submit(&harness, &order, None);
        peer.market_read.notified().await;
        let remaining = expires.saturating_sub(
            nautilus_core::time::get_atomic_clock_realtime()
                .get_time_ns()
                .as_u64(),
        );
        tokio::time::sleep(Duration::from_nanos(remaining)).await;
        drop(blocked);

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected Denied")
        };

        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi GTD expiry must be in the future"
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("1.0001"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_prepared_submission_rejects_changed_external_authority() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut external = fixtures["order"]["order"].clone();
        external["order_id"] = "external-changing".into();
        external["client_order_id"] = "external-changing".into();
        external["ticker"] = bootstrap_instrument().id.symbol.as_str().into();
        external["book_side"] = "ask".into();
        external["outcome_side"] = "no".into();
        external["status"] = "resting".into();
        external["fill_count_fp"] = "0.00".into();
        external["remaining_count_fp"] = "0.10".into();
        external["initial_count_fp"] = "0.10".into();
        external["yes_price_dollars"] = "0.600000".into();
        external["no_price_dollars"] = "0.400000".into();
        external["expiration_time"] = Value::Null;
        *peer.orders.lock().unwrap() = json!({"orders":[external],"cursor":""});
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let order = native_order_builder(&harness, "external-preparation", OrderSide::Buy)
            .time_in_force(TimeInForce::Ioc)
            .build();
        native_submit(&harness, &order, None);
        peer.market_read.notified().await;
        {
            let mut orders = peer.orders.lock().unwrap();
            orders["orders"][0]["remaining_count_fp"] = "0.20".into();
            orders["orders"][0]["initial_count_fp"] = "0.20".into();
            orders["orders"][0]["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }

        native_wait(&mut output, || {
            native_balance(&harness).locked.as_decimal() == dec("0.085520")
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        drop(blocked);

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(denied) = &events[0] else {
            panic!("expected Denied")
        };

        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi submission requires current-session readiness"
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(
            native_balance(&harness).locked.as_decimal(),
            dec("0.085520")
        );
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("0.914580"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::long(OrderSide::Buy, "0.25", "1.1500")]
#[case::short(OrderSide::Sell, "-0.25", "1.1000")]
#[tokio::test]
async fn native_reduce_only_fills_keep_canonical_account_net(
    #[case] opening: OrderSide,
    #[case] net: &str,
    #[case] cash: &str,
    #[values(false, true)] cross_strategy: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.market.lock().unwrap()["open_time"] = "2020-01-01T00:00:00Z".into();
        peer.market.lock().unwrap()["close_time"] = "2100-01-01T00:00:00Z".into();
        let instrument = bootstrap_instrument();
        let (harness, mut client, mut output, _data) =
            bootstrap_consumer(&peer, &instrument, false);
        let position_id = PositionId::from(format!("{}-OWNER-001", instrument.id));
        bootstrap_opening_position(&harness, &instrument, opening, position_id);
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = instrument.id.symbol.as_str().into();
        position["position_fp"] = net.into();
        let snapshot_positions = json!({
            "market_positions":[position],
            "event_positions":[],
            "cursor":"",
        });
        *peer.positions.lock().unwrap() = snapshot_positions;
        client.start().unwrap();
        let queued = connect_consuming_accounts(&mut client, &mut output).await;
        replay_bootstrap(&queued);
        harness.register_client(Box::new(client)).unwrap();

        let side = if opening == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        let order = native_order_builder(&harness, "canonical-reduction", side)
            .strategy_id(if cross_strategy {
                "OTHER-002".into()
            } else {
                "OWNER-001".into()
            })
            .quantity(Quantity::from("0.25"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();

        native_submit(&harness, &order, None);

        if cross_strategy {
            let events = native_wait(&mut output, || {
                native_status(&harness, order.client_order_id()) == OrderStatus::Denied
            })
            .await;

            assert_eq!(events.len(), 1);

            let OrderEventAny::Denied(denied) = &events[0] else {
                panic!("expected Denied")
            };

            assert_eq!(
                denied.reason.as_str(),
                "VALIDATION_FAILED: Kalshi reduce-only requires a strategy-owned canonical position"
            );
            assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
            let canonical = harness
                .cache()
                .borrow()
                .positions_open_refs(
                    None,
                    Some(&instrument.id),
                    None,
                    Some(&harness.account_id()),
                    None,
                )
                .iter()
                .map(|position| position.signed_decimal_qty())
                .sum::<Decimal>();
            assert_eq!(canonical, dec(net));
            assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
            assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
            native_disconnect(&harness).await;
            return;
        }

        peer.submissions.written.notified().await;
        let competing = native_order_builder(&harness, "canonical-competing", opening)
            .strategy_id("OWNER-001".into())
            .quantity(Quantity::from("0.10"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        native_submit(&harness, &competing, None);
        let competing_events = native_wait(&mut output, || {
            native_status(&harness, competing.client_order_id()) == OrderStatus::Denied
        })
        .await;

        let competing_denial = competing_events
            .iter()
            .find_map(|event| {
                if let OrderEventAny::Denied(denied) = event {
                    Some(denied)
                } else {
                    None
                }
            })
            .unwrap();

        assert_eq!(
            competing_denial.reason.as_str(),
            concat!(
                "VALIDATION_FAILED: Kalshi reduce-only requires ",
                "exclusive strategy-instrument order flow",
            )
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        peer.market_read.notified().await;
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let delayed = native_order_builder(&harness, "canonical-delayed", side)
            .strategy_id("OWNER-001".into())
            .quantity(Quantity::from("0.10"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        native_submit(&harness, &delayed, None);
        peer.market_read.notified().await;
        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        let now = jiff::Timestamp::now().to_string();
        source["status"] = "executed".into();
        source["fill_count_fp"] = "0.25".into();
        source["remaining_count_fp"] = "0.00".into();
        source["last_update_time"] = now.clone().into();
        source["taker_fees_dollars"] = "0.000100".into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["order_id"] = source["order_id"].clone();
        fill["ticker"] = source["ticker"].clone();
        fill["market_ticker"] = source["ticker"].clone();
        fill["book_side"] = source["book_side"].clone();
        fill["outcome_side"] = source["outcome_side"].clone();
        fill["count_fp"] = "0.25".into();
        fill["yes_price_dollars"] = "0.600000".into();
        fill["no_price_dollars"] = "0.400000".into();
        fill["fee_cost"] = "0.000100".into();
        fill["created_time"] = now.clone().into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = source["ticker"].clone();
        position["position_fp"] = "0.00".into();
        position["last_updated_ts"] = now.into();
        let snapshot_orders = json!({
            "orders":[source],
            "cursor":"",
        });
        *peer.orders.lock().unwrap() = snapshot_orders;
        let snapshot_fills = json!({
            "fills":[fill],
            "cursor":"",
        });
        *peer.fills.lock().unwrap() = snapshot_fills;
        let snapshot_positions = json!({
            "market_positions":[position],
            "event_positions":[],
            "cursor":"",
        });
        *peer.positions.lock().unwrap() = snapshot_positions;
        peer.balance.lock().unwrap()["balance_dollars"] = cash.into();

        let events = native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Filled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let canonical = harness
            .cache()
            .borrow()
            .positions_open_refs(
                None,
                Some(&instrument.id),
                None,
                Some(&harness.account_id()),
                None,
            )
            .iter()
            .map(|position| position.signed_decimal_qty())
            .sum::<Decimal>();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            1
        );
        assert_eq!(native_balance(&harness).total.as_decimal(), dec(cash));
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(
            canonical,
            Decimal::ZERO,
            "canonical account net differs from flat venue net"
        );
        drop(blocked);

        let denied = native_wait(&mut output, || {
            native_status(&harness, delayed.client_order_id()) == OrderStatus::Denied
        })
        .await;

        assert_eq!(denied.len(), 1);

        let OrderEventAny::Denied(denied) = &denied[0] else {
            panic!("expected Denied")
        };

        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi submission requires current-session readiness"
        );
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        peer.submissions.ack_gate.add_permits(2);
        *peer.submissions.outcome.lock().unwrap() = "immediate".into();
        let next = native_order_builder(&harness, "canonical-after", opening)
            .strategy_id("OWNER-001".into())
            .quantity(Quantity::from("0.01"))
            .time_in_force(TimeInForce::Ioc)
            .build();
        native_submit(&harness, &next, None);
        native_wait(&mut output, || {
            native_status(&harness, next.client_order_id()) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 2);
        assert_eq!(native_balance(&harness).total.as_decimal(), dec(cash));
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

struct MutationPeer {
    writes: Mutex<Vec<(String, String)>>,
    mode: Mutex<String>,
    written: tokio::sync::Notify,
    ack_gate: Arc<tokio::sync::Semaphore>,
}

impl MutationPeer {
    async fn respond(
        &self,
        mut stream: tokio::net::TcpStream,
        headers: &str,
        orders: &Mutex<Value>,
    ) {
        let target = headers.lines().next().unwrap().to_owned();

        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|length| length.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);

        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await.unwrap();
        let body = String::from_utf8(bytes).unwrap();
        self.writes
            .lock()
            .unwrap()
            .push((target.clone(), body.clone()));

        let request: Value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap()
        };

        let mode = self.mode.lock().unwrap().clone();
        let now;
        let mut ack;
        {
            let mut rows = orders.lock().unwrap();
            let venue_id = target
                .split("/orders/")
                .nth(1)
                .unwrap()
                .split(['/', '?', ' '])
                .next()
                .unwrap();
            let source = rows["orders"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["order_id"] == venue_id)
                .unwrap();
            now = Self::timestamp(source, &mode);
            ack = json!({"order_id": source["order_id"], "client_order_id": source["client_order_id"], "ts_ms":now.as_millisecond()});
            ack["remaining_count"] = source["remaining_count_fp"].clone();
            ack["reduced_by"] = source["remaining_count_fp"].clone();

            if matches!(
                mode.as_str(),
                "success"
                    | "lost"
                    | "late-reject"
                    | "partial"
                    | "future"
                    | "partial-ahead"
                    | "partial-behind"
            ) {
                if target.contains("/amend?") {
                    let price = request["price"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap();
                    source["yes_price_dollars"] = request["price"].clone();
                    source["no_price_dollars"] = (Decimal::ONE - price).to_string().into();
                    let total = request["count"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap();
                    let filled = source["fill_count_fp"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap();
                    source["remaining_count_fp"] =
                        (total - filled).max(Decimal::ZERO).to_string().into();
                } else if target.contains("/decrease?") {
                    let remaining = source["remaining_count_fp"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap();

                    let reduce_by = if mode.starts_with("partial") {
                        dec("0.25")
                    } else {
                        request["reduce_by"]
                            .as_str()
                            .unwrap()
                            .parse::<Decimal>()
                            .unwrap()
                    };

                    source["remaining_count_fp"] = (remaining - reduce_by)
                        .max(Decimal::ZERO)
                        .to_string()
                        .into();

                    if source["remaining_count_fp"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap()
                        .is_zero()
                    {
                        source["status"] = "canceled".into();
                    }
                } else {
                    source["remaining_count_fp"] = "0.00".into();
                    source["status"] = "canceled".into();
                }

                source["last_update_time"] = now.to_string().into();
                ack["remaining_count"] = source["remaining_count_fp"].clone();
            }
        }

        self.written.notify_one();

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let (status, body) = match mode.as_str() {
            "unknown" | "lost" => (503, fixtures["reject_temporary"].to_string()),
            "not-found" => (404, fixtures["reject_missing_order"].to_string()),
            "reject" | "late-reject" => (400, fixtures["reject_parameters"].to_string()),
            "malformed" => (200, "{".into()),
            "identity" => {
                ack["order_id"] = "other-order".into();
                (200, ack.to_string())
            }
            "future" => {
                ack["ts_ms"] = (now.as_millisecond() + 86_400_000).into();
                (200, ack.to_string())
            }
            "stale" => {
                ack["ts_ms"] = 1.into();
                (200, ack.to_string())
            }
            _ => (200, ack.to_string()),
        };

        let gate = Arc::clone(&self.ack_gate);
        tokio::spawn(async move {
            let permit = gate.acquire().await.unwrap();
            permit.forget();
            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }

    fn timestamp(source: &Value, mode: &str) -> jiff::Timestamp {
        let now = jiff::Timestamp::now();

        match mode {
            "partial-ahead" => {
                jiff::Timestamp::from_millisecond(now.as_millisecond() + 60_000).unwrap()
            }
            "partial-behind" => {
                let previous: jiff::Timestamp = source["last_update_time"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                jiff::Timestamp::from_millisecond(previous.as_millisecond() + 1).unwrap()
            }
            _ => now,
        }
    }
}

fn native_modify(
    harness: &ExecutionHarness,
    order: &OrderAny,
    quantity: Option<Quantity>,
    price: Option<Price>,
) -> UUID4 {
    let command_id = UUID4::new();

    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(harness.client_id()),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.venue_order_id(),
        quantity,
        price,
        None,
        command_id,
        UnixNanos::default(),
        None,
        None,
    );

    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .modify_order(cmd)
        .unwrap();
    command_id
}

fn native_cancel(harness: &ExecutionHarness, order: &OrderAny) -> UUID4 {
    let command_id = UUID4::new();

    let cmd = CancelOrder::new(
        order.trader_id(),
        Some(harness.client_id()),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.venue_order_id(),
        command_id,
        UnixNanos::default(),
        None,
        None,
    );

    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .cancel_order(cmd)
        .unwrap();
    command_id
}

#[rstest]
#[tokio::test]
async fn native_closed_order_changes_are_denied_before_dispatch(
    #[values(false, true)] modify: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "closed-change", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let accepted = harness.cache().borrow().order(&id).unwrap().clone();
        native_cancel(&harness, &accepted);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let closed = harness.cache().borrow().order(&id).unwrap().clone();
        let balance = native_balance(&harness);
        let command_id = if modify {
            native_modify(&harness, &closed, None, Some(Price::from("0.500000")))
        } else {
            native_cancel(&harness, &closed)
        };

        let events = tokio::time::timeout(
            Duration::from_secs(2),
            native_wait(&mut output, || {
                match harness.cache().borrow().order(&id).unwrap().last_event() {
                    OrderEventAny::CancelDenied(event) => event.causation_id == Some(command_id),
                    OrderEventAny::ModifyDenied(event) => event.causation_id == Some(command_id),
                    _ => false,
                }
            }),
        )
        .await
        .expect("closed-order command must emit its denial");

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(matches!(
            (modify, event),
            (true, OrderEventAny::ModifyDenied(_)) | (false, OrderEventAny::CancelDenied(_))
        ));
        assert_eq!(event.trader_id(), closed.trader_id());
        assert_eq!(event.strategy_id(), closed.strategy_id());
        assert_eq!(event.instrument_id(), closed.instrument_id());
        assert_eq!(event.client_order_id(), id);
        assert_eq!(event.venue_order_id(), closed.venue_order_id());
        assert_eq!(event.account_id(), closed.account_id());

        let causation_id = match event {
            OrderEventAny::CancelDenied(event) => event.causation_id,
            OrderEventAny::ModifyDenied(event) => event.causation_id,
            _ => unreachable!(),
        };

        assert_eq!(causation_id, Some(command_id));
        assert_eq!(event.ts_event(), closed.ts_last());
        assert_eq!(
            event.message().unwrap().as_str(),
            "VALIDATION_FAILED: Kalshi order is already closed"
        );
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(cached.status(), OrderStatus::Canceled);
        assert_eq!(cached.quantity(), closed.quantity());
        assert_eq!(cached.price(), closed.price());
        assert_eq!(cached.filled_qty(), closed.filled_qty());
        assert_eq!(cached.leaves_qty(), closed.leaves_qty());
        assert_eq!(cached.venue_order_id(), closed.venue_order_id());
        assert!(!cached.has_pending_cancel_command());
        assert!(!cached.has_pending_update_command());
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_individual_cancels_drain_both_orders_across_readiness_invalidation() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        peer.submissions.ack_gate.add_permits(2);
        peer.changes.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let mut orders = Vec::new();

        for (name, side) in [("stop-buy", OrderSide::Buy), ("stop-sell", OrderSide::Sell)] {
            let order = native_order(&harness, name, side, false);
            let id = order.client_order_id();
            native_submit(&harness, &order, None);
            native_wait(&mut output, || {
                let connected = harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected();
                native_status(&harness, id) == OrderStatus::Accepted && connected
            })
            .await;

            orders.push(harness.cache().borrow().order(&id).unwrap().clone());
        }

        for order in &orders {
            native_cancel(&harness, order);
        }

        let events = native_wait(&mut output, || {
            let connected = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();

            for order in &orders {
                let cached = harness
                    .cache()
                    .borrow()
                    .order(&order.client_order_id())
                    .unwrap()
                    .clone();
                assert!(
                    !matches!(cached.last_event(), OrderEventAny::CancelDenied(_)),
                    "{cached:?}"
                );
            }

            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Canceled
            }) && connected
        })
        .await;

        let writes = peer.changes.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert_eq!(events.len(), 2);

        for order in &orders {
            let id = order.client_order_id();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, OrderEventAny::Canceled(event) if event.client_order_id == id))
                    .count(),
                1
            );
            assert_eq!(
                writes
                    .iter()
                    .filter(|(target, _)| target.contains(&order.venue_order_id().unwrap().to_string()))
                    .count(),
                1
            );
            let current = harness.cache().borrow().order(&id).unwrap().clone();
            assert_eq!(current.quantity(), order.quantity());
            assert_eq!(current.filled_qty(), Quantity::from("0.00"));
            assert_eq!(current.price(), order.price());
        }

        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert!(
            peer.orders.lock().unwrap()["orders"]
                .as_array()
                .unwrap()
                .iter()
                .all(|order| order["status"] == "canceled")
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

const NATIVE_ADMISSION_TIMEOUT_REASON: &str =
    "VALIDATION_FAILED: Kalshi change admission timed out before dispatch";
const NATIVE_GENERATION_CHANGE_REASON: &str =
    "VALIDATION_FAILED: Kalshi change connection generation changed before admission";

#[rstest]
#[case::amend(OrderStatus::Accepted, false, false, None)]
#[case::amend_reconnect(
    OrderStatus::Accepted,
    false,
    true,
    Some(NATIVE_GENERATION_CHANGE_REASON)
)]
#[case::cancel(OrderStatus::Canceled, false, false, None)]
#[case::cancel_reconnect(
    OrderStatus::Canceled,
    false,
    true,
    Some(NATIVE_GENERATION_CHANGE_REASON)
)]
#[case::cancel_after_fill(OrderStatus::Canceled, true, false, None)]
#[case::amend_expired(
    OrderStatus::Accepted,
    false,
    false,
    Some(NATIVE_ADMISSION_TIMEOUT_REASON)
)]
#[case::cancel_expired(
    OrderStatus::Canceled,
    false,
    false,
    Some(NATIVE_ADMISSION_TIMEOUT_REASON)
)]
#[tokio::test]
async fn native_changes_wait_for_observed_recovery_before_admission(
    #[case] expected: OrderStatus,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[case] partial_fill: bool,
    #[case] reconnect: bool,
    #[case] rejection_reason: Option<&str>,
) {
    let cancel = expected == OrderStatus::Canceled;
    let expired = rejection_reason == Some(NATIVE_ADMISSION_TIMEOUT_REASON);

    tokio::time::timeout(Duration::from_secs(if expired { 40 } else { 18 }), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "queued-change", side, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let cached = harness.cache().borrow().order(&id).unwrap().clone();

        let query = QueryOrder::new(
            harness.trader_id(),
            Some(harness.client_id()),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            cached.venue_order_id(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .query_order(query)
            .unwrap();

        let first_account = loop {
            let event = output.recv().await.unwrap();
            if matches!(event, ExecutionEvent::Account(_)) {
                break event;
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );

        let command_id = if cancel {
            native_cancel(&harness, &cached)
        } else {
            native_modify(&harness, &cached, None, Some(Price::from("0.500000")))
        };

        if partial_fill {
            native_fill(&peer, "0.25", false);
        }

        if reconnect {
            *peer.notification.lock().unwrap() = Some(json!({"type": "disconnect"}));
            peer.notification_ready.notify_one();
            wait_until_async(
                || async { peer.notification.lock().unwrap().is_none() },
                Duration::from_secs(2),
            )
            .await;
        }

        if let Some(rejection_reason) = rejection_reason {
            let mut recovery = if reconnect {
                native_reconnect(&harness, &mut output).await.into_iter()
            } else {
                Vec::new().into_iter()
            };

            let outcome = tokio::time::timeout(Duration::from_secs(18), async {
                loop {
                    let event = if let Some(event) = recovery.next() {
                        event
                    } else {
                        tokio::select! {
                            () = peer.changes.written.notified() => {
                                panic!("queued change dispatched without observed recovery");
                            }
                            event = output.recv() => event.unwrap(),
                        }
                    };

                    match event {
                        ExecutionEvent::Order(event) => break event,
                        ExecutionEvent::Account(_) => {}
                        event => AsyncRunner::handle_exec_event(event),
                    }
                }
            })
            .await
            .expect("queued change must be denied before the default inflight timeout");

            assert!(matches!(
                (cancel, &outcome),
                (true, OrderEventAny::CancelDenied(_)) | (false, OrderEventAny::ModifyDenied(_))
            ));
            let outcome = outcome.into_boxed();
            assert_eq!(outcome.causation_id(), Some(command_id));
            assert_eq!(outcome.reason().unwrap().as_str(), rejection_reason);
            assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);

            if expired {
                native_dispatch([first_account]);
                native_wait(&mut output, || {
                    harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
                })
                .await;

                let current = harness.cache().borrow().order_owned(&id).unwrap();
                assert_eq!(current.status(), OrderStatus::Accepted);
                assert_eq!(current.quantity(), cached.quantity());
                assert_eq!(current.price(), cached.price());
                assert_eq!(current.filled_qty(), cached.filled_qty());
                assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
            }

            native_disconnect(&harness).await;
            return;
        }

        let mut recovered = Vec::new();
        let mut fill_observed = !partial_fill;

        let next_account = loop {
            tokio::select! {
                () = peer.changes.written.notified() => {
                    panic!("change dispatched before account observation");
                }
                event = output.recv() => {
                    let event = event.unwrap();
                    if matches!(event, ExecutionEvent::Account(_)) {
                        if fill_observed {
                            break event;
                        }
                        continue;
                    }

                    if let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = &event {
                        assert!(partial_fill);
                        assert_eq!(fill.last_qty, Quantity::from("0.25"));
                        recovered.push(OrderEventAny::Filled(fill.clone()));
                        fill_observed = true;
                    } else {
                        assert!(!matches!(event, ExecutionEvent::Order(_)), "{event:?}");
                    }
                    AsyncRunner::handle_exec_event(event);
                }
            }
        };

        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        native_dispatch([first_account, next_account]);

        let events = native_wait(&mut output, || {
            let connected = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            let cache = harness.cache().borrow();
            let current = cache.order(&id).unwrap();
            (if cancel {
                current.status() == OrderStatus::Canceled
            } else {
                current.price() == Some(Price::from("0.500000"))
            }) && connected
        })
        .await;

        recovered.extend(events);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        assert_eq!(recovered.len(), if partial_fill { 2 } else { 1 });
        let changes: Vec<_> = recovered
            .iter()
            .filter(|event| !matches!(event, OrderEventAny::Filled(_)))
            .collect();
        assert_eq!(changes.len(), 1);
        assert!(
            matches!(
                changes[0],
                OrderEventAny::Canceled(_) if cancel
            ) || matches!(changes[0], OrderEventAny::Updated(_) if !cancel)
        );
        assert_eq!(native_status(&harness, id), expected);
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().filled_qty(),
            Quantity::from(if partial_fill { "0.25" } else { "0.00" })
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::price("price", "1.25", "0.500000", false)]
#[case::increase("increase", "1.50", "0.600000", false)]
#[case::decrease("decrease", "0.50", "0.600000", false)]
#[case::zero("zero", "0.00", "0.600000", true)]
#[case::cancel("cancel", "1.25", "0.600000", true)]
#[tokio::test]
async fn native_change_wire_and_final_state(
    #[case] mode: &str,
    #[case] qty: &str,
    #[case] price: &str,
    #[case] canceled: bool,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(0, 7)] subaccount: u32,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_scope(None, 10, subaccount).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);

        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change", side, false);
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let cached = harness
            .cache()
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .clone();

        if mode == "cancel" {
            native_cancel(&harness, &cached);
        } else {
            native_modify(
                &harness,
                &cached,
                if mode == "price" {
                    None
                } else {
                    Some(Quantity::from(qty))
                },
                if mode == "price" {
                    Some(Price::from(price))
                } else {
                    None
                },
            );
        }

        let events = native_wait(&mut output, || {
            let cache = harness.cache().borrow();
            let current = cache.order(&order.client_order_id()).unwrap();
            (if canceled {
                current.status() == OrderStatus::Canceled
            } else {
                current.quantity() == Quantity::from(qty)
                    && current.price() == Some(Price::from(price))
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let writes = peer.changes.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);

        assert_change_wire(&writes[0], mode, &order, subaccount);
        assert_eq!(
            peer.submissions.writes.lock().unwrap()[0]["subaccount"],
            subaccount
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            usize::from(canceled)
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Updated(_)))
                .count(),
            usize::from(!canceled)
        );
        assert!(events.iter().all(|event| !matches!(
            event,
            OrderEventAny::ModifyRejected(_)
                | OrderEventAny::CancelRejected(_)
                | OrderEventAny::Filled(_)
        )));
        let current = harness
            .cache()
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .clone();
        assert_eq!(current.filled_qty(), Quantity::from("0.00"));
        assert_eq!(current.venue_order_id(), cached.venue_order_id());
        assert_eq!(current.price(), Some(Price::from(price)));
        assert_eq!(
            current.quantity(),
            if canceled {
                cached.quantity()
            } else {
                Quantity::from(qty)
            }
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

fn assert_change_wire(wire: &(String, String), mode: &str, order: &OrderAny, subaccount: u32) {
    let side = order.order_side();

    if mode == "cancel" {
        assert_eq!(
            *wire,
            (
                format!(
                    "DELETE /portfolio/events/orders/native-1?subaccount={subaccount}&exchange_index=3&market_ticker={} HTTP/1.1",
                    order.instrument_id().symbol
                ),
                String::new()
            )
        );
    } else {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_requests.json")).unwrap();

        let (path, body) = if mode == "decrease" || mode == "zero" {
            let mut body = fixtures["native_decrease"].clone();
            body["market_ticker"] = order.instrument_id().symbol.as_str().into();
            body["reduce_by"] = if mode == "zero" { "1.25" } else { "0.75" }.into();

            (
                format!(
                    "POST /portfolio/events/orders/native-1/decrease?subaccount={subaccount} HTTP/1.1"
                ),
                body,
            )
        } else {
            let mut body = fixtures["native_amend"].clone();
            body["count"] = if mode == "price" { "1.25" } else { "1.5" }.into();
            body["price"] = if mode == "price" { "0.5000" } else { "0.6000" }.into();
            body["side"] = if side == OrderSide::Buy { "bid" } else { "ask" }.into();
            body["ticker"] = order.instrument_id().symbol.as_str().into();

            (
                format!(
                    "POST /portfolio/events/orders/native-1/amend?subaccount={subaccount} HTTP/1.1"
                ),
                body,
            )
        };

        assert_eq!(*wire, (path, body.to_string()));
    }
}

fn native_fill(peer: &Peer, qty: &str, terminal: bool) -> Decimal {
    let id = peer.orders.lock().unwrap()["orders"][0]["client_order_id"]
        .as_str()
        .unwrap()
        .to_owned();
    native_fill_for_order(peer, &id, qty, terminal)
}

fn native_fill_for_order(peer: &Peer, id: &str, qty: &str, terminal: bool) -> Decimal {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let quantity = dec(qty);
    let now = jiff::Timestamp::now().to_string();
    let mut source = peer.orders.lock().unwrap()["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["client_order_id"] == id)
        .unwrap()
        .clone();
    let remaining = dec(source["remaining_count_fp"].as_str().unwrap());
    source["fill_count_fp"] = qty.into();
    source["remaining_count_fp"] = (remaining - quantity).max(Decimal::ZERO).to_string().into();
    if terminal {
        source["status"] = "executed".into();
    }

    source["last_update_time"] = now.clone().into();
    source["taker_fees_dollars"] = "0.000100".into();
    let mut fill = fixtures["fills"]["fills"][0].clone();
    fill["order_id"] = source["order_id"].clone();
    fill["ticker"] = source["ticker"].clone();
    fill["market_ticker"] = source["ticker"].clone();
    fill["outcome_side"] = source["outcome_side"].clone();
    fill["book_side"] = source["book_side"].clone();
    fill["count_fp"] = qty.into();
    fill["yes_price_dollars"] = "0.600000".into();
    fill["no_price_dollars"] = "0.400000".into();
    fill["fee_cost"] = "0.000100".into();
    fill["created_time"] = now.clone().into();
    let mut position = fixtures["positions"]["market_positions"][0].clone();
    position["ticker"] = source["ticker"].clone();
    let buy = source["book_side"] == "bid";
    position["position_fp"] = if buy { quantity } else { -quantity }.to_string().into();
    position["last_updated_ts"] = now.into();
    *peer.orders.lock().unwrap()["orders"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["client_order_id"] == id)
        .unwrap() = source;
    *peer.fills.lock().unwrap() = json!({"fills":[fill],"cursor":""});
    *peer.positions.lock().unwrap() =
        json!({"market_positions":[position],"event_positions":[],"cursor":""});
    let cash = dec("1.0001") - quantity * if buy { dec("0.6") } else { dec("0.4") } - dec("0.0001");
    peer.balance.lock().unwrap()["balance_dollars"] = cash.to_string().into();
    cash
}

#[rstest]
#[case::buy_insufficient(OrderSide::Buy, "0.400000", "1.000100", false)]
#[case::buy_exact(OrderSide::Buy, "0.400000", "1.105200", true)]
#[case::sell_insufficient(OrderSide::Sell, "0.800000", "0.600100", false)]
#[case::sell_exact(OrderSide::Sell, "0.800000", "0.705200", true)]
#[tokio::test]
async fn native_amendment_reserves_old_fills_and_new_remaining_quantity(
    #[case] side: OrderSide,
    #[case] price: &str,
    #[case] cash: &str,
    #[case] admitted: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.balance.lock().unwrap()["balance_dollars"] = cash.into();
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "amend-mixed-hold", side, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let original = native_balance(&harness);
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let command = native_modify(
            &harness,
            &cached,
            Some(Quantity::from("2.00")),
            Some(Price::from(price)),
        );

        let dispatched = loop {
            tokio::select! {
                () = peer.changes.written.notified() => break true,
                event = output.recv() => {
                    let event = event.unwrap();
                    if let ExecutionEvent::Order(OrderEventAny::ModifyDenied(rejected)) = &event {
                        assert_eq!(rejected.causation_id, Some(command));
                        assert_eq!(
                            rejected.reason.as_str(),
                            "VALIDATION_FAILED: insufficient Kalshi cash for admission holds"
                        );
                        AsyncRunner::handle_exec_event(event);
                        break false;
                    }
                    AsyncRunner::handle_exec_event(event);
                }
            }
        };

        assert_eq!(dispatched, admitted);

        if admitted {
            native_fill(&peer, "1.00", false);

            let principal = if side == OrderSide::Buy {
                "0.60"
            } else {
                "0.40"
            };

            let remaining_cash = dec(cash) - dec(principal) - dec("0.000100");
            peer.balance.lock().unwrap()["balance_dollars"] = remaining_cash.to_string().into();
            peer.changes.ack_gate.add_permits(1);
            native_wait(&mut output, || {
                harness.cache().borrow().order(&id).unwrap().filled_qty() == Quantity::from("1.00")
                    && harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
            })
            .await;

            let current = harness.cache().borrow().order(&id).unwrap().clone();

            let locked = if side == OrderSide::Buy {
                "0.427600"
            } else {
                "0.227600"
            };

            let balance = native_balance(&harness);
            assert_eq!(current.status(), OrderStatus::PartiallyFilled);
            assert_eq!(current.quantity(), Quantity::from("2.00"));
            assert_eq!(current.filled_qty(), Quantity::from("1.00"));
            assert_eq!(current.leaves_qty(), Quantity::from("1.00"));
            assert_eq!(current.price(), Some(Price::from(price)));
            assert_eq!(balance.total.as_decimal(), remaining_cash);
            assert_eq!(balance.locked.as_decimal(), dec(locked));
            assert_eq!(balance.free.as_decimal(), dec("0.077500"));
            assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        } else {
            assert_eq!(native_balance(&harness), original);
            assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
            assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        }

        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::before_amend("amend", true, false, (OrderStatus::PartiallyFilled, "0.50", "0.25", "0.500000"))]
#[case::before_decrease("decrease", true, false, (OrderStatus::PartiallyFilled, "0.50", "0.25", "0.600000"))]
#[case::during_amend("amend", false, false, (OrderStatus::PartiallyFilled, "0.50", "0.25", "0.500000"))]
#[case::during_decrease("decrease", false, false, (OrderStatus::PartiallyFilled, "0.50", "0.25", "0.600000"))]
#[case::partial_cancel("cancel", false, false, (OrderStatus::Canceled, "1.25", "1.00", "0.600000"))]
#[case::full_cancel("cancel", false, true, (OrderStatus::Filled, "1.25", "0.00", "0.600000"))]
#[tokio::test]
async fn native_change_accounts_for_fills(
    #[case] operation: &str,
    #[case] before: bool,
    #[case] full: bool,
    #[case] expected: (OrderStatus, &str, &str, &str),
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change-fill", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        let id = order.client_order_id();
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let mut events = Vec::new();

        if before {
            native_fill(&peer, "0.25", false);
            events.extend(
                native_wait(&mut output, || {
                    native_status(&harness, id) == OrderStatus::PartiallyFilled
                        && harness
                            .exec_engine()
                            .borrow()
                            .get_client(&harness.client_id())
                            .unwrap()
                            .is_connected()
                })
                .await,
            );
        }

        if full {
            *peer.changes.mode.lock().unwrap() = "unknown".into();
        }

        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        if operation == "cancel" {
            native_cancel(&harness, &cached);
        } else {
            native_modify(
                &harness,
                &cached,
                Some(Quantity::from("0.50")),
                (operation == "amend").then_some(Price::from("0.500000")),
            );
        }

        peer.changes.written.notified().await;

        let filled = if full { "1.25" } else { "0.25" };

        let cash = if before {
            dec("0.850000")
        } else {
            native_fill(&peer, filled, full)
        };

        peer.changes.ack_gate.add_permits(1);
        events.extend(
            native_wait(&mut output, || {
                let cache = harness.cache().borrow();
                let current = cache.order(&id).unwrap();
                current.filled_qty() == Quantity::from(filled)
                    && current.status() == expected.0
                    && current.quantity() == Quantity::from(expected.1)
                    && current.price() == Some(Price::from(expected.3))
                    && harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
            })
            .await,
        );

        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let (status, quantity, leaves, price) = expected;
        assert_eq!(cached.status(), status);
        assert_eq!(cached.quantity(), Quantity::from(quantity));
        assert_eq!(cached.filled_qty(), Quantity::from(filled));
        assert_eq!(cached.leaves_qty(), Quantity::from(leaves));
        assert_eq!(cached.price(), Some(Price::from(price)));
        assert_eq!(
            cached.venue_order_id(),
            Some(VenueOrderId::from("native-1"))
        );
        assert_eq!(native_balance(&harness).total.as_decimal(), cash);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            usize::from(expected.0 == OrderStatus::Canceled)
        );
        assert!(events.iter().all(|event| !matches!(
            event,
            OrderEventAny::ModifyRejected(_) | OrderEventAny::CancelRejected(_)
        )));
        let writes = peer.changes.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);

        if operation == "amend" {
            let body: Value = serde_json::from_str(&writes[0].1).unwrap();
            assert_eq!(body["count"], "0.5");
        }

        if operation == "decrease" {
            let body: Value = serde_json::from_str(&writes[0].1).unwrap();
            assert_eq!(body["reduce_by"], "0.75");
        }

        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("unknown")]
#[case("not-found")]
#[case("malformed")]
#[case("identity")]
#[case("stale")]
#[case("lost")]
#[case("future")]
#[tokio::test]
async fn native_change_ambiguity_reconciles_without_replay(
    #[case] mode: &str,
    #[values(false, true)] cancel: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 1).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change-unknown", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        let id = order.client_order_id();
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        *peer.changes.mode.lock().unwrap() = mode.into();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        if cancel {
            native_cancel(&harness, &cached);
        } else {
            native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        }

        peer.changes.written.notified().await;
        peer.changes.ack_gate.add_permits(1);
        native_disconnect(&harness).await;
        let mut events = native_dispatch(std::iter::from_fn(|| output.try_recv().ok()));

        if !matches!(mode, "lost" | "future") {
            assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
            assert_eq!(
                harness.cache().borrow().order(&id).unwrap().price(),
                Some(Price::from("0.600000"))
            );
            assert!(events.is_empty());
        }

        let mut evidence = peer.orders.lock().unwrap()["orders"][0].clone();
        if cancel {
            evidence["status"] = "canceled".into();
            evidence["remaining_count_fp"] = "0.00".into();
        } else {
            evidence["yes_price_dollars"] = "0.5000".into();
            evidence["no_price_dollars"] = "0.5000".into();
        }

        evidence["last_update_time"] = jiff::Timestamp::now().to_string().into();
        *peer.orders.lock().unwrap() = json!({"orders":[evidence],"cursor":""});

        events.extend(native_dispatch(
            native_reconnect(&harness, &mut output).await,
        ));

        if !cancel {
            events.extend(
                native_wait(&mut output, || {
                    harness.cache().borrow().order(&id).unwrap().price()
                        == Some(Price::from("0.500000"))
                })
                .await,
            );

            assert!(
                !harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            );
            {
                let mut rows = peer.orders.lock().unwrap();
                rows["orders"][0]["status"] = "canceled".into();
                rows["orders"][0]["remaining_count_fp"] = "0.00".into();
                rows["orders"][0]["last_update_time"] = jiff::Timestamp::now().to_string().into();
            }
        }

        events.extend(
            native_wait(&mut output, || {
                native_status(&harness, id) == OrderStatus::Canceled
                    && harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
            })
            .await,
        );

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Updated(_)))
                .count(),
            usize::from(!cancel)
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            1
        );
        assert_eq!(events.len(), if cancel { 1 } else { 2 });
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        assert_eq!(native_status(&harness, id), OrderStatus::Canceled);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::venue_reject("reject", "Invalid parameters", true)]
#[case::off_grid(
    "off-grid",
    "VALIDATION_FAILED: Kalshi price is off the market grid",
    false
)]
#[case::unchanged(
    "unchanged",
    "VALIDATION_FAILED: Kalshi amendment does not change order terms",
    false
)]
#[tokio::test]
async fn native_modify_rejection_is_scoped_and_clean(
    #[case] mode: &str,
    #[case] reason: &str,
    #[case] sent: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change-reject", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        let id = order.client_order_id();
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        *peer.changes.mode.lock().unwrap() = mode.into();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();

        let command_id = native_modify(
            &harness,
            &cached,
            None,
            Some(Price::from(match mode {
                "off-grid" => "0.500100",
                "unchanged" => "0.600000",
                _ => "0.500000",
            })),
        );

        let event = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(outcome) = &event
                && matches!(
                    outcome,
                    OrderEventAny::ModifyDenied(_) | OrderEventAny::ModifyRejected(_)
                )
            {
                let outcome = outcome.clone();
                AsyncRunner::handle_exec_event(event);
                break outcome;
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert_eq!(matches!(event, OrderEventAny::ModifyRejected(_)), sent);
        assert_eq!(matches!(event, OrderEventAny::ModifyDenied(_)), !sent);
        let event = event.into_boxed();
        assert_eq!(event.trader_id(), order.trader_id());
        assert_eq!(event.strategy_id(), order.strategy_id());
        assert_eq!(event.instrument_id(), order.instrument_id());
        assert_eq!(event.client_order_id(), id);
        assert_eq!(event.venue_order_id(), cached.venue_order_id());
        assert_eq!(event.account_id(), Some(harness.account_id()));
        assert_eq!(event.causation_id(), Some(command_id));
        assert_eq!(event.reason().unwrap().as_str(), reason);
        assert!(!event.reconciliation());
        assert_eq!(event.ts_event(), cached.ts_last());
        assert!(event.ts_init() >= event.ts_event());
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), usize::from(sent));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_change_rejection_preserves_recovered_terms() {
    tokio::time::timeout(Duration::from_secs(22), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change-late", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        *peer.changes.mode.lock().unwrap() = "late-reject".into();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let command_id = native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        peer.changes.written.notified().await;

        let first = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.500000"))
        })
        .await;

        assert_eq!(first.len(), 1);
        assert!(matches!(first[0], OrderEventAny::Updated(_)));
        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        peer.changes.ack_gate.add_permits(1);

        let rejected = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = &event {
                let rejected = *rejected;
                AsyncRunner::handle_exec_event(event);
                break rejected;
            }

            AsyncRunner::handle_exec_event(event);
        };

        native_wait(&mut output, || {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        assert_eq!(rejected.causation_id, Some(command_id));
        assert_eq!(rejected.reason.as_str(), "Invalid parameters");
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().price(),
            Some(Price::from("0.500000"))
        );
        *peer.changes.mode.lock().unwrap() = "success".into();
        peer.changes.ack_gate.add_permits(1);
        let current = harness.cache().borrow().order(&id).unwrap().clone();
        native_modify(&harness, &current, None, Some(Price::from("0.400000")));
        let second = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.400000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(second.len(), 1);
        assert!(matches!(second[0], OrderEventAny::Updated(_)));
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 2);
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::busy(false)]
#[case::stale_identity(true)]
#[tokio::test]
async fn native_cancel_local_rejection_preserves_the_inflight_amendment(#[case] stale: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-change-local", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        let id = order.client_order_id();
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let cached = harness.cache().borrow().order(&id).unwrap().clone();

        let command = ModifyOrder::new(
            cached.trader_id(),
            Some(harness.client_id()),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            cached.venue_order_id(),
            None,
            Some(Price::from("0.500000")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        *peer.changes.mode.lock().unwrap() = "success".into();
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        if !stale {
            let mut pending = OrderPendingUpdate::new(
                cached.trader_id(),
                cached.strategy_id(),
                cached.instrument_id(),
                id,
                Some(harness.account_id()),
                UUID4::new(),
                now,
                now,
                false,
                cached.venue_order_id(),
            );

            pending.causation_id = Some(command.command_id);
            AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::PendingUpdate(
                pending,
            )));
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .modify_order(command.clone())
                .unwrap();
            peer.changes.written.notified().await;
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .modify_order(command.clone())
                .unwrap();
        }

        let command_id = UUID4::new();

        let mut pending = OrderPendingCancel::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            Some(harness.account_id()),
            UUID4::new(),
            now,
            now,
            false,
            cached.venue_order_id(),
        );

        pending.causation_id = Some(command_id);
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::PendingCancel(
            pending,
        )));

        let cancel = CancelOrder::new(
            cached.trader_id(),
            Some(harness.client_id()),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            if stale {
                Some("old-leg".into())
            } else {
                cached.venue_order_id()
            },
            command_id,
            UnixNanos::default(),
            None,
            None,
        );

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .cancel_order(cancel)
            .unwrap();

        let mut updates = Vec::new();

        let rejected = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(OrderEventAny::CancelDenied(rejected)) = event {
                break rejected;
            }

            if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = &event {
                updates.push(OrderEventAny::Updated(*updated));
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert_eq!(rejected.causation_id, Some(command_id));
        assert_eq!(rejected.client_order_id, id);
        assert_eq!(rejected.venue_order_id, cached.venue_order_id());
        assert_eq!(rejected.account_id, Some(harness.account_id()));
        assert_eq!(
            rejected.reason.as_str(),
            if stale {
                "VALIDATION_FAILED: Kalshi change venue identity mismatch"
            } else {
                "VALIDATION_FAILED: Kalshi order change is already in flight"
            }
        );
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::CancelDenied(
            rejected,
        )));
        assert_eq!(
            native_status(&harness, id),
            if stale {
                OrderStatus::Accepted
            } else {
                OrderStatus::PendingUpdate
            }
        );

        if stale {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .modify_order(command.clone())
                .unwrap();
            peer.changes.written.notified().await;
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .modify_order(command.clone())
                .unwrap();
        }

        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        source["yes_price_dollars"] = "0.5000".into();
        source["no_price_dollars"] = "0.5000".into();
        source["last_update_time"] = jiff::Timestamp::now().to_string().into();
        *peer.orders.lock().unwrap() = json!({"orders":[source],"cursor":""});
        peer.changes.ack_gate.add_permits(1);

        let events = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.500000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        updates.extend(events);
        assert_eq!(updates.len(), 1);
        assert!(matches!(updates[0], OrderEventAny::Updated(_)));
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("partial")]
#[case("partial-ahead")]
#[case("partial-behind")]
#[tokio::test]
async fn native_decrease_partial_result_uses_acknowledged_authoritative_terms(#[case] mode: &str) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "native-partial-decrease", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        let id = order.client_order_id();
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        native_dispatch(native_reconnect(&harness, &mut output).await);
        native_wait(&mut output, || {
            let last = harness
                .cache()
                .borrow()
                .order(&id)
                .unwrap()
                .ts_last()
                .as_u64();
            nautilus_core::time::get_atomic_clock_realtime()
                .get_time_ns()
                .as_u64()
                > last + 2_000_000
        })
        .await;

        *peer.changes.mode.lock().unwrap() = mode.into();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        native_modify(&harness, &cached, Some(Quantity::from("0.50")), None);
        peer.changes.written.notified().await;
        peer.changes.ack_gate.add_permits(1);

        let events = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().quantity() == Quantity::from("1.00")
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(events.len(), 1);

        let OrderEventAny::Updated(updated) = &events[0] else {
            panic!("expected the actual decrease")
        };

        assert_eq!(updated.quantity, Quantity::from("1.00"));
        assert_eq!(updated.price, Some(Price::from("0.600000")));
        assert_eq!(updated.client_order_id, id);
        assert_eq!(updated.venue_order_id, Some(VenueOrderId::from("native-1")));
        assert_eq!(updated.account_id, Some(harness.account_id()));
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::active(false)]
#[case::closed_during_recovery(true)]
#[tokio::test]
async fn native_change_preparation_waits_for_recovered_account(#[case] closed: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "preparation-recovery", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        native_disconnect(&harness).await;
        native_dispatch(native_reconnect(&harness, &mut output).await);
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let session = harness
            .cache()
            .borrow()
            .account(&harness.account_id())
            .unwrap()
            .last_event()
            .unwrap()
            .info;
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let command_id = native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        peer.market_read.notified().await;
        let notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_native"]
                .clone();
        *peer.notification.lock().unwrap() = Some(notification);
        peer.notification_ready.notify_one();

        let account = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Account(account) = &event
                && account.info != session
            {
                break event;
            }

            AsyncRunner::handle_exec_event(event);
        };

        if closed {
            peer.market.lock().unwrap()["status"] = "inactive".into();
        }

        drop(blocked);

        let next_account = loop {
            tokio::select! {
                () = peer.changes.written.notified() => {
                    panic!("amend preparation dispatched before account observation");
                }
                event = output.recv() => {
                    let event = event.unwrap();
                    if matches!(event, ExecutionEvent::Account(_)) {
                        break event;
                    }
                    assert!(!matches!(event, ExecutionEvent::Order(_)), "{event:?}");
                    AsyncRunner::handle_exec_event(event);
                }
            }
        };

        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        native_dispatch([account, next_account]);

        if closed {
            let rejected = loop {
                let _ = harness.exec_engine().borrow().get_client(&harness.client_id()).unwrap().is_connected();
                let event = tokio::select! {
                    () = peer.changes.written.notified() => panic!("stale market preparation reached the venue"),
                    event = output.recv() => event.unwrap(),
                };

                if let ExecutionEvent::Order(OrderEventAny::ModifyDenied(rejected)) = &event {
                    let rejected = *rejected;
                    AsyncRunner::handle_exec_event(event);
                    break rejected;
                }

                AsyncRunner::handle_exec_event(event);
            };

            assert_eq!(rejected.causation_id, Some(command_id));
            assert_eq!(rejected.reason.as_str(), "VALIDATION_FAILED: Kalshi market is not open for trading");
            assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
            assert_eq!(harness.cache().borrow().order(&id).unwrap().price(), cached.price());
            native_disconnect(&harness).await;
            return;
        }

        let events = native_wait(&mut output, || {
            let connected = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.500000"))
                && connected
        })
        .await;

        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], OrderEventAny::Updated(_)));
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        let current = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(current.quantity(), cached.quantity());
        assert_eq!(current.filled_qty(), cached.filled_qty());
        assert_eq!(current.venue_order_id(), cached.venue_order_id());
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_change_preparation_rejects_newer_order_terms() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "change-preparation", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        native_disconnect(&harness).await;
        native_dispatch(native_reconnect(&harness, &mut output).await);
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let command_id = native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        peer.market_read.notified().await;
        {
            let mut rows = peer.orders.lock().unwrap();
            rows["orders"][0]["yes_price_dollars"] = "0.4000".into();
            rows["orders"][0]["no_price_dollars"] = "0.6000".into();
            rows["orders"][0]["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }

        native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.400000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        drop(blocked);

        let rejection = loop {
            let event = output.recv().await.unwrap();
            if let ExecutionEvent::Order(OrderEventAny::ModifyDenied(rejected)) = &event {
                let rejected = *rejected;
                AsyncRunner::handle_exec_event(event);
                break rejected;
            }

            AsyncRunner::handle_exec_event(event);
        };

        assert_eq!(rejection.causation_id, Some(command_id));
        assert_eq!(rejection.client_order_id, id);
        assert_eq!(
            rejection.reason.as_str(),
            "VALIDATION_FAILED: Kalshi order changed before admission"
        );
        assert_eq!(
            harness.cache().borrow().order(&id).unwrap().price(),
            Some(Price::from("0.400000"))
        );
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::success(false)]
#[case::venue_rejection(true)]
#[tokio::test]
async fn native_change_completed_commands_do_not_replay(#[case] rejected: bool) {
    tokio::time::timeout(Duration::from_secs(22), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "change-replay", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let cached = harness.cache().borrow().order(&id).unwrap().clone();

        let command = ModifyOrder::new(
            cached.trader_id(),
            Some(harness.client_id()),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            cached.venue_order_id(),
            None,
            Some(Price::from("0.500000")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        if rejected {
            *peer.changes.mode.lock().unwrap() = "reject".into();
        }

        peer.changes.ack_gate.add_permits(1);
        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .modify_order(command.clone())
            .unwrap();
        peer.changes.written.notified().await;

        if rejected {
            loop {
                let event = output.recv().await.unwrap();
                let done = matches!(
                    &event,
                    ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))
                );
                AsyncRunner::handle_exec_event(event);

                if done {
                    break;
                }
            }
        }

        native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price()
                == Some(Price::from(if rejected { "0.600000" } else { "0.500000" }))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        native_disconnect(&harness).await;
        native_dispatch(native_reconnect(&harness, &mut output).await);
        *peer.changes.mode.lock().unwrap() = "success".into();
        peer.changes.ack_gate.add_permits(2);
        native_modify(&harness, &cached, None, Some(Price::from("0.400000")));
        native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.400000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .modify_order(command)
            .unwrap();
        native_modify(&harness, &cached, None, Some(Price::from("0.300000")));
        let events = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.300000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Updated(_)));
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 3);
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
#[allow(
    clippy::await_holding_refcell_ref,
    reason = "connect consumes accounts and queues order events until the client borrow ends"
)]
async fn native_connect_preserves_an_active_mutation() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "connect-mutation", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let subscriptions = peer.subscriptions.lock().unwrap().clone();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        native_modify(&harness, &cached, None, Some(Price::from("0.500000")));
        peer.changes.written.notified().await;
        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );

        let queued = {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            connect_consuming_accounts(client.as_mut(), &mut output).await
        };

        native_dispatch(queued);
        assert_eq!(*peer.subscriptions.lock().unwrap(), subscriptions);

        peer.changes.ack_gate.add_permits(1);
        native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().price() == Some(Price::from("0.500000"))
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::mapped("mapped", false)]
#[case::unknown_status("status", false)]
#[case::unknown_type("type", false)]
#[case::unknown_both("both", false)]
#[case::unknown_canceled_status("status", true)]
#[tokio::test]
async fn native_unknown_cancel_publishes_fills_before_resolution(
    #[case] classification: &str,
    #[case] zero_leaves: bool,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "unknown-cancel-fill", side, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        *peer.changes.mode.lock().unwrap() = "unknown".into();
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        native_cancel(&harness, &cached);
        peer.changes.written.notified().await;
        peer.changes.ack_gate.add_permits(1);
        native_disconnect(&harness).await;
        native_dispatch(native_reconnect(&harness, &mut output).await);
        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let cash = native_fill(&peer, "1.00", false);

        {
            let mut rows = peer.orders.lock().unwrap();
            let source = &mut rows["orders"][0];
            if matches!(classification, "status" | "both") {
                source["status"] = "new_status".into();
            }

            if matches!(classification, "type" | "both") {
                source["type"] = "new_type".into();
            }

            if zero_leaves {
                source["remaining_count_fp"] = "0.00".into();
            }
        }

        let events = native_wait(&mut output, || {
            harness.cache().borrow().order(&id).unwrap().filled_qty() == Quantity::from("1.00")
                && native_balance(&harness).total.as_decimal() == cash
        })
        .await;

        let current = harness.cache().borrow().order(&id).unwrap().clone();
        assert_eq!(current.status(), OrderStatus::PartiallyFilled);
        assert_eq!(current.quantity(), Quantity::from("1.25"));
        assert_eq!(current.leaves_qty(), Quantity::from("0.25"));
        assert_eq!(current.price(), Some(Price::from("0.600000")));
        let balance = native_balance(&harness);
        assert_eq!(
            balance.total.as_decimal(),
            dec(if side == OrderSide::Buy {
                "0.400000"
            } else {
                "0.600000"
            })
        );
        assert_eq!(
            balance.locked.as_decimal(),
            dec(match (classification, side) {
                ("mapped", OrderSide::Buy) => "0.156900",
                ("mapped", OrderSide::Sell) => "0.106900",
                (_, OrderSide::Buy) => "0.184400",
                (_, OrderSide::Sell) => "0.134400",
            })
        );
        assert_eq!(
            balance.free.as_decimal(),
            dec(match (classification, side) {
                ("mapped", OrderSide::Buy) => "0.243100",
                ("mapped", OrderSide::Sell) => "0.493100",
                (_, OrderSide::Buy) => "0.215600",
                (_, OrderSide::Sell) => "0.465600",
            })
        );
        assert_eq!(events.len(), 1);

        let OrderEventAny::Filled(fill) = &events[0] else {
            panic!("expected owned fill");
        };

        assert_eq!(fill.trader_id, cached.trader_id());
        assert_eq!(fill.strategy_id, cached.strategy_id());
        assert_eq!(fill.instrument_id, order.instrument_id());
        assert_eq!(fill.client_order_id, id);
        assert_eq!(Some(fill.venue_order_id), cached.venue_order_id());
        assert_eq!(fill.account_id, harness.account_id());
        assert_eq!(
            fill.trade_id,
            TradeId::from(
                peer.fills.lock().unwrap()["fills"][0]["fill_id"]
                    .as_str()
                    .unwrap()
            )
        );
        assert_eq!(fill.order_type, OrderType::Limit);
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.last_qty, Quantity::from("1.00"));
        assert_eq!(fill.last_px, Price::from("0.600000"));
        assert_eq!(fill.currency, Currency::USD());
        assert_eq!(
            fill.commission,
            Some(Money::from_decimal_exact(dec("0.000100"), Currency::USD()).unwrap())
        );
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill.position_id, None);
        assert!(!fill.reconciliation);
        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let reads = peer.balance_reads.load(Ordering::SeqCst);
        let replay = native_wait(&mut output, || {
            peer.balance_reads.load(Ordering::SeqCst) >= reads + 3
        })
        .await;

        assert!(replay.is_empty());
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(
            harness
                .cache()
                .borrow()
                .positions_open_refs(
                    None,
                    Some(&order.instrument_id()),
                    None,
                    Some(&harness.account_id()),
                    None
                )
                .iter()
                .map(|p| p.signed_decimal_qty())
                .sum::<Decimal>(),
            dec(if side == OrderSide::Buy {
                "1.00"
            } else {
                "-1.00"
            })
        );
        {
            let mut rows = peer.orders.lock().unwrap();
            rows["orders"][0]["type"] = "limit".into();
            rows["orders"][0]["status"] = "canceled".into();
            rows["orders"][0]["remaining_count_fp"] = "0.00".into();
            rows["orders"][0]["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }

        let terminal = native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(terminal.len(), 1);
        assert!(matches!(terminal[0], OrderEventAny::Canceled(_)));
        assert_eq!(native_balance(&harness).total.as_decimal(), cash);
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::queued(false)]
#[case::preparing(true)]
#[tokio::test]
async fn native_change_shutdown_before_dispatch_rejects_without_a_write(#[case] preparing: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.submissions.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "change-shutdown", OrderSide::Buy, false);
        let id = order.client_order_id();
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, id) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        native_dispatch(native_reconnect(&harness, &mut output).await);
        let cached = harness.cache().borrow().order(&id).unwrap().clone();
        let balance = native_balance(&harness);
        let blocked = Arc::clone(&peer.market_gate).acquire_owned().await.unwrap();
        let _ = peer.market_read.notified().now_or_never();
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        let command_id = native_modify(&harness, &cached, None, Some(Price::from("0.500000")));

        let mut pending = OrderPendingUpdate::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            id,
            cached.account_id(),
            UUID4::new(),
            now,
            now,
            false,
            cached.venue_order_id(),
        );

        pending.causation_id = Some(command_id);
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::PendingUpdate(
            pending,
        )));

        if preparing {
            peer.market_read.notified().await;
        }

        native_disconnect(&harness).await;
        let events = native_dispatch(std::iter::from_fn(|| output.try_recv().ok()));
        assert_eq!(events.len(), 1);

        let OrderEventAny::ModifyDenied(rejected) = &events[0] else {
            panic!("expected ModifyDenied")
        };

        assert_eq!(rejected.trader_id, cached.trader_id());
        assert_eq!(rejected.strategy_id, cached.strategy_id());
        assert_eq!(rejected.instrument_id, cached.instrument_id());
        assert_eq!(rejected.client_order_id, id);
        assert_eq!(rejected.venue_order_id, cached.venue_order_id());
        assert_eq!(rejected.account_id, cached.account_id());
        assert_eq!(rejected.causation_id, Some(command_id));
        assert_eq!(
            rejected.reason.as_str(),
            "VALIDATION_FAILED: Kalshi change canceled before dispatch"
        );
        assert_eq!(rejected.ts_event, cached.ts_last());
        assert_eq!(native_status(&harness, id), OrderStatus::Accepted);
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        drop(blocked);
        let replay = native_dispatch(native_reconnect(&harness, &mut output).await);
        assert!(replay.is_empty());
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::submit("submit")]
#[case::amend("amend")]
#[case::decrease("decrease")]
#[case::cancel("cancel")]
#[tokio::test]
async fn single_write_rechecks_connection_after_admitted_discovery(#[case] operation: &str) {
    tokio::time::timeout(Duration::from_secs(45), async {
        let peer = Peer::with_timeout(None, 15).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "3.0001".into();
        peer.limits.lock().unwrap()["read"]["refill_rate"] = 10000.into();
        peer.limits.lock().unwrap()["read"]["bucket_capacity"] = 20000.into();
        peer.costs.lock().unwrap()["default_cost"] = 23.into();
        peer.limits.lock().unwrap()["write"]["refill_rate"] = 1.into();
        peer.limits.lock().unwrap()["write"]["bucket_capacity"] = 23.into();
        peer.submissions.ack_gate.add_permits(2);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "dispatch-original", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                && harness.exec_engine().borrow().get_client(&harness.client_id()).unwrap().is_connected()
        }).await;

        let cached = harness.cache().borrow().order(&order.client_order_id()).unwrap().clone();
        let _ = peer.submissions.written.notified().now_or_never();

        let transport = KalshiDataClientConfig {
            api_key_id: peer.config.api_key_id.clone(),
            private_key: peer.config.private_key.clone(),
            base_url_http: peer.config.base_url_http.clone(),
            http_timeout_secs: 15,
            ..Default::default()
        };

        let http = KalshiHttpClient::new_authenticated(&transport).unwrap();
        peer.limits.lock().unwrap()["grants"][0]["expires_ts"] =
            (jiff::Timestamp::now().as_second() + 3).into();
        http.get_account_limits().await.unwrap();
        peer.limits.lock().unwrap()["grants"][0]["expires_ts"] = Value::Null;
        let _ = peer.limits_read.notified().now_or_never();
        let discovery = Arc::clone(&peer.limits_gate).acquire_owned().await.unwrap();
        let probe = native_order(&harness, "dispatch-probe", OrderSide::Buy, false);
        let mut command_id = None;

        if operation == "submit" {
            native_submit(&harness, &probe, None);
        } else if operation == "amend" {
            command_id = Some(native_modify(&harness, &cached, None, Some(Price::from("0.500000"))));
        }

        if operation == "decrease" {
            command_id = Some(native_modify(&harness, &cached, Some(Quantity::from("1.00")), None));
        } else if operation == "cancel" {
            command_id = Some(native_cancel(&harness, &cached));
        }

        if operation == "submit" {
            native_wait(&mut output, || native_status(&harness, probe.client_order_id()) == OrderStatus::Submitted).await;
        } else {
            wait_until_async(|| async {
                !harness.exec_engine().borrow().get_client(&harness.client_id()).unwrap().is_connected()
            }, Duration::from_secs(3)).await;
        }

        peer.limits_read.notified().await;
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        let subscriptions = peer.subscriptions.lock().unwrap().len();
        *peer.notification.lock().unwrap() = Some(json!({"type":"disconnect"}));
        peer.notification_ready.notify_one();
        wait_until_async(
            || async { peer.notification.lock().unwrap().is_none() },
            Duration::from_secs(2),
        )
        .await;
        {
            let mut engine = harness.exec_engine().borrow_mut();
            let client = engine.get_client_adapter_mut(&harness.client_id()).unwrap();
            client.stop().unwrap();
        }

        drop(discovery);
        let mut recovery = native_reconnect(&harness, &mut output).await.into_iter();
        assert!(peer.subscriptions.lock().unwrap().len() > subscriptions);

        let outcome = loop {
            let event = if let Some(event) = recovery.next() {
                event
            } else {
                tokio::select! {
                    () = peer.submissions.written.notified() => panic!("Invalidated submission reached the venue"),
                    () = peer.changes.written.notified() => panic!("Invalidated change reached the venue"),
                    event = output.recv() => event.unwrap(),
                }
            };

            let outcome = match &event {
                ExecutionEvent::Order(event) => match event {
                    OrderEventAny::Rejected(_) if operation == "submit" => Some(event.clone()),
                    OrderEventAny::ModifyDenied(_) if matches!(operation, "amend" | "decrease") => Some(event.clone()),
                    OrderEventAny::CancelDenied(_) if operation == "cancel" => Some(event.clone()),
                    _ => None,
                },
                _ => None,
            };

            AsyncRunner::handle_exec_event(event);

            if let Some(outcome) = outcome { break outcome; }
        };

        assert_eq!(outcome.client_order_id(), if operation == "submit" { probe.client_order_id() } else { cached.client_order_id() });

        let outcome = outcome.into_boxed();
        assert_eq!(outcome.reason().unwrap().as_str(), if operation == "submit" { "Order was not sent" } else { "VALIDATION_FAILED: Order was not sent" });

        if let Some(command_id) = command_id {
            assert_eq!(outcome.causation_id(), Some(command_id));
        }

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 0);
        native_disconnect(&harness).await;
    }).await.unwrap();
}

struct BatchPeer {
    writes: Mutex<Vec<(String, Value)>>,
    mode: Mutex<String>,
    written: tokio::sync::Notify,
    ack_gate: Arc<tokio::sync::Semaphore>,
}

impl BatchPeer {
    async fn respond(
        &self,
        mut stream: tokio::net::TcpStream,
        headers: &str,
        fixtures: &Value,
        orders: &Mutex<Value>,
        subaccount: u32,
    ) {
        let target = headers.lines().next().unwrap().to_owned();

        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|value| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();

        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();

        let sequence = {
            let mut writes = self.writes.lock().unwrap();
            writes.push((target.clone(), request.clone()));
            writes.len()
        };

        let mode = self.mode.lock().unwrap().clone();
        let mut results = Vec::new();

        for (index, child) in request["orders"].as_array().unwrap().iter().enumerate() {
            assert_eq!(child["exchange_index"], 3);
            assert_eq!(child["subaccount"], subaccount);

            let result = if target.starts_with("POST ") {
                Self::create(child, fixtures, orders, &mode, sequence, index)
            } else {
                assert!(target.starts_with("DELETE "));
                Self::cancel(child, orders, &mode, index)
            };

            results.push(result);
        }

        match mode.as_str() {
            "reordered" => results.reverse(),
            "missing" => {
                results.pop();
            }
            "duplicate" => {
                results[1] = results[0].clone();
            }
            "venue-duplicate" => {
                results[0]["order_id"] = results[1]["order_id"].clone();
            }
            _ => {}
        }

        let (status, body) = if mode == "whole-reject" {
            (400, fixtures["reject_batch"].to_string())
        } else if matches!(mode.as_str(), "lost" | "unknown" | "partial-unknown") {
            (503, fixtures["error"].to_string())
        } else {
            (200, json!({"orders":results}).to_string())
        };

        self.written.notify_one();
        let gate = Arc::clone(&self.ack_gate);
        tokio::spawn(async move {
            let permit = gate.acquire().await.unwrap();
            permit.forget();
            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }

    fn create(
        child: &Value,
        fixtures: &Value,
        orders: &Mutex<Value>,
        mode: &str,
        sequence: usize,
        index: usize,
    ) -> Value {
        if matches!(
            mode,
            "permission_denied" | "duplicate_client_order_id" | "not_found" | "error-identity"
        ) {
            let code = if mode == "error-identity" {
                "post_only_cross"
            } else {
                mode
            };

            let mut result =
                json!({"client_order_id":child["client_order_id"], "error":{"code":code}});

            if mode == "error-identity" {
                result["order_id"] = "unconfirmed-order".into();
            }

            return result;
        }

        if index == 1 {
            match mode {
                "reject" => {
                    return json!({"client_order_id":child["client_order_id"],"error":{"code":"post_only_cross","message":"Would cross"}});
                }
                "reject-details" => {
                    return json!({"client_order_id":child["client_order_id"],"error":fixtures["reject_post_only"],"ts_ms":null});
                }
                "reject-invalid" => {
                    let mut error = fixtures["reject_post_only"].clone();
                    error["details"] = "invalid quantity".into();
                    return json!({"client_order_id":child["client_order_id"],"error":error,"ts_ms":null});
                }
                _ => {}
            }
        }

        SubmissionPeer::create(
            child,
            fixtures,
            orders,
            if matches!(mode, "unknown" | "whole-reject") || mode == "partial-unknown" && index > 0
            {
                "unknown"
            } else {
                "resting"
            },
            &format!("batch-{sequence}-{index}"),
        )
    }

    fn cancel(child: &Value, orders: &Mutex<Value>, mode: &str, index: usize) -> Value {
        let mut rows = orders.lock().unwrap();
        let source = rows["orders"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["order_id"] == child["order_id"])
            .unwrap();
        let now = MutationPeer::timestamp(source, "success");
        let mut result = json!({"order_id":child["order_id"],"client_order_id":source["client_order_id"],"reduced_by":source["remaining_count_fp"],"ts_ms":now.as_millisecond()});

        if matches!(
            mode,
            "permission_denied" | "duplicate_client_order_id" | "not_found"
        ) {
            return json!({"order_id":child["order_id"], "client_order_id":null, "ts_ms":null, "reduced_by":"0.00", "error":{"code":mode}});
        }

        if matches!(mode, "reject" | "reject-null") && index == 1 {
            if mode == "reject-null" {
                result["client_order_id"] = Value::Null;
            }

            result["reduced_by"] = "0.00".into();
            result.as_object_mut().unwrap().remove("ts_ms");
            result["error"] =
                serde_json::from_str::<Value>(include_str!("../../test_data/rest_responses.json"))
                    .unwrap()["reject_cancel"]
                    .clone();
        } else if !matches!(mode, "unknown" | "whole-reject")
            && (mode != "partial-unknown" || index == 0)
        {
            source["remaining_count_fp"] = "0.00".into();
            source["status"] = "canceled".into();
            source["last_update_time"] = now.to_string().into();
        }

        result
    }
}

fn native_list(harness: &ExecutionHarness, orders: &[OrderAny]) {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::{events::OrderInitialized, orders::OrderList};

    let mut children = orders.to_vec();
    for child in &mut children {
        child.set_order_list_id("BATCH-LIST".into());
    }

    let orders = children.as_slice();

    let list = OrderList::new(
        "BATCH-LIST".into(),
        orders[0].instrument_id(),
        orders[0].strategy_id(),
        orders.iter().map(Order::client_order_id).collect(),
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        harness.trader_id(),
        Some(harness.client_id()),
        orders[0].strategy_id(),
        list,
        orders.iter().map(OrderInitialized::from).collect(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    for order in orders {
        harness
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(harness.client_id()), false)
            .unwrap();
    }

    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .submit_order_list(cmd)
        .unwrap();
}

fn native_cancel_batch(harness: &ExecutionHarness, orders: &[OrderAny]) -> BatchCancelOrders {
    let cancels = orders
        .iter()
        .map(|order| {
            CancelOrder::new(
                order.trader_id(),
                Some(harness.client_id()),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.venue_order_id(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect();

    let cmd = BatchCancelOrders::new(
        harness.trader_id(),
        Some(harness.client_id()),
        orders[0].strategy_id(),
        orders[0].instrument_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    for (index, order) in orders.iter().enumerate() {
        let cached = harness
            .cache()
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .clone();

        if cached.is_closed() {
            continue;
        }

        let mut pending = OrderPendingCancel::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            cached.client_order_id(),
            cached.account_id(),
            UUID4::new(),
            cached.ts_last(),
            cached.ts_last(),
            false,
            cached.venue_order_id(),
        );

        pending.causation_id = Some(cmd.cancels[index].command_id);
        AsyncRunner::handle_exec_event(ExecutionEvent::Order(OrderEventAny::PendingCancel(
            pending,
        )));
    }

    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .batch_cancel_orders(cmd.clone())
        .unwrap();
    cmd
}

#[rstest]
#[case("success", 0)]
#[case("reordered", 0)]
#[case("missing", 0)]
#[case("duplicate", 0)]
#[case("lost", 0)]
#[case("venue-duplicate", 0)]
#[case("reject", 0)]
#[case("reject-details", 0)]
#[case("reject-invalid", 0)]
#[case("reordered", 7)]
#[tokio::test]
async fn native_batch_list_correlates_each_child_and_recovers(
    #[case] mode: &str,
    #[case] subaccount: u32,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::with_scope(None, 2, subaccount).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "10.0001".into();
        *peer.batches.mode.lock().unwrap() = mode.into();
        peer.batches.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..3)
            .map(|i| {
                native_order_builder(&harness, &format!("list-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from(format!("0.{}", i + 1).as_str()))
                    .post_only(true)
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        let events = native_wait(&mut output, || {
            orders.iter().enumerate().all(|(i, order)| {
                native_status(&harness, order.client_order_id())
                    == if mode.starts_with("reject") && i == 1 {
                        OrderStatus::Rejected
                    } else {
                        OrderStatus::Accepted
                    }
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        assert!(
            writes[0]
                .0
                .starts_with("POST /portfolio/events/orders/batched ")
        );
        assert_eq!(writes[0].1["orders"].as_array().unwrap().len(), 3);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
                .count(),
            3
        );

        let rejections: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Rejected(event) => Some(event),
                _ => None,
            })
            .collect();

        let expected = match mode {
            "reject-invalid" => ("Invalid order", false),
            _ => ("Post-only order would cross", true),
        };

        assert_eq!(rejections.len(), usize::from(mode.starts_with("reject")));

        for rejection in rejections {
            assert_eq!(rejection.client_order_id, orders[1].client_order_id());
            assert_eq!(rejection.reason.as_str(), expected.0);
            assert_eq!(rejection.due_post_only, expected.1);
        }

        for (i, order) in orders.iter().enumerate() {
            let count = ["0.1", "0.2", "0.3"][i];

            let mut expected_request =
                serde_json::from_str::<Value>(include_str!("../../test_data/rest_requests.json"))
                    .unwrap()["native_batch_create"]
                    .clone();
            expected_request["subaccount"] = subaccount.into();
            expected_request["ticker"] = serde_json::json!(order.instrument_id().symbol.as_str());
            expected_request["count"] = serde_json::json!(count);
            expected_request["client_order_id"] =
                serde_json::json!(order.client_order_id().as_str());

            assert_eq!(writes[0].1["orders"][i], expected_request);
            let cache = harness.cache().borrow();
            let cached = cache.order(&order.client_order_id()).unwrap();
            assert_eq!(cached.quantity(), order.quantity());
            assert_eq!(cached.price(), order.price());
            assert!(cached.is_post_only());
            assert_eq!(cached.filled_qty(), Quantity::from("0.00"));

            if mode.starts_with("reject") && i == 1 {
                assert_eq!(cached.venue_order_id(), None);
            } else {
                assert_eq!(
                    cached.venue_order_id(),
                    Some(VenueOrderId::from(format!("batch-1-{i}").as_str()))
                );
            }
        }

        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[tokio::test]
async fn native_batch_cancel_skips_closed_children(#[values(false, true)] closed_first: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.batches.ack_gate.add_permits(2);
        peer.changes.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..2)
            .map(|i| {
                native_order_builder(&harness, &format!("mixed-cancel-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let closed_index = usize::from(!closed_first);
        let closed_id = orders[closed_index].client_order_id();
        let open_id = orders[1 - closed_index].client_order_id();
        let accepted = harness.cache().borrow().order(&closed_id).unwrap().clone();
        native_cancel(&harness, &accepted);
        native_wait(&mut output, || {
            native_status(&harness, closed_id) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let closed = harness.cache().borrow().order(&closed_id).unwrap().clone();
        native_cancel_batch(&harness, &orders);
        let events = native_wait(&mut output, || {
            native_status(&harness, open_id) == OrderStatus::Canceled
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], OrderEventAny::Canceled(event) if event.client_order_id == open_id)
        );
        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert!(
            writes[1]
                .0
                .starts_with("DELETE /portfolio/events/orders/batched ")
        );
        assert_eq!(
            writes[1].1,
            json!({"orders": [{
                "order_id": format!("batch-1-{}", 1 - closed_index),
                "subaccount": 0,
                "exchange_index": 3,
                "market_ticker": harness.instrument_id().symbol.as_str()
            }]})
        );
        assert_eq!(peer.changes.writes.lock().unwrap().len(), 1);
        let cached_closed = harness.cache().borrow().order(&closed_id).unwrap().clone();
        assert_eq!(cached_closed.events(), closed.events());
        let cached_open = harness.cache().borrow().order(&open_id).unwrap().clone();
        assert_eq!(cached_open.status(), OrderStatus::Canceled);
        assert!(!cached_open.has_pending_cancel_command());
        assert_eq!(native_balance(&harness).total.as_decimal(), dec("1.0001"));
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(native_balance(&harness).free.as_decimal(), dec("1.0001"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("success", 0)]
#[case("reordered", 0)]
#[case("missing", 0)]
#[case("duplicate", 0)]
#[case("lost", 0)]
#[case("reject", 0)]
#[case("reject-null", 0)]
#[case("reordered", 7)]
#[tokio::test]
async fn native_batch_cancel_correlates_each_child_and_recovers(
    #[case] mode: &str,
    #[case] subaccount: u32,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let peer = Peer::with_scope(None, 2, subaccount).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "10.0001".into();
        peer.batches.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..3)
            .map(|i| {
                native_order_builder(&harness, &format!("cancel-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from(format!("0.{}", i + 1).as_str()))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        *peer.batches.mode.lock().unwrap() = mode.into();
        native_cancel_batch(&harness, &orders);
        let events = native_wait(&mut output, || {
            orders.iter().enumerate().all(|(i, order)| {
                native_status(&harness, order.client_order_id())
                    == if matches!(mode, "reject" | "reject-null") && i == 1 {
                        OrderStatus::Accepted
                    } else {
                        OrderStatus::Canceled
                    }
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert!(
            writes[1]
                .0
                .starts_with("DELETE /portfolio/events/orders/batched ")
        );

        for (i, child) in writes[1].1["orders"].as_array().unwrap().iter().enumerate() {
            assert_eq!(child["order_id"], format!("batch-1-{i}"));
            assert_eq!(child["subaccount"], subaccount);
            assert_eq!(child["exchange_index"], 3);
            assert_eq!(
                child["market_ticker"],
                harness.instrument_id().symbol.as_str()
            );
        }

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            if matches!(mode, "reject" | "reject-null") {
                2
            } else {
                3
            }
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::CancelRejected(_)))
                .count(),
            usize::from(matches!(mode, "reject" | "reject-null"))
        );
        assert_eq!(
            native_balance(&harness).locked.as_decimal(),
            if matches!(mode, "reject" | "reject-null") {
                dec("0.12552")
            } else {
                Decimal::ZERO
            }
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_batch_chunks_by_weighted_capacity() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "10.0001".into();
        peer.limits.lock().unwrap()["write"]["bucket_capacity"] = 33.into();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let costs = [
            fixtures["batch_create_cost"].clone(),
            fixtures["batch_cancel_cost"].clone(),
        ];

        peer.costs.lock().unwrap()["endpoint_costs"]
            .as_array_mut()
            .unwrap()
            .extend(costs);
        peer.batches.ack_gate.add_permits(6);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..5)
            .map(|i| {
                native_order_builder(&harness, &format!("chunk-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && ready
        })
        .await;

        native_cancel_batch(&harness, &orders);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Canceled
            }) && ready
        })
        .await;

        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(
            writes
                .iter()
                .map(|(_, body)| body["orders"].as_array().unwrap().len())
                .collect::<Vec<_>>(),
            vec![3, 2, 2, 2, 1]
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

fn native_cancel_all(
    harness: &ExecutionHarness,
    strategy: StrategyId,
    side: Option<OrderSide>,
) -> CancelAllOrders {
    let cmd = CancelAllOrders::new(
        harness.trader_id(),
        Some(harness.client_id()),
        strategy,
        harness.instrument_id(),
        side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    harness
        .exec_engine()
        .borrow()
        .execute(TradingCommand::CancelAllOrders(cmd.clone()));
    cmd
}

#[tokio::test]
async fn native_batch_cancel_all_preserves_scoped_peers() {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "20.0001".into();
        peer.batches.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = [OrderSide::Buy, OrderSide::Sell, OrderSide::Buy]
            .into_iter()
            .enumerate()
            .map(|(i, side)| {
                native_order_builder(&harness, &format!("scope-{i}"), side)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let decoys = native_scope_peers(&harness, orders[0].strategy_id());

        let mut foreign = peer.orders.lock().unwrap()["orders"][0].clone();
        foreign["order_id"] = "external-owned-elsewhere".into();
        foreign["client_order_id"] = "external-client".into();
        peer.orders.lock().unwrap()["orders"]
            .as_array_mut()
            .unwrap()
            .push(foreign.clone());
        let mut shard = foreign.clone();
        shard["order_id"] = "other-shard".into();
        shard["client_order_id"] = "other-shard-client".into();
        shard["exchange_index"] = 2.into();
        *peer.orders_archive.lock().unwrap() = Some(json!({"orders":[shard.clone()],"cursor":""}));
        native_cancel_all(&harness, orders[0].strategy_id(), Some(OrderSide::Buy));
        let events = native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, orders[0].client_order_id()) == OrderStatus::Canceled
                && native_status(&harness, orders[2].client_order_id()) == OrderStatus::Canceled
                && ready
        })
        .await;

        assert_eq!(
            native_status(&harness, orders[1].client_order_id()),
            OrderStatus::Accepted
        );

        for order in decoys {
            assert_eq!(
                harness
                    .cache()
                    .borrow()
                    .order(&order.client_order_id())
                    .unwrap()
                    .clone(),
                order
            );
        }

        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 2);
        assert_eq!(
            writes[1].1["orders"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["order_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["batch-1-0", "batch-1-2"]
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            2
        );
        let rows = peer.orders.lock().unwrap()["orders"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(
            rows.iter()
                .find(|row| row["order_id"] == foreign["order_id"])
                .unwrap(),
            &foreign
        );
        assert_eq!(
            peer.orders_archive.lock().unwrap().as_ref().unwrap()["orders"][0],
            shard
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn native_batch_cancel_all_retains_unacknowledged_submission(#[case] stop: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order_builder(&harness, "pending-scope", OrderSide::Buy)
            .quantity(Quantity::from("0.10"))
            .build();
        native_submit(&harness, &order, None);
        peer.submissions.written.notified().await;
        native_wait(&mut output, || {
            native_status(&harness, order.client_order_id()) == OrderStatus::Submitted
        })
        .await;

        native_cancel_all(&harness, order.strategy_id(), None);
        if stop {
            native_disconnect(&harness).await;
            assert_eq!(peer.batches.writes.lock().unwrap().len(), 0);
            assert_eq!(peer.submissions.writes.lock().unwrap().len(), 1);
            peer.submissions.ack_gate.add_permits(1);
            let replay = native_reconnect(&harness, &mut output).await;
            native_dispatch(replay);
            native_wait(&mut output, || {
                let ready = harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected();
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted && ready
            })
            .await;

            assert_eq!(peer.batches.writes.lock().unwrap().len(), 0);
            native_disconnect(&harness).await;
        } else {
            peer.submissions.ack_gate.add_permits(1);
            native_wait(&mut output, || {
                let ready = harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected();
                native_status(&harness, order.client_order_id()) == OrderStatus::Canceled && ready
            })
            .await;

            assert_eq!(peer.batches.writes.lock().unwrap().len(), 1);
            assert_eq!(
                peer.batches.writes.lock().unwrap()[0].1["orders"][0]["order_id"],
                "native-1"
            );
            native_disconnect(&harness).await;
        }
    })
    .await
    .unwrap();
}

#[rstest]
#[case::validation(false)]
#[case::readiness(true)]
#[tokio::test]
async fn native_batch_denials_preserve_inflight_recovery(#[case] recovering: bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let query = if recovering {
            let seed = native_order_builder(&harness, "denied-seed", OrderSide::Buy)
                .kind(OrderType::Market)
                .build();
            native_submit(&harness, &seed, None);
            native_wait(&mut output, || {
                native_status(&harness, seed.client_order_id()) == OrderStatus::Denied
            })
            .await;

            let query = QueryOrder::new(
                harness.trader_id(),
                Some(harness.client_id()),
                seed.strategy_id(),
                seed.instrument_id(),
                seed.client_order_id(),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            );

            Some(query)
        } else {
            None
        };

        peer.balance_gate
            .forget_permits(peer.balance_gate.available_permits());
        let reads = peer.balance_reads.load(Ordering::SeqCst);
        wait_until_async(
            || async { peer.balance_reads.load(Ordering::SeqCst) > reads },
            Duration::from_secs(4),
        )
        .await;
        let mut reads = peer.balance_reads.load(Ordering::SeqCst);
        native_dispatch(std::iter::from_fn(|| output.try_recv().ok()));

        if let Some(query) = query {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .query_order(query)
                .unwrap();
            peer.balance_gate.add_permits(2);
            wait_until_async(
                || async { peer.balance_reads.load(Ordering::SeqCst) >= reads + 2 },
                Duration::from_secs(4),
            )
            .await;
            reads = peer.balance_reads.load(Ordering::SeqCst);
            assert!(output.try_recv().is_err());
        }

        let balance = native_balance(&harness);

        for attempt in 0..3 {
            let first =
                native_order_builder(&harness, &format!("denied-{attempt}-buy"), OrderSide::Buy)
                    .quantity(Quantity::from("0.01"))
                    .build();
            let mut second =
                native_order_builder(&harness, &format!("denied-{attempt}-sell"), OrderSide::Sell);
            second.quantity(Quantity::from("0.01"));
            if !recovering {
                second.kind(OrderType::Market);
            }

            let orders = [first, second.build()];
            native_list(&harness, &orders);
            let events = native_wait(&mut output, || {
                orders.iter().all(|order| {
                    native_status(&harness, order.client_order_id()) == OrderStatus::Denied
                })
            })
            .await;

            assert_eq!(events.len(), 2);

            for order in &orders {
                let event = events
                    .iter()
                    .find(|event| event.client_order_id() == order.client_order_id())
                    .unwrap();

                let OrderEventAny::Denied(event) = event else {
                    panic!("expected denial");
                };

                assert_eq!(
                    event.reason.as_str(),
                    if recovering {
                        "VALIDATION_FAILED: Kalshi submission requires current-session readiness"
                    } else if event.client_order_id == orders[0].client_order_id() {
                        "VALIDATION_FAILED: Kalshi order-list child failed validation"
                    } else {
                        "UNSUPPORTED_ORDER_TYPE: MARKET"
                    }
                );
            }
        }

        peer.balance_gate.add_permits(2);
        wait_until_async(
            || async { peer.balance_reads.load(Ordering::SeqCst) >= reads + 2 },
            Duration::from_secs(4),
        )
        .await;
        let events: Vec<_> = std::iter::from_fn(|| output.try_recv().ok()).collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ExecutionEvent::Account(_)))
                .count(),
            1
        );
        assert!(native_dispatch(events).is_empty());
        assert!(
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        assert_eq!(native_balance(&harness), balance);
        assert_eq!(peer.batches.writes.lock().unwrap().len(), 0);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        peer.balance_gate
            .add_permits(tokio::sync::Semaphore::MAX_PERMITS);
        peer.batches.ack_gate.add_permits(1);

        let orders = [
            native_order_builder(&harness, "recovered-buy", OrderSide::Buy)
                .quantity(Quantity::from("0.01"))
                .build(),
            native_order_builder(&harness, "recovered-sell", OrderSide::Sell)
                .quantity(Quantity::from("0.01"))
                .build(),
        ];

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        let balance = native_balance(&harness);
        assert_eq!(balance.total.as_decimal(), dec("1.000100"));
        assert_eq!(balance.locked.as_decimal(), dec("0.010552"));
        assert_eq!(balance.free.as_decimal(), dec("0.989548"));
        let writes = peer.batches.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1["orders"][0]["client_order_id"], "recovered-buy");
        assert_eq!(
            writes[0].1["orders"][1]["client_order_id"],
            "recovered-sell"
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::wire(true, false)]
#[case::fallback(false, false)]
#[case::unplanned(true, true)]
#[tokio::test]
async fn native_recovery_rechecks_locally_denied_identities(
    #[case] wire_id: bool,
    #[case] unplanned: bool,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.balance.lock().unwrap()["balance_dollars"] = "5.000100".into();
        resting_bootstrap(&peer, &bootstrap_instrument());
        {
            let mut rows = peer.orders.lock().unwrap();
            rows["orders"][0]["order_id"] = "preload-order".into();
            rows["orders"][0]["remaining_count_fp"] = "0.01".into();
            rows["orders"][0]["initial_count_fp"] = "0.01".into();
        }

        let (harness, mut output, _data) = native_consumer(&peer).await;
        peer.balance_gate
            .forget_permits(peer.balance_gate.available_permits());
        let reads = peer.balance_reads.load(Ordering::SeqCst);
        wait_until_async(
            || async { peer.balance_reads.load(Ordering::SeqCst) > reads },
            Duration::from_secs(4),
        )
        .await;
        let held = peer.series_gate.acquire().await.unwrap();
        let _ = peer.series_read.notified().now_or_never();
        native_dispatch(std::iter::from_fn(|| output.try_recv().ok()));
        {
            let mut rows = peer.orders.lock().unwrap();
            let mut source = rows["orders"][0].clone();
            source["order_id"] = "collision".into();
            source["client_order_id"] = if wire_id { "collision" } else { "" }.into();
            if unplanned {
                source["status"] = "canceled".into();
                source["remaining_count_fp"] = "0.00".into();
                source["last_update_time"] = jiff::Timestamp::now().to_string().into();
                let mut resting = source.clone();
                resting["order_id"] = "later-order".into();
                resting["client_order_id"] = "later-client".into();
                resting["status"] = "resting".into();
                resting["remaining_count_fp"] = "0.01".into();
                rows["orders"].as_array_mut().unwrap().push(resting);
            }

            rows["orders"].as_array_mut().unwrap().push(source);
        }

        peer.balance_gate.add_permits(2);
        peer.series_read.notified().await;
        let collision = native_order_builder(&harness, "collision", OrderSide::Buy)
            .kind(OrderType::Market)
            .build();
        native_submit(&harness, &collision, None);
        native_wait(&mut output, || {
            native_status(&harness, collision.client_order_id()) == OrderStatus::Denied
        })
        .await;

        let reads = peer.balance_reads.load(Ordering::SeqCst);
        drop(held);
        wait_until_async(
            || async { peer.balance_reads.load(Ordering::SeqCst) > reads },
            Duration::from_secs(4),
        )
        .await;
        assert!(output.try_recv().is_err());
        assert_eq!(
            native_status(&harness, collision.client_order_id()),
            OrderStatus::Denied
        );
        assert!(
            !harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        );
        let balance = native_balance(&harness);
        assert_eq!(balance.total.as_decimal(), dec("5.000100"));
        assert_eq!(balance.locked.as_decimal(), dec("0.006277"));
        assert_eq!(balance.free.as_decimal(), dec("4.993823"));
        assert_eq!(peer.batches.writes.lock().unwrap().len(), 0);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("linked")]
#[case("bracket")]
#[case("type")]
#[case("grid")]
#[case("cash")]
#[tokio::test]
async fn native_batch_list_validates_every_child_before_dispatch(#[case] invalid: &str) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::new(None).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let valid = native_order_builder(&harness, "valid-child", OrderSide::Buy)
            .quantity(Quantity::from("0.10"))
            .build();
        let mut builder = native_order_builder(&harness, "invalid-child", OrderSide::Buy);
        builder.quantity(Quantity::from(if invalid == "cash" {
            "10.00"
        } else {
            "0.20"
        }));

        match invalid {
            "linked" => {
                builder.parent_order_id("valid-child".into());
            }
            "bracket" => {
                builder
                    .contingency_type(ContingencyType::Oto)
                    .order_list_id("BATCH-LIST".into())
                    .linked_order_ids(vec!["valid-child".into()]);
            }
            "type" => {
                builder.kind(OrderType::Market);
            }
            "grid" => {
                builder.price(Price::from("0.6001"));
            }
            _ => {}
        }

        let orders = [valid, builder.build()];
        native_list(&harness, &orders);
        let events = tokio::select! {
            events = native_wait(&mut output, || {
            orders.iter().all(|order| {
                harness
                    .cache()
                    .borrow()
                    .order(&order.client_order_id())
                    .unwrap()
                    .is_closed()
            })
        })
         => events,
            () = peer.batches.written.notified() => panic!("Invalid order list reached the venue"),
        };

        for (index, order) in orders.iter().enumerate() {
            let expected = if invalid == "cash" && index == 0 {
                OrderStatus::Rejected
            } else {
                OrderStatus::Denied
            };

            assert_eq!(native_status(&harness, order.client_order_id()), expected);
        }

        if matches!(invalid, "linked" | "bracket" | "type") {
            let reason = events
                .iter()
                .find_map(|event| match event {
                    OrderEventAny::Denied(event)
                        if event.client_order_id == orders[0].client_order_id() =>
                    {
                        Some(event.reason)
                    }
                    _ => None,
                })
                .unwrap();

            assert_eq!(
                reason.as_str(),
                "VALIDATION_FAILED: Kalshi order-list child failed validation"
            );
        }

        assert_eq!(peer.batches.writes.lock().unwrap().len(), 0);
        assert_eq!(peer.submissions.writes.lock().unwrap().len(), 0);
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
                .count(),
            0
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case("unknown", false)]
#[case("unknown", true)]
#[case("whole-reject", false)]
#[case("whole-reject", true)]
#[case("permission_denied", false)]
#[case("permission_denied", true)]
#[case("duplicate_client_order_id", false)]
#[case("not_found", true)]
#[case("error-identity", false)]
#[tokio::test]
async fn native_batch_ambiguity_retains_each_child_until_authority(
    #[case] mode: &str,
    #[case] cancel: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..2)
            .map(|i| {
                native_order_builder(&harness, &format!("unknown-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        if cancel {
            native_list(&harness, &orders);
            native_wait(&mut output, || {
                orders.iter().all(|order| {
                    native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                }) && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            })
            .await;
        }

        *peer.batches.mode.lock().unwrap() = mode.into();
        let reads = peer.balance_reads.load(Ordering::SeqCst);

        let command = if cancel {
            Some(native_cancel_batch(&harness, &orders))
        } else {
            native_list(&harness, &orders);
            None
        };

        let expected = if cancel {
            OrderStatus::PendingCancel
        } else {
            OrderStatus::Submitted
        };

        let expected_writes = if cancel { 2 } else { 1 };
        while peer.batches.writes.lock().unwrap().len() < expected_writes {
            peer.batches.written.notified().await;
        }

        let events = native_wait(&mut output, || {
            peer.balance_reads.load(Ordering::SeqCst) > reads
                && orders
                    .iter()
                    .all(|order| native_status(&harness, order.client_order_id()) == expected)
        })
        .await;

        assert!(events.iter().all(|event| !matches!(
            event,
            OrderEventAny::Rejected(_)
                | OrderEventAny::CancelRejected(_)
                | OrderEventAny::Canceled(_)
        )));
        assert_eq!(native_balance(&harness).locked.as_decimal(), dec("0.12552"));
        batch_authority(&peer, cancel);

        native_refresh(&harness);

        let recovered_status = if cancel {
            OrderStatus::Canceled
        } else {
            OrderStatus::Accepted
        };

        let recovered = native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders
                .iter()
                .all(|order| native_status(&harness, order.client_order_id()) == recovered_status)
                && ready
        })
        .await;

        assert_eq!(
            recovered
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            if cancel { 2 } else { 0 }
        );

        if let Some(command) = command {
            harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .batch_cancel_orders(command)
                .unwrap();
        }

        assert_eq!(peer.batches.writes.lock().unwrap().len(), expected_writes);
        assert_eq!(
            native_balance(&harness).locked.as_decimal(),
            if cancel {
                Decimal::ZERO
            } else {
                dec("0.12552")
            }
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

fn native_scope_peers(harness: &ExecutionHarness, strategy: StrategyId) -> Vec<OrderAny> {
    let mut decoys = Vec::new();

    for kind in ["strategy", "instrument", "account", "client"] {
        let mut order = native_order_builder(harness, &format!("peer-{kind}"), OrderSide::Buy)
            .strategy_id(if kind == "strategy" {
                StrategyId::from("OTHER-001")
            } else {
                strategy
            })
            .instrument_id(if kind == "instrument" {
                "OTHER-MARKET.KALSHI".into()
            } else {
                harness.instrument_id()
            })
            .quantity(Quantity::from("0.17"))
            .build();

        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                VenueOrderId::from(format!("venue-{kind}").as_str()),
                if kind == "account" {
                    AccountId::from("KALSHI-OTHER")
                } else {
                    harness.account_id()
                },
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(1),
                false,
            )))
            .unwrap();

        harness
            .cache()
            .borrow_mut()
            .add_order(
                order.clone(),
                None,
                Some(if kind == "client" {
                    "OTHER".into()
                } else {
                    harness.client_id()
                }),
                false,
            )
            .unwrap();

        decoys.push(order);
    }

    decoys
}

fn batch_authority(peer: &Peer, cancel: bool) {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

    if cancel {
        let mut rows = peer.orders.lock().unwrap();
        for row in rows["orders"].as_array_mut().unwrap() {
            row["status"] = "canceled".into();
            row["remaining_count_fp"] = "0.00".into();
            row["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }
    } else {
        let request = peer.batches.writes.lock().unwrap()[0].1.clone();
        for (i, child) in request["orders"].as_array().unwrap().iter().enumerate() {
            if peer.orders.lock().unwrap()["orders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["client_order_id"] == child["client_order_id"])
            {
                continue;
            }

            SubmissionPeer::create(
                child,
                &fixtures,
                &peer.orders,
                "resting",
                &format!("recovered-{i}"),
            );
        }
    }
}

fn native_refresh(harness: &ExecutionHarness) {
    let cmd = QueryAccount::new(
        harness.trader_id(),
        Some(harness.client_id()),
        harness.account_id(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    harness
        .exec_engine()
        .borrow()
        .get_client(&harness.client_id())
        .unwrap()
        .query_account(cmd)
        .unwrap();
}

#[rstest]
#[tokio::test]
async fn native_batch_partial_application_keeps_the_other_child_unresolved(
    #[values(false, true)] cancel: bool,
) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..2)
            .map(|i| {
                native_order_builder(&harness, &format!("partial-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        if cancel {
            native_list(&harness, &orders);
            native_wait(&mut output, || {
                orders.iter().all(|order| {
                    native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                }) && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            })
            .await;
        }

        *peer.batches.mode.lock().unwrap() = "partial-unknown".into();

        if cancel {
            native_cancel_batch(&harness, &orders);
        } else {
            native_list(&harness, &orders);
        }

        let resolved = if cancel {
            OrderStatus::Canceled
        } else {
            OrderStatus::Accepted
        };

        let pending = if cancel {
            OrderStatus::PendingCancel
        } else {
            OrderStatus::Submitted
        };

        let held = if cancel {
            dec("0.06276")
        } else {
            dec("0.12552")
        };

        let events = native_wait(&mut output, || {
            native_status(&harness, orders[0].client_order_id()) == resolved
                && native_status(&harness, orders[1].client_order_id()) == pending
                && native_balance(&harness).locked.as_decimal() == held
        })
        .await;

        assert!(events.iter().all(|event| !matches!(
            event,
            OrderEventAny::Rejected(_) | OrderEventAny::CancelRejected(_)
        )));
        assert_eq!(
            native_status(&harness, orders[1].client_order_id()),
            pending
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), held);

        if cancel {
            assert!(
                !harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            );
        }

        batch_authority(&peer, cancel);
        native_refresh(&harness);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders
                .iter()
                .all(|order| native_status(&harness, order.client_order_id()) == resolved)
                && ready
        })
        .await;

        assert_eq!(
            peer.batches.writes.lock().unwrap().len(),
            if cancel { 2 } else { 1 }
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_batch_cancel_all_replay_preserves_later_orders() {
    tokio::time::timeout(Duration::from_secs(26), async {
        let peer = Peer::new(None).await;
        peer.submissions.ack_gate.add_permits(3);
        peer.batches.ack_gate.add_permits(3);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let original = native_order_builder(&harness, "root-original", OrderSide::Buy)
            .quantity(Quantity::from("0.10"))
            .build();
        native_submit(&harness, &original, None);
        native_wait(&mut output, || {
            native_status(&harness, original.client_order_id()) == OrderStatus::Accepted
                && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
        })
        .await;

        let command = native_cancel_all(&harness, original.strategy_id(), None);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, original.client_order_id()) == OrderStatus::Canceled && ready
        })
        .await;

        let later = native_order_builder(&harness, "root-later", OrderSide::Buy)
            .quantity(Quantity::from("0.10"))
            .build();
        let sentinel = native_order_builder(&harness, "root-sentinel", OrderSide::Buy)
            .strategy_id("SENTINEL-001".into())
            .quantity(Quantity::from("0.10"))
            .build();

        for order in [&later, &sentinel] {
            native_submit(&harness, order, None);
            native_wait(&mut output, || {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                    && harness
                        .exec_engine()
                        .borrow()
                        .get_client(&harness.client_id())
                        .unwrap()
                        .is_connected()
            })
            .await;
        }

        harness
            .exec_engine()
            .borrow()
            .execute(TradingCommand::CancelAllOrders(command));
        native_cancel_all(&harness, sentinel.strategy_id(), None);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, sentinel.client_order_id()) == OrderStatus::Canceled && ready
        })
        .await;

        assert_eq!(
            native_status(&harness, later.client_order_id()),
            OrderStatus::Accepted
        );
        assert_eq!(peer.batches.writes.lock().unwrap().len(), 2);
        assert_eq!(
            peer.batches.writes.lock().unwrap()[1].1["orders"][0]["order_id"],
            "native-3"
        );
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn native_batch_stop_preserves_dispatch_evidence(#[case] dispatched: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(1);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..2)
            .map(|i| {
                native_order_builder(&harness, &format!("stop-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected()
        })
        .await;

        *peer.batches.mode.lock().unwrap() = "unknown".into();
        native_cancel_batch(&harness, &orders);

        if dispatched {
            while peer.batches.writes.lock().unwrap().len() < 2 {
                peer.batches.written.notified().await;
            }
        }

        harness
            .exec_engine()
            .borrow_mut()
            .get_client_adapter_mut(&harness.client_id())
            .unwrap()
            .stop()
            .unwrap();
        native_disconnect(&harness).await;
        let events = native_dispatch(std::iter::from_fn(|| output.try_recv().ok()));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::CancelDenied(_)))
                .count(),
            if dispatched { 0 } else { 2 }
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            0
        );

        for order in &orders {
            assert_eq!(
                native_status(&harness, order.client_order_id()),
                if dispatched {
                    OrderStatus::PendingCancel
                } else {
                    OrderStatus::Accepted
                }
            );
        }

        assert_eq!(
            peer.batches.writes.lock().unwrap().len(),
            if dispatched { 2 } else { 1 }
        );
        peer.batches.ack_gate.add_permits(1);
    })
    .await
    .unwrap();
}

#[rstest]
#[tokio::test]
async fn native_batch_ambiguity_stops_unsent_chunks(#[values(false, true)] cancel: bool) {
    tokio::time::timeout(Duration::from_secs(18), async {
        let peer = Peer::new(None).await;
        peer.limits.lock().unwrap()["write"]["bucket_capacity"] = 33.into();
        let cost =
            serde_json::from_str::<Value>(include_str!("../../test_data/rest_responses.json"))
                .unwrap()["batch_cancel_cost"]
                .clone();
        peer.costs.lock().unwrap()["endpoint_costs"]
            .as_array_mut()
            .unwrap()
            .push(cost);
        peer.batches.ack_gate.add_permits(4);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..5)
            .map(|i| {
                native_order_builder(&harness, &format!("tail-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        if cancel {
            native_list(&harness, &orders);
            native_wait(&mut output, || {
                orders.iter().all(|order| {
                    native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                }) && harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected()
            })
            .await;
        }

        *peer.batches.mode.lock().unwrap() = "unknown".into();

        if cancel {
            native_cancel_batch(&harness, &orders);
        } else {
            native_list(&harness, &orders);
        }

        let split = if cancel { 2 } else { 3 };

        let pending = if cancel {
            OrderStatus::PendingCancel
        } else {
            OrderStatus::Submitted
        };

        let unsent = if cancel {
            OrderStatus::Accepted
        } else {
            OrderStatus::Rejected
        };

        let expected: Vec<_> = (0..5)
            .map(|i| if i < split { pending } else { unsent })
            .collect();

        let events = native_wait(&mut output, || {
            orders
                .iter()
                .zip(&expected)
                .all(|(order, status)| native_status(&harness, order.client_order_id()) == *status)
        })
        .await;

        assert_eq!(
            peer.batches.writes.lock().unwrap().len(),
            if cancel { 3 } else { 1 }
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    OrderEventAny::Rejected(_) | OrderEventAny::CancelDenied(_)
                ))
                .count(),
            5 - split
        );
        assert_unsent_outcomes(&events, &orders[split..]);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

fn assert_unsent_outcomes(events: &[OrderEventAny], orders: &[OrderAny]) {
    for order in orders {
        let event = events
            .iter()
            .find(|event| {
                event.client_order_id() == order.client_order_id()
                    && matches!(
                        event,
                        OrderEventAny::Rejected(_) | OrderEventAny::CancelDenied(_)
                    )
            })
            .unwrap();

        match event {
            OrderEventAny::Rejected(event) => {
                assert_eq!(event.reason.as_str(), "Order was not sent");
            }
            OrderEventAny::CancelDenied(event) => assert_eq!(
                event.reason.as_str(),
                "VALIDATION_FAILED: Order was not sent"
            ),
            _ => panic!("expected an unsent command outcome"),
        }
    }
}

#[tokio::test]
async fn native_batch_later_chunks_survive_private_notices() {
    tokio::time::timeout(Duration::from_secs(25), async {
        // The held acknowledgement outlives the default two-second HTTP timeout
        let peer = Peer::with_timeout(None, 10).await;
        peer.limits.lock().unwrap()["write"]["bucket_capacity"] = 33.into();
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..4)
            .map(|i| {
                native_order_builder(&harness, &format!("notice-chunk-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        let session = harness
            .cache()
            .borrow()
            .account(&harness.account_id())
            .unwrap()
            .last_event()
            .unwrap()
            .info;
        native_list(&harness, &orders);

        while peer.batches.writes.lock().unwrap().is_empty() {
            peer.batches.written.notified().await;
        }

        let notification =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_batch"]
                .clone();
        *peer.notification.lock().unwrap() = Some(notification);
        peer.notification_ready.notify_one();
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            harness
                .cache()
                .borrow()
                .account(&harness.account_id())
                .unwrap()
                .last_event()
                .unwrap()
                .info
                != session
                && ready
        })
        .await;

        assert_eq!(peer.batches.writes.lock().unwrap().len(), 1);
        peer.batches.ack_gate.add_permits(2);

        let events = native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
                    || harness
                        .cache()
                        .borrow()
                        .order(&order.client_order_id())
                        .unwrap()
                        .is_closed()
            }) && ready
        })
        .await;

        for order in &orders {
            assert_eq!(
                native_status(&harness, order.client_order_id()),
                OrderStatus::Accepted
            );
        }

        assert!(
            events
                .iter()
                .all(|event| !matches!(event, OrderEventAny::Rejected(_)))
        );
        assert_eq!(
            peer.batches
                .writes
                .lock()
                .unwrap()
                .iter()
                .map(|(_, body)| body["orders"].as_array().unwrap().len())
                .collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), dec("0.25104"));
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_batch_cancel_all_progresses_while_one_submission_is_unknown() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(3);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..2)
            .map(|i| {
                native_order_builder(&harness, &format!("known-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && ready
        })
        .await;

        let pending = native_order_builder(&harness, "unknown-selected", OrderSide::Buy)
            .quantity(Quantity::from("0.10"))
            .build();
        *peer.submissions.outcome.lock().unwrap() = "unknown".into();
        peer.submissions.ack_gate.add_permits(1);
        native_submit(&harness, &pending, None);
        peer.submissions.written.notified().await;
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, pending.client_order_id()) == OrderStatus::Submitted && ready
        })
        .await;

        native_cancel_all(&harness, pending.strategy_id(), None);
        let events = native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Canceled
            }) && ready
        })
        .await;

        assert_eq!(
            native_status(&harness, pending.client_order_id()),
            OrderStatus::Submitted
        );
        assert_eq!(peer.batches.writes.lock().unwrap().len(), 2);
        assert_eq!(native_balance(&harness).locked.as_decimal(), dec("0.06276"));
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, OrderEventAny::CancelRejected(_)))
        );
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let request = peer.submissions.writes.lock().unwrap()[0].clone();
        SubmissionPeer::create(&request, &fixtures, &peer.orders, "resting", "late-owned");
        native_refresh(&harness);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, pending.client_order_id()) == OrderStatus::Canceled && ready
        })
        .await;

        assert_eq!(peer.batches.writes.lock().unwrap().len(), 3);
        assert_eq!(
            peer.batches.writes.lock().unwrap()[2].1["orders"][0]["order_id"],
            "late-owned"
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_batch_mixed_instruments_preserve_other_owned_orders() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let peer = Peer::new(None).await;
        peer.batches.ack_gate.add_permits(3);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let mut market: Market =
            serde_json::from_value(peer.market.lock().unwrap().clone()).unwrap();
        market.ticker = "OTHER-MARKET".into();
        let definitions: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        event.exchange_index = Field::Value(3);
        let other = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        peer.market_overrides
            .lock()
            .unwrap()
            .insert(market.ticker.clone(), serde_json::to_value(market).unwrap());
        harness
            .cache()
            .borrow_mut()
            .add_instrument(other.clone().into())
            .unwrap();

        let orders = [
            native_order_builder(&harness, "instrument-primary", OrderSide::Buy)
                .quantity(Quantity::from("0.10"))
                .build(),
            native_order_builder(&harness, "instrument-other", OrderSide::Buy)
                .instrument_id(other.id)
                .quantity(Quantity::from("0.20"))
                .build(),
        ];

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && ready
        })
        .await;

        let original = peer.orders.lock().unwrap()["orders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["ticker"] == "OTHER-MARKET")
            .unwrap()
            .clone();
        native_cancel_all(&harness, orders[0].strategy_id(), Some(OrderSide::Buy));
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, orders[0].client_order_id()) == OrderStatus::Canceled && ready
        })
        .await;

        assert_eq!(
            native_status(&harness, orders[1].client_order_id()),
            OrderStatus::Accepted
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), dec("0.12552"));
        assert_eq!(peer.batches.writes.lock().unwrap().len(), 2);
        assert_eq!(
            peer.batches.writes.lock().unwrap()[1].1["orders"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            peer.batches.writes.lock().unwrap()[1].1["orders"][0]["order_id"],
            "batch-1-0"
        );
        assert_eq!(
            *peer.orders.lock().unwrap()["orders"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["ticker"] == "OTHER-MARKET")
                .unwrap(),
            original
        );
        native_cancel_batch(&harness, &orders[1..]);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, orders[1].client_order_id()) == OrderStatus::Canceled && ready
        })
        .await;

        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[rstest]
#[case::partially_closed(false)]
#[case::fully_closed(true)]
#[tokio::test]
async fn native_batch_cancel_all_skips_targets_closed_between_chunks(#[case] fully_closed: bool) {
    tokio::time::timeout(Duration::from_secs(40), async {
        let peer = Peer::with_timeout(None, 10).await;
        peer.limits.lock().unwrap()["write"]["bucket_capacity"] = 33.into();
        let cost =
            serde_json::from_str::<Value>(include_str!("../../test_data/rest_responses.json"))
                .unwrap()["batch_cancel_cost"]
                .clone();
        peer.costs.lock().unwrap()["endpoint_costs"]
            .as_array_mut()
            .unwrap()
            .push(cost);
        peer.batches.ack_gate.add_permits(2);
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let orders: Vec<_> = (0..5)
            .map(|i| {
                native_order_builder(&harness, &format!("closing-target-{i}"), OrderSide::Buy)
                    .quantity(Quantity::from("0.10"))
                    .build()
            })
            .collect();

        native_list(&harness, &orders);
        native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            orders.iter().all(|order| {
                native_status(&harness, order.client_order_id()) == OrderStatus::Accepted
            }) && ready
        })
        .await;

        native_cancel_all(&harness, orders[0].strategy_id(), Some(OrderSide::Buy));

        while peer.batches.writes.lock().unwrap().len() < 3 {
            peer.batches.written.notified().await;
        }

        let filled_id = orders[2].client_order_id();
        native_fill_for_order(&peer, filled_id.as_str(), "0.10", true);
        if fully_closed {
            let mut rows = peer.orders.lock().unwrap();
            let row = rows["orders"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["client_order_id"] == orders[3].client_order_id().as_str())
                .unwrap();
            row["status"] = "canceled".into();
            row["remaining_count_fp"] = "0.00".into();
            row["last_update_time"] = jiff::Timestamp::now().to_string().into();
        }

        let mut events = native_wait(&mut output, || {
            let ready = harness
                .exec_engine()
                .borrow()
                .get_client(&harness.client_id())
                .unwrap()
                .is_connected();
            native_status(&harness, filled_id) == OrderStatus::Filled
                && (!fully_closed
                    || native_status(&harness, orders[3].client_order_id())
                        == OrderStatus::Canceled)
                && ready
        })
        .await;

        assert_eq!(peer.batches.writes.lock().unwrap().len(), 3);
        peer.batches.ack_gate.add_permits(3);
        events.extend(
            native_wait(&mut output, || {
                let ready = harness
                    .exec_engine()
                    .borrow()
                    .get_client(&harness.client_id())
                    .unwrap()
                    .is_connected();
                orders.iter().enumerate().all(|(i, order)| {
                    native_status(&harness, order.client_order_id())
                        == if i == 2 {
                            OrderStatus::Filled
                        } else {
                            OrderStatus::Canceled
                        }
                }) && ready
            })
            .await,
        );

        let writes = peer.batches.writes.lock().unwrap().clone();
        let requested: Vec<_> = writes[2..]
            .iter()
            .flat_map(|(_, request)| request["orders"].as_array().unwrap())
            .map(|row| row["order_id"].as_str().unwrap().to_owned())
            .collect();

        let expected: Vec<_> = orders
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2 && (!fully_closed || *i != 3))
            .map(|(_, order)| {
                harness
                    .cache()
                    .borrow()
                    .order(&order.client_order_id())
                    .unwrap()
                    .venue_order_id()
                    .unwrap()
                    .to_string()
            })
            .collect();

        assert_eq!(requested, expected);
        assert_eq!(writes.len(), if fully_closed { 4 } else { 5 });
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Canceled(_)))
                .count(),
            4
        );
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, OrderEventAny::CancelRejected(_)))
        );
        assert_eq!(
            harness
                .cache()
                .borrow()
                .order(&filled_id)
                .unwrap()
                .filled_qty(),
            Quantity::from("0.10")
        );
        assert_eq!(native_balance(&harness).locked.as_decimal(), Decimal::ZERO);
        native_disconnect(&harness).await;
    })
    .await
    .unwrap();
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "report collection borrows the registered execution client"
)]
#[tokio::test]
async fn conformance_schedule_preserves_order_totals_reports_and_teardown() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let mut data_peer = crate::data_peer::Peer::new().await;
        let id = parse_instrument_id("KXBTC15M-26OCT162345-45").unwrap();
        let (data_sender, mut data_events) = tokio::sync::mpsc::unbounded_channel();
        let mut data_client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            data_peer.config.clone(),
            vec![id],
            data_sender.into(),
        )
        .unwrap();

        data_client.connect().await.unwrap();

        let DataEvent::Data(Data::Instrument(instrument)) = data_events.recv().await.unwrap()
        else {
            panic!("expected bootstrap definition");
        };

        let InstrumentAny::BinaryOption(instrument) = *instrument else {
            panic!("expected binary option");
        };

        assert_eq!(instrument.id, id);
        let lifecycle = data_peer.command().await;
        assert_eq!(lifecycle["cmd"], "subscribe");
        assert_eq!(lifecycle["params"]["channels"][0], "market_lifecycle_v2");

        data_client
            .subscribe_quotes(SubscribeQuotes::new(
                id,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(2),
                None,
                None,
            ))
            .unwrap();
        let quotes = data_peer.command().await;
        assert_eq!(quotes["params"]["channels"][0], "ticker");
        data_peer.emit("ticker", id, false);

        let DataEvent::Data(Data::Quote(tick)) = data_events.recv().await.unwrap() else {
            panic!("expected quote");
        };

        assert_eq!(tick.instrument_id, id);
        assert_eq!(tick.bid_price.as_decimal(), dec("0.123456"));
        assert_eq!(tick.ask_price.as_decimal(), dec("0.789123"));
        assert_eq!(tick.bid_size.as_decimal(), dec("3.25"));
        assert_eq!(tick.ask_size.as_decimal(), dec("6.75"));
        assert_eq!(tick.ts_event, UnixNanos::from(1_700_000_123_789_000_000));

        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(id);
        data_client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                id,
                BookType::L2_MBP,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(3),
                None,
                true,
                None,
                None,
            ))
            .unwrap();
        let books = data_peer.command().await;
        assert_eq!(books["cmd"], "subscribe");
        assert_eq!(books["params"]["use_yes_price"], true);

        let DataEvent::Data(Data::BookDeltas(batch)) = data_events.recv().await.unwrap() else {
            panic!("expected book snapshot");
        };

        checker.apply(&batch).unwrap();
        let (bids, asks) = book_oracle();
        checker.verify(id, 100, &bids, &asks).unwrap();
        let mut frame =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["stale_book_delta"]
                .clone();
        frame["msg"]["market_ticker"] = serde_json::json!(id.symbol.as_str());
        data_peer.actions.send(("raw".into(), frame)).unwrap();
        data_peer.emit("ticker", id, false);

        let DataEvent::Data(Data::Quote(tick)) = data_events.recv().await.unwrap() else {
            panic!("expected quote after the ignored stale frame");
        };

        assert_eq!(tick.instrument_id, id);
        assert!(data_events.try_recv().is_err());

        let peer = Peer::with_timeout(None, 10).await;
        let (harness, mut output, _data) = native_consumer(&peer).await;
        let order = native_order(&harness, "conformance-limit", OrderSide::Buy, false);
        native_submit(&harness, &order, None);
        peer.submissions.written.notified().await;
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let now = jiff::Timestamp::now();
        let mut source = peer.orders.lock().unwrap()["orders"][0].clone();
        source["status"] = "executed".into();
        source["fill_count_fp"] = "1.25".into();
        source["remaining_count_fp"] = "0.00".into();
        source["last_update_time"] = now.to_string().into();
        source["taker_fees_dollars"] = "0.000100".into();
        let mut fill = fixtures["fills"]["fills"][0].clone();
        fill["order_id"] = source["order_id"].clone();
        fill["ticker"] = source["ticker"].clone();
        fill["market_ticker"] = source["ticker"].clone();
        fill["outcome_side"] = source["outcome_side"].clone();
        fill["book_side"] = source["book_side"].clone();
        fill["count_fp"] = "1.25".into();
        fill["yes_price_dollars"] = "0.600000".into();
        fill["no_price_dollars"] = "0.400000".into();
        fill["fee_cost"] = "0.000100".into();
        fill["created_time"] = now.to_string().into();
        let mut position = fixtures["positions"]["market_positions"][0].clone();
        position["ticker"] = source["ticker"].clone();
        position["position_fp"] = "1.25".into();
        position["last_updated_ts"] = now.to_string().into();
        *peer.orders.lock().unwrap() = json!({"orders":[source], "cursor": ""});
        *peer.fills.lock().unwrap() = json!({"fills":[fill], "cursor": ""});
        *peer.positions.lock().unwrap() =
            json!({"market_positions":[position], "event_positions":[], "cursor": ""});
        peer.balance.lock().unwrap()["balance_dollars"] = "0.250000".into();

        let client_order_id = order.client_order_id();
        let events = native_wait(&mut output, || {
            native_status(&harness, client_order_id) == OrderStatus::Filled
                && native_balance(&harness).total.as_decimal() == dec("0.250000")
        })
        .await;

        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));

        let OrderEventAny::Filled(filled) = &events[2] else {
            panic!("expected fill after accept");
        };

        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.strategy_id, order.strategy_id());
        assert_eq!(filled.instrument_id, order.instrument_id());
        assert_eq!(filled.venue_order_id, VenueOrderId::from("native-1"));
        assert_eq!(filled.last_qty, Quantity::from("1.25"));
        assert_eq!(filled.last_px, Price::from("0.600000"));
        assert_eq!(filled.commission.unwrap().as_decimal(), dec("0.000100"));
        assert_eq!(events.len(), 3);

        let balance = native_balance(&harness);
        assert_eq!(balance.total.as_decimal(), dec("0.250000"));
        assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
        assert_eq!(balance.free.as_decimal(), dec("0.250000"));
        {
            let cache = harness.cache().borrow();
            let positions = cache.positions_open(
                None,
                Some(&order.instrument_id()),
                None,
                Some(&harness.account_id()),
                None,
            );
            assert_eq!(positions.len(), 1);
            assert_eq!(positions[0].signed_decimal_qty(), dec("1.25"));
            assert_eq!(positions[0].strategy_id, order.strategy_id());
        }

        let order_reports = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                Some(order.instrument_id()),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        let fill_reports = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                Some(order.instrument_id()),
                Some(VenueOrderId::from("native-1")),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        let position_reports = harness
            .exec_engine()
            .borrow()
            .get_client(&harness.client_id())
            .unwrap()
            .generate_position_status_reports(&GeneratePositionStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                Some(order.instrument_id()),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(order_reports.len(), 1);
        assert_eq!(
            order_reports[0].venue_order_id,
            VenueOrderId::from("native-1")
        );
        assert_eq!(order_reports[0].client_order_id, Some(client_order_id));
        assert_eq!(order_reports[0].quantity, Quantity::from("1.25"));
        assert_eq!(order_reports[0].filled_qty, Quantity::from("1.25"));
        assert_eq!(fill_reports.len(), 1);
        assert_eq!(fill_reports[0].trade_id, TradeId::from("fill-a"));
        assert_eq!(fill_reports[0].client_order_id, Some(client_order_id));
        assert_eq!(fill_reports[0].commission.as_decimal(), dec("0.000100"));
        assert_eq!(position_reports.len(), 1);
        assert_eq!(position_reports[0].signed_decimal_qty, dec("1.25"));

        peer.submissions.ack_gate.add_permits(1);
        data_client.disconnect().await.unwrap();
        native_disconnect(&harness).await;
        let data_requests = data_peer.requests.load(Ordering::SeqCst);
        let writes = peer.submissions.writes.lock().unwrap().len();
        let balance_reads = peer.balance_reads.load(Ordering::SeqCst);
        wait_until_async(
            || async { peer.sessions.load(Ordering::SeqCst) == 0 },
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(data_peer.requests.load(Ordering::SeqCst), data_requests);
        assert!(data_events.try_recv().is_err());

        while let Ok(command) = data_peer.commands.try_recv() {
            assert_eq!(command["cmd"], "closed");
        }

        assert_eq!(peer.submissions.writes.lock().unwrap().len(), writes);
        assert_eq!(peer.balance_reads.load(Ordering::SeqCst), balance_reads);
        assert_eq!(peer.sessions.load(Ordering::SeqCst), 0);
        assert!(output.try_recv().is_err());
    })
    .await
    .unwrap();
}

fn book_oracle() -> (BTreeMap<Decimal, Decimal>, BTreeMap<Decimal, Decimal>) {
    let fixture: Value = serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
    let mut bids = BTreeMap::new();
    let mut asks = BTreeMap::new();

    for row in fixture["snapshot"]["yes_dollars_fp"].as_array().unwrap() {
        bids.insert(
            row[0].as_str().unwrap().parse().unwrap(),
            row[1].as_str().unwrap().parse().unwrap(),
        );
    }

    for row in fixture["snapshot"]["no_dollars_fp"].as_array().unwrap() {
        asks.insert(
            row[0].as_str().unwrap().parse().unwrap(),
            row[1].as_str().unwrap().parse().unwrap(),
        );
    }

    (bids, asks)
}
