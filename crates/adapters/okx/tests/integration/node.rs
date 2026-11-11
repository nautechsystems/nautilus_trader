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

//! OKX submission recovery through the native node and localhost transports.

use std::{
    cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, sync::Arc, time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::{
    actor::DataActor,
    enums::Environment,
    msgbus::{self, MessagingSwitchboard, stubs::get_any_saving_handler},
    testing::wait_until_async,
};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    execution::submission::{
        SubmissionRecoveryExhausted, SubmissionRecoveryPolicy, SubmissionRecoverySource,
    },
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::Instrument,
    orders::{Order, OrderTestBuilder},
    reports::OrderStatusReport,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_okx::{
    common::{
        enums::{OKXEnvironment, OKXInstrumentType, OKXRegion},
        models::OKXInstrument,
        parse::parse_instrument_any,
    },
    config::OKXExecutionClientConfig,
    factories::OKXExecutionClientFactory,
    http::client::OKXResponse,
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rstest::rstest;
use serde_json::{Value, json};

const CLIENT_ORDER_ID: &str = "ORecovery1";
const VENUE_ORDER_ID: &str = "2497956918703120386";
const INSTRUMENT_ID: &str = "BTC-USD.OKX";
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct SubmitOnStart {
    core: StrategyCore,
    events: Rc<RefCell<Vec<OrderEventAny>>>,
    query_on: Option<QueryOn>,
}

impl DataActor for SubmitOnStart {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TESTER-001"))
            .strategy_id(StrategyId::from("RECOVERY-001"))
            .instrument_id(InstrumentId::from(INSTRUMENT_ID))
            .client_order_id(ClientOrderId::from(CLIENT_ORDER_ID))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.00100000"))
            .price(Price::from("50000.0"))
            .build();
        self.submit_order(order, None, Some(ClientId::from("OKX")), None)?;
        Ok(())
    }
}

nautilus_strategy!(SubmitOnStart, {
    fn on_order_event(&mut self, event: OrderEventAny) {
        let query = matches!(
            (self.query_on, &event),
            (Some(QueryOn::Submitted), OrderEventAny::Submitted(_))
                | (Some(QueryOn::Accepted), OrderEventAny::Accepted(_))
                | (Some(QueryOn::Filled), OrderEventAny::Filled(_))
        );

        if query {
            self.query_on = None;
            let order = self.cache().order(&event.client_order_id()).unwrap();
            self.query_order(&order, Some(ClientId::from("OKX")), None)
                .unwrap();
        }
        self.events.borrow_mut().push(event);
    }
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryOn {
    Submitted,
    Accepted,
    Inflight,
    Filled,
}

#[derive(Debug)]
struct VenueState {
    submit_code: Option<&'static str>,
    order_response: tokio::sync::Mutex<Option<Value>>,
    fill_response: tokio::sync::Mutex<Vec<Value>>,
    hold_history: tokio::sync::Mutex<bool>,
    history_started: tokio::sync::Notify,
    history_release: tokio::sync::Notify,
    submissions: tokio::sync::Mutex<Vec<Value>>,
    queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    fill_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    updates: tokio::sync::broadcast::Sender<Value>,
}

fn spot_instruments() -> Value {
    let mut response: Value = serde_json::from_str(include_str!(
        "../../test_data/http_get_instruments_spot.json"
    ))
    .unwrap();
    response["data"].as_array_mut().unwrap().truncate(1);
    response["data"][0]["instIdCode"] = json!(42);
    response
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<Arc<VenueState>>) -> Response {
    ws.on_upgrade(move |socket| serve_socket(socket, state))
}

async fn serve_socket(mut socket: WebSocket, state: Arc<VenueState>) {
    let mut updates = state.updates.subscribe();
    let mut orders_subscribed = false;

    loop {
        let message = tokio::select! {
            message = socket.next() => match message {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Ping(data))) => {
                    if socket.send(Message::Pong(data)).await.is_err() { break; }
                    continue;
                }
                _ => break,
            },
            update = updates.recv(), if orders_subscribed => {
                if socket.send(Message::Text(update.unwrap().to_string().into())).await.is_err() { break; }
                continue;
            }
        };

        if message == "ping" {
            if socket.send(Message::Text("pong".into())).await.is_err() {
                break;
            }
            continue;
        }
        let request: Value = serde_json::from_str(&message).unwrap();
        let response = match request["op"].as_str().unwrap() {
            "login" => json!({"event": "login", "code": "0", "msg": "", "connId": "test"}),
            "subscribe" => {
                for arg in request["args"].as_array().unwrap() {
                    orders_subscribed |= arg["channel"] == "orders";
                    socket
                        .send(Message::Text(
                            json!({"event": "subscribe", "arg": arg, "connId": "test"})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                }
                continue;
            }
            "amend-order" => continue,
            "order" => {
                state.submissions.lock().await.push(request.clone());
                let Some(code) = state.submit_code else {
                    continue;
                };
                json!({
                    "id": request["id"], "op": "order", "code": "1", "msg": "",
                    "data": [{"clOrdId": request["args"][0]["clOrdId"], "ordId": "", "tag": "", "sCode": code, "sMsg": "test response"}]
                })
            }
            _ => panic!("unexpected WebSocket request: {request}"),
        };

        if socket
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn start_venue(code: Option<&'static str>) -> (SocketAddr, Arc<VenueState>) {
    let (updates, _) = tokio::sync::broadcast::channel(16);
    let state = Arc::new(VenueState {
        submit_code: code,
        order_response: tokio::sync::Mutex::new(None),
        fill_response: tokio::sync::Mutex::new(Vec::new()),
        hold_history: tokio::sync::Mutex::new(false),
        history_started: tokio::sync::Notify::new(),
        history_release: tokio::sync::Notify::new(),
        submissions: tokio::sync::Mutex::new(Vec::new()),
        queries: tokio::sync::Mutex::new(Vec::new()),
        fill_queries: tokio::sync::Mutex::new(Vec::new()),
        updates,
    });
    let router = Router::new()
        .route("/ws/private", get(websocket))
        .route("/ws/business", get(websocket))
        .route(
            "/api/v5/public/instruments",
            get(|| async { Json(spot_instruments()) }),
        )
        .route(
            "/api/v5/account/instruments",
            get(|| async { Json(spot_instruments()) }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async {
                let mut balance: Value = serde_json::from_str(include_str!(
                    "../../test_data/http_get_account_balance.json"
                ))
                .unwrap();
                balance["data"][0]["details"][0]["ccy"] = json!("USD");
                Json(balance)
            }),
        )
        .route(
            "/api/v5/trade/order",
            get(
                |State(state): State<Arc<VenueState>>,
                 Query(query): Query<HashMap<String, String>>| async move {
                    state.queries.lock().await.push(query);
                    Json(state.order_response.lock().await.clone().unwrap_or_else(
                        || json!({"code": "51603", "msg": "Order does not exist", "data": []}),
                    ))
                },
            ),
        )
        .route("/api/v5/trade/orders-pending", get(order_reports))
        .route("/api/v5/trade/orders-history", get(order_reports))
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})) }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})) }),
        )
        .route("/api/v5/trade/fills", get(order_fills))
        .route("/api/v5/trade/fills-history", get(order_fills))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (addr, state)
}

async fn order_reports(State(state): State<Arc<VenueState>>) -> Json<Value> {
    Json(
        state
            .order_response
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| json!({"code": "0", "msg": "", "data": []})),
    )
}

async fn order_fills(
    State(state): State<Arc<VenueState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    state.fill_queries.lock().await.push(query);
    if *state.hold_history.lock().await {
        state.history_started.notify_one();
        state.history_release.notified().await;
        *state.hold_history.lock().await = false;
    }

    Json(json!({"code": "0", "msg": "", "data": state.fill_response.lock().await.clone()}))
}

fn build_node(
    addr: SocketAddr,
    retain: bool,
    events: Rc<RefCell<Vec<OrderEventAny>>>,
    query_on: Option<QueryOn>,
) -> LiveNode {
    let mut exec_engine = LiveExecutionEngineConfig {
        reconciliation: false,
        inflight_check_interval_ms: 10,
        inflight_check_threshold_ms: if query_on.is_some_and(|query| query != QueryOn::Inflight) {
            DEADLINE.as_millis().try_into().unwrap()
        } else {
            100
        },
        inflight_check_retries: 2,
        ..Default::default()
    };

    if retain {
        exec_engine.submission_recovery_policy = SubmissionRecoveryPolicy::RetainUnresolved;
    }

    build_node_with_config(addr, events, query_on, exec_engine)
}

fn build_node_with_config(
    addr: SocketAddr,
    events: Rc<RefCell<Vec<OrderEventAny>>>,
    query_on: Option<QueryOn>,
    exec_engine: LiveExecutionEngineConfig,
) -> LiveNode {
    let mut node = LiveNodeBuilder::from_config(LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from("TESTER-001"),
        exec_engine,
        delay_post_stop: Duration::from_millis(20),
        ..Default::default()
    })
    .unwrap()
    .with_name("OKXSubmissionRecovery")
    .add_exec_client(
        None,
        Box::new(OKXExecutionClientFactory),
        Box::new(OKXExecutionClientConfig {
            api_key: Some("test-key".into()),
            api_secret: Some("test-secret".into()),
            api_passphrase: Some("test-passphrase".into()),
            instrument_types: vec![OKXInstrumentType::Spot],
            environment: OKXEnvironment::Demo,
            region: OKXRegion::Eea,
            base_url_http: Some(format!("http://{addr}")),
            base_url_ws_private: Some(format!("ws://{addr}/ws/private")),
            base_url_ws_business: Some(format!("ws://{addr}/ws/business")),
            max_retries: 0,
            max_reconciliation_history_pages: Some(0),
            ..Default::default()
        }),
    )
    .unwrap()
    .build()
    .unwrap();
    let instruments: OKXResponse<OKXInstrument> =
        serde_json::from_value(spot_instruments()).unwrap();
    let instrument = parse_instrument_any(&instruments.data[0], None, None, UnixNanos::default())
        .unwrap()
        .unwrap();
    assert_eq!(instrument.id(), InstrumentId::from(INSTRUMENT_ID));
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument)
        .unwrap();
    node.add_strategy(SubmitOnStart {
        core: StrategyCore::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("RECOVERY-001")),
            ..Default::default()
        }),
        events,
        query_on,
    })
    .unwrap();
    node
}

fn venue_update(filled: bool, submitted_ms: u64) -> Value {
    let mut frame: Value =
        serde_json::from_str(include_str!("../../test_data/ws_orders_ioc.json")).unwrap();
    let order = &mut frame["data"][0];
    order["clOrdId"] = json!(CLIENT_ORDER_ID);
    let now_ms = get_atomic_clock_realtime().get_time_ms().to_string();
    order["cTime"] = json!(submitted_ms.to_string());
    order["uTime"] = json!(now_ms);
    order["fillTime"] = json!(if filled { now_ms.as_str() } else { "" });
    order["instId"] = json!("BTC-USD");
    order["side"] = json!("buy");
    order["ordType"] = json!("limit");
    order["px"] = json!("50000.0");
    order["sz"] = json!("0.00100000");
    order["ccy"] = json!("USD");
    order["feeCcy"] = json!("USD");
    order["state"] = json!(if filled { "filled" } else { "live" });
    order["accFillSz"] = json!(if filled { "0.00100000" } else { "0" });
    order["fillSz"] = json!(if filled { "0.00100000" } else { "0" });
    order["avgPx"] = json!(if filled { "50000.0" } else { "" });
    order["fillPx"] = order["avgPx"].clone();
    order["tradeId"] = json!(if filled { "1518905531" } else { "" });
    order["fee"] = json!(if filled { "-0.05" } else { "0" });
    frame
}

#[rstest]
#[case::timeout_50004(Some("50004"))]
#[case::timeout_51149(Some("51149"))]
#[case::missing_acknowledgement(None)]
#[tokio::test]
async fn submission_exhaustion_and_late_evidence(
    #[case] code: Option<&'static str>,
    #[values(false, true)] retain: bool,
) {
    let (addr, venue) = start_venue(code).await;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut node = build_node(addr, retain, Rc::clone(&events), None);
    let cache = Rc::clone(&node.kernel().cache);
    let handle = node.handle();
    let (handler, diagnostics) = get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    let topic = MessagingSwitchboard::submission_recovery_exhausted_topic();
    msgbus::subscribe_any(topic.into(), handler.clone(), None);
    let driver = async {
        wait_until_async(
            || async {
                if retain {
                    !diagnostics.get_messages().is_empty()
                } else {
                    events
                        .borrow()
                        .iter()
                        .any(|e| matches!(e, OrderEventAny::Rejected(_)))
                }
            },
            DEADLINE,
        )
        .await;
        let expected = if retain {
            OrderStatus::Submitted
        } else {
            OrderStatus::Rejected
        };
        assert_eq!(
            cache
                .borrow()
                .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                .unwrap()
                .status(),
            expected
        );
        // Observe beyond several check intervals to prove exhaustion stops new queries
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(venue.queries.lock().await.len(), 1);
        let submitted_ms = cache
            .borrow()
            .order(&ClientOrderId::from(CLIENT_ORDER_ID))
            .unwrap()
            .ts_submitted()
            .unwrap()
            .as_u64()
            / 1_000_000;

        if retain && code == Some("50004") {
            venue
                .updates
                .send(venue_update(false, submitted_ms))
                .unwrap();
            wait_until_async(
                || async {
                    events
                        .borrow()
                        .iter()
                        .any(|e| matches!(e, OrderEventAny::Accepted(_)))
                },
                DEADLINE,
            )
            .await;
        }
        let fill = venue_update(true, submitted_ms);
        venue.updates.send(fill.clone()).unwrap();

        if retain {
            wait_until_async(
                || async {
                    events
                        .borrow()
                        .iter()
                        .any(|e| matches!(e, OrderEventAny::Filled(_)))
                },
                DEADLINE,
            )
            .await;
        }
        venue.updates.send(fill).unwrap();
        // A following account update on the same private stream proves the duplicate was consumed
        let sentinel_ms = get_atomic_clock_realtime().get_time_ms();
        let mut account: Value = serde_json::from_str(include_str!(
            "../../test_data/http_get_account_balance.json"
        ))
        .unwrap();
        account["data"][0]["details"][0]["ccy"] = json!("USD");
        account["data"][0]["uTime"] = json!(sentinel_ms.to_string());
        venue
            .updates
            .send(json!({"arg": {"channel": "account"}, "data": account["data"]}))
            .unwrap();
        wait_until_async(
            || async {
                cache
                    .borrow()
                    .account(&AccountId::from("OKX-001"))
                    .and_then(|account| account.last_event())
                    .is_some_and(|event| event.ts_event.as_u64() == sentinel_ms * 1_000_000)
            },
            DEADLINE,
        )
        .await;
        handle.stop();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();
    result.unwrap();
    msgbus::unsubscribe_any(topic.into(), &handler);
    let diagnostics = diagnostics.get_messages();
    assert_eq!(diagnostics.len(), usize::from(retain));

    if let Some(diagnostic) = diagnostics.first() {
        assert_eq!(diagnostic.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(diagnostic.client_id, Some(ClientId::from("OKX")));
        assert_eq!(diagnostic.strategy_id, StrategyId::from("RECOVERY-001"));
        assert_eq!(diagnostic.instrument_id, InstrumentId::from(INSTRUMENT_ID));
        assert_eq!(
            diagnostic.client_order_id,
            ClientOrderId::from(CLIENT_ORDER_ID)
        );
        assert_eq!(diagnostic.source, SubmissionRecoverySource::Inflight);
        assert_eq!(diagnostic.retry_count, 2);
    }
    let submissions = venue.submissions.lock().await;
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0]["args"][0]["clOrdId"], CLIENT_ORDER_ID);
    assert_eq!(submissions[0]["args"][0]["tdMode"], "cash");
    assert_eq!(submissions[0]["args"][0]["instIdCode"], 42);
    let queries = venue.queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0]["clOrdId"], CLIENT_ORDER_ID);
    assert_eq!(queries[0]["instId"], "BTC-USD");
    let events = events.borrow();
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
            .filter(|e| matches!(e, OrderEventAny::Rejected(_)))
            .count(),
        usize::from(!retain)
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, OrderEventAny::Filled(_)))
            .count(),
        usize::from(retain)
    );
    let cache = cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();

    if retain {
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(
            order.venue_order_id(),
            Some(VenueOrderId::from(VENUE_ORDER_ID))
        );
        assert_eq!(order.filled_qty(), Quantity::from("0.00100000"));
        assert_eq!(order.trade_ids(), vec![&TradeId::from("1518905531")]);
        assert_eq!(
            order.commissions().get(&Currency::USD()),
            Some(&Money::from("0.05 USD"))
        );
        let positions = cache.positions_open(None, None, None, None, None);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("0.00100000"));
    } else {
        assert_eq!(order.status(), OrderStatus::Rejected);
        assert_eq!(order.filled_qty(), Quantity::from("0.00000000"));
        assert!(
            cache
                .positions_open(None, None, None, None, None)
                .is_empty()
        );
    }
}

#[rstest]
#[tokio::test]
async fn submission_exhaustion_at_shutdown(#[values(false, true)] retain: bool) {
    let (addr, venue) = start_venue(Some("50004")).await;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut node = build_node(addr, retain, Rc::clone(&events), None);
    let handle = node.handle();
    let (handler, diagnostics) = get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    let topic = MessagingSwitchboard::submission_recovery_exhausted_topic();
    msgbus::subscribe_any(topic.into(), handler.clone(), None);
    let driver = async {
        wait_until_async(
            || async {
                if retain {
                    !diagnostics.get_messages().is_empty()
                } else {
                    events
                        .borrow()
                        .iter()
                        .any(|e| matches!(e, OrderEventAny::Rejected(_)))
                }
            },
            DEADLINE,
        )
        .await;
        handle.stop();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();
    msgbus::unsubscribe_any(topic.into(), &handler);

    if retain {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Submission recovery incomplete at shutdown"));
        assert!(error.contains(CLIENT_ORDER_ID));
    } else {
        result.unwrap();
    }
    assert_eq!(node.state(), NodeState::Stopped);
    assert!(node.kernel().check_engines_disconnected());
    assert_eq!(venue.submissions.lock().await.len(), 1);
    assert_eq!(venue.queries.lock().await.len(), 1);
    assert_eq!(diagnostics.get_messages().len(), usize::from(retain));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, OrderEventAny::Rejected(_)))
            .count(),
        usize::from(!retain)
    );
    let cache = node.kernel().cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(
        order.status(),
        if retain {
            OrderStatus::Submitted
        } else {
            OrderStatus::Rejected
        }
    );
    assert_eq!(order.is_inflight(), retain);
    assert_eq!(order.filled_qty(), Quantity::from("0.00000000"));
    assert!(
        cache
            .positions_open(None, None, None, None, None)
            .is_empty()
    );
}

#[rstest]
#[tokio::test]
async fn authoritative_submit_rejection(#[values(false, true)] retain: bool) {
    let (addr, venue) = start_venue(Some("51008")).await;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut node = build_node(addr, retain, Rc::clone(&events), None);
    let handle = node.handle();
    let driver = async {
        wait_until_async(
            || async {
                events
                    .borrow()
                    .iter()
                    .any(|e| matches!(e, OrderEventAny::Rejected(_)))
            },
            DEADLINE,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        handle.stop();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(venue.submissions.lock().await.len(), 1);
    assert!(venue.queries.lock().await.is_empty());
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, OrderEventAny::Rejected(_)))
            .count(),
        1
    );
    let cache = node.kernel().cache.borrow();
    assert_eq!(
        cache
            .order(&ClientOrderId::from(CLIENT_ORDER_ID))
            .unwrap()
            .status(),
        OrderStatus::Rejected
    );
}

#[rstest]
#[tokio::test]
async fn query_preserves_authoritative_fills(
    #[values(false, true)] partial: bool,
    #[values(
        QueryOn::Submitted,
        QueryOn::Accepted,
        QueryOn::Inflight,
        QueryOn::Filled
    )]
    query_on: QueryOn,
    #[values(false, true)] base_fee: bool,
) {
    let (addr, venue) = start_venue(None).await;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut node = build_node(addr, true, Rc::clone(&events), Some(query_on));
    let cache = Rc::clone(&node.kernel().cache);
    let handle = node.handle();
    let submitted_ms = get_atomic_clock_realtime().get_time_ms();

    let fee_ccy = if base_fee { "BTC" } else { "USD" };
    let total_qty = "0.00100000";

    let initial_qty = if partial { "0.00040000" } else { total_qty };

    let initial_fee = match (partial, base_fee) {
        (true, true) => "-0.0000008",
        (false, true) => "-0.000002",
        (true, false) => "-0.02",
        (false, false) => "-0.05",
    };

    let mut initial = venue_update(true, submitted_ms);
    let order = &mut initial["data"][0];
    order["sz"] = json!(total_qty);
    order["state"] = json!(if partial {
        "partially_filled"
    } else {
        "filled"
    });
    order["accFillSz"] = json!(initial_qty);
    order["fillSz"] = json!(initial_qty);
    order["feeCcy"] = json!(fee_ccy);
    order["fee"] = json!(initial_fee);
    order["tradeId"] = json!("90001");

    let mut response: Value =
        serde_json::from_str(include_str!("../../test_data/http_get_orders_history.json")).unwrap();
    response["data"] = initial["data"].clone();
    *venue.order_response.lock().await = Some(response);

    let (report_handler, reports) = get_any_saving_handler::<OrderStatusReport>(None);
    let report_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    msgbus::subscribe_any(report_topic.into(), report_handler.clone(), None);
    let (exhaustion_handler, exhaustions) =
        get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    let exhaustion_topic = MessagingSwitchboard::submission_recovery_exhausted_topic();
    msgbus::subscribe_any(exhaustion_topic.into(), exhaustion_handler.clone(), None);

    let driver = async {
        wait_until_async(
            || async { !venue.submissions.lock().await.is_empty() },
            DEADLINE,
        )
        .await;

        if matches!(query_on, QueryOn::Accepted | QueryOn::Filled) {
            venue
                .updates
                .send(venue_update(false, submitted_ms))
                .unwrap();
        }

        if query_on == QueryOn::Filled {
            venue.updates.send(initial.clone()).unwrap();
        }

        wait_until_async(|| async { !reports.get_messages().is_empty() }, DEADLINE).await;

        if matches!(query_on, QueryOn::Submitted | QueryOn::Inflight) {
            wait_until_async(
                || async {
                    cache
                        .borrow()
                        .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                        .is_some_and(|order| order.status() == OrderStatus::Accepted)
                },
                DEADLINE,
            )
            .await;

            if query_on == QueryOn::Submitted {
                // Cross the inflight deadline without delaying socket tasks
                tokio::time::pause();
                tokio::time::advance(DEADLINE).await;
                tokio::time::resume();
            }

            // Observe late recovery events before allowing authoritative fill evidence
            tokio::time::sleep(Duration::from_millis(400)).await;
            let cache = cache.borrow();
            let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
            assert_eq!(order.status(), OrderStatus::Accepted);
            assert_eq!(
                order.venue_order_id(),
                Some(VenueOrderId::from(VENUE_ORDER_ID))
            );
            assert_eq!(order.filled_qty(), Quantity::from("0.00000000"));
            assert!(order.trade_ids().is_empty());
            assert!(order.commissions().is_empty());
            assert!(exhaustions.get_messages().is_empty());
        }

        if query_on != QueryOn::Filled {
            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|e| matches!(e, OrderEventAny::Filled(_)))
            );
            assert!(venue.fill_queries.lock().await.is_empty());
            venue.updates.send(initial.clone()).unwrap();
        }

        venue.updates.send(initial.clone()).unwrap();
        if partial {
            venue
                .updates
                .send(venue_update(false, submitted_ms))
                .unwrap();
            let mut remainder = venue_update(true, submitted_ms);
            remainder["data"][0]["tradeId"] = json!("90002");
            remainder["data"][0]["sz"] = json!(total_qty);
            remainder["data"][0]["accFillSz"] = json!(total_qty);
            remainder["data"][0]["fillSz"] = json!("0.00060000");
            remainder["data"][0]["feeCcy"] = json!(fee_ccy);
            remainder["data"][0]["fee"] = json!(if base_fee { "-0.000002" } else { "-0.05" });
            venue.updates.send(remainder).unwrap();
        }

        let sentinel_ms = get_atomic_clock_realtime().get_time_ms();
        let mut account: Value = serde_json::from_str(include_str!(
            "../../test_data/http_get_account_balance.json"
        ))
        .unwrap();
        account["data"][0]["details"][0]["ccy"] = json!("USD");
        account["data"][0]["uTime"] = json!(sentinel_ms.to_string());
        venue
            .updates
            .send(json!({"arg": {"channel": "account"}, "data": account["data"]}))
            .unwrap();
        wait_until_async(
            || async {
                cache
                    .borrow()
                    .account(&AccountId::from("OKX-001"))
                    .and_then(|a| a.last_event())
                    .is_some_and(|event| event.ts_event.as_u64() == sentinel_ms * 1_000_000)
            },
            DEADLINE,
        )
        .await;

        handle.stop();
    };

    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();

    msgbus::unsubscribe_any(report_topic.into(), &report_handler);
    msgbus::unsubscribe_any(exhaustion_topic.into(), &exhaustion_handler);
    result.unwrap();
    let fill_queries = venue.fill_queries.lock().await;
    let cache = cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from(total_qty));
    assert_eq!(order.quantity(), Quantity::from(total_qty));

    let expected_ids = if partial {
        vec![TradeId::from("90001"), TradeId::from("90002")]
    } else {
        vec![TradeId::from("90001")]
    };

    assert_eq!(order.trade_ids(), expected_ids.iter().collect::<Vec<_>>());
    assert_eq!(
        order.commissions().get(&if base_fee {
            Currency::BTC()
        } else {
            Currency::USD()
        }),
        Some(&Money::from(if base_fee {
            "0.000002 BTC"
        } else {
            "0.05 USD"
        }))
    );
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(
        positions[0].quantity,
        Quantity::from(if base_fee { "0.00099800" } else { total_qty })
    );
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, OrderEventAny::Filled(_)))
            .count(),
        if partial { 2 } else { 1 }
    );
    assert!(fill_queries.is_empty());
    assert_eq!(reports.get_messages().len(), 1);
}

#[rstest]
#[case::full(false, false)]
#[case::partial(true, false)]
#[case::metadata_gap(true, true)]
#[tokio::test]
async fn reconciliation_recovers_missing_private_fills(
    #[case] partial: bool,
    #[case] metadata_gap: bool,
    #[values(
        ("BTC", "-0.0000008", "-0.000002", "-0.0000012", "0.000002 BTC", "0.00099800"),
        ("USD", "-0.02", "-0.05", "-0.03", "0.05 USD", "0.00100000")
    )]
    fees: (&str, &str, &str, &str, &str, &str),
    #[values(false, true)] delayed_history: bool,
) {
    let (addr, venue) = start_venue(None).await;
    let events = Rc::new(RefCell::new(Vec::new()));

    let mut node = build_node_with_config(
        addr,
        Rc::clone(&events),
        Some(QueryOn::Accepted),
        LiveExecutionEngineConfig {
            reconciliation: false,
            open_check_interval_secs: Some(0.05),
            open_check_threshold_ms: 0,
            open_check_open_only: false,
            submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
            ..Default::default()
        },
    );

    let cache = Rc::clone(&node.kernel().cache);
    let handle = node.handle();
    let now = get_atomic_clock_realtime().get_time_ms();

    let quantity = if partial { "0.00040000" } else { "0.00100000" };

    let (currency, partial_fee, full_fee, remainder_fee, expected_fee, expected_position_qty) =
        fees;

    let fee = if partial { partial_fee } else { full_fee };

    let mut initial = venue_update(true, now);
    let order = &mut initial["data"][0];
    order["tradeId"] = json!("90001");
    order["state"] = json!(if partial {
        "partially_filled"
    } else {
        "filled"
    });
    order["fillSz"] = json!(quantity);
    order["accFillSz"] = json!(quantity);
    order["fee"] = json!(fee);
    order["feeCcy"] = json!(currency);
    order["fillFee"] = json!(fee);
    order["fillFeeCcy"] = json!(currency);
    let mut status = initial.clone();
    let mut fills = vec![fill_detail(quantity, fee, currency, "90001", now)];

    if metadata_gap {
        status["data"][0]["state"] = json!("filled");
        status["data"][0]["accFillSz"] = json!("0.00100000");
        status["data"][0]["fee"] = json!(full_fee);
        fills.push(fill_detail(
            "0.00060000",
            remainder_fee,
            currency,
            "90002",
            now + 1,
        ));
        *venue.hold_history.lock().await = true;
    }

    *venue.order_response.lock().await =
        Some(json!({"code": "0", "msg": "", "data": status["data"]}));

    if !delayed_history {
        *venue.fill_response.lock().await = fills.clone();
    }

    let driver = async {
        wait_until_async(
            || async { !venue.submissions.lock().await.is_empty() },
            DEADLINE,
        )
        .await;
        venue.updates.send(venue_update(false, now)).unwrap();
        if metadata_gap {
            venue.history_started.notified().await;
            let mut incomplete = initial.clone();
            incomplete["data"][0]["fee"] = Value::Null;
            venue.updates.send(incomplete).unwrap();
            let mut next = venue_update(true, now);
            next["data"][0]["tradeId"] = json!("90002");
            next["data"][0]["fillSz"] = json!("0.00060000");
            next["data"][0]["feeCcy"] = json!(currency);
            next["data"][0]["fee"] = json!(full_fee);
            next["data"][0]["fillFee"] = Value::Null;
            venue.updates.send(next).unwrap();
            let sentinel = get_atomic_clock_realtime().get_time_ms();
            let mut account: Value = serde_json::from_str(include_str!(
                "../../test_data/http_get_account_balance.json"
            ))
            .unwrap();
            account["data"][0]["details"][0]["ccy"] = json!("USD");
            account["data"][0]["uTime"] = json!(sentinel.to_string());
            venue
                .updates
                .send(json!({"arg": {"channel": "account"}, "data": account["data"]}))
                .unwrap();
            wait_until_async(
                || async {
                    cache
                        .borrow()
                        .account(&AccountId::from("OKX-001"))
                        .and_then(|a| a.last_event())
                        .is_some_and(|e| e.ts_event.as_u64() == sentinel * 1_000_000)
                },
                DEADLINE,
            )
            .await;

            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|e| matches!(e, OrderEventAny::Filled(_)))
            );
            venue.history_release.notify_one();
        }

        if delayed_history {
            wait_until_async(
                || async { venue.fill_queries.lock().await.len() >= 2 },
                DEADLINE,
            )
            .await;
            assert_eq!(
                cache
                    .borrow()
                    .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                    .unwrap()
                    .filled_qty(),
                Quantity::from("0.00000000")
            );
            assert!(
                !events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, OrderEventAny::Filled(_)))
            );
            *venue.fill_response.lock().await = fills;
        }

        wait_until_async(
            || async {
                cache
                    .borrow()
                    .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                    .unwrap()
                    .filled_qty()
                    == Quantity::from(if metadata_gap { "0.00100000" } else { quantity })
            },
            DEADLINE,
        )
        .await;

        if partial && !metadata_gap {
            let mut remainder = venue_update(true, now);
            let order = &mut remainder["data"][0];
            order["tradeId"] = json!("90002");
            order["fillSz"] = json!("0.00060000");
            order["feeCcy"] = json!(currency);
            order["fee"] = json!(full_fee);
            order["fillFeeCcy"] = json!(currency);
            order["fillFee"] = json!(remainder_fee);
            venue.updates.send(remainder).unwrap();
        }

        venue.updates.send(initial).unwrap();
        let sentinel = get_atomic_clock_realtime().get_time_ms();
        let mut account: Value = serde_json::from_str(include_str!(
            "../../test_data/http_get_account_balance.json"
        ))
        .unwrap();
        account["data"][0]["details"][0]["ccy"] = json!("USD");
        account["data"][0]["uTime"] = json!(sentinel.to_string());
        venue
            .updates
            .send(json!({"arg": {"channel": "account"}, "data": account["data"]}))
            .unwrap();
        wait_until_async(
            || async {
                cache
                    .borrow()
                    .account(&AccountId::from("OKX-001"))
                    .and_then(|account| account.last_event())
                    .is_some_and(|event| event.ts_event.as_u64() == sentinel * 1_000_000)
            },
            DEADLINE,
        )
        .await;

        handle.stop();
    };

    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();

    result.unwrap();
    let cache = cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("0.00100000"));

    let expected_ids = if partial {
        vec![TradeId::from("90001"), TradeId::from("90002")]
    } else {
        vec![TradeId::from("90001")]
    };

    assert_eq!(order.trade_ids(), expected_ids.iter().collect::<Vec<_>>());
    let expected_commission = Money::from(expected_fee);
    assert_eq!(
        order.commissions().get(&expected_commission.currency),
        Some(&expected_commission),
    );
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from(expected_position_qty));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        if partial { 2 } else { 1 }
    );
}

#[tokio::test]
async fn reconciliation_preserves_amendment_during_fill_collection() {
    let (addr, venue) = start_venue(None).await;
    let events = Rc::new(RefCell::new(Vec::new()));

    let mut node = build_node_with_config(
        addr,
        Rc::clone(&events),
        None,
        LiveExecutionEngineConfig {
            reconciliation: false,
            open_check_interval_secs: Some(0.05),
            open_check_threshold_ms: 0,
            open_check_open_only: false,
            ..Default::default()
        },
    );

    let cache = Rc::clone(&node.kernel().cache);
    let handle = node.handle();
    let now = get_atomic_clock_realtime().get_time_ms();
    let mut initial = venue_update(true, now);
    initial["data"][0]["state"] = json!("partially_filled");
    initial["data"][0]["accFillSz"] = json!("0.00040000");
    initial["data"][0]["fillSz"] = json!("0.00040000");
    initial["data"][0]["fee"] = json!("-0.02");
    *venue.order_response.lock().await =
        Some(json!({"code": "0", "msg": "", "data": initial["data"]}));
    *venue.fill_response.lock().await = vec![
        fill_detail("0.00040000", "-0.02", "USD", "90001", now),
        fill_detail("0.00060000", "-0.03", "USD", "90002", now + 1),
    ];
    *venue.hold_history.lock().await = true;

    let driver = async {
        wait_until_async(
            || async { !venue.submissions.lock().await.is_empty() },
            DEADLINE,
        )
        .await;
        venue.updates.send(venue_update(false, now)).unwrap();
        tokio::time::timeout(DEADLINE, venue.history_started.notified())
            .await
            .unwrap();
        let mut amendment = venue_update(false, now);
        amendment["data"][0]["sz"] = json!("0.00200000");
        amendment["data"][0]["px"] = json!("51000.0");
        amendment["data"][0]["uTime"] =
            json!((get_atomic_clock_realtime().get_time_ms() + 1).to_string());
        venue.updates.send(amendment).unwrap();
        wait_until_async(
            || async {
                cache
                    .borrow()
                    .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                    .is_some_and(|order| {
                        order.quantity() == Quantity::from("0.00200000")
                            && order.price() == Some(Price::from("51000.0"))
                    })
            },
            DEADLINE,
        )
        .await;

        venue.history_release.notify_one();
        wait_until_async(
            || async {
                cache
                    .borrow()
                    .order(&ClientOrderId::from(CLIENT_ORDER_ID))
                    .unwrap()
                    .filled_qty()
                    == Quantity::from("0.00100000")
            },
            DEADLINE,
        )
        .await;

        handle.stop();
    };

    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .unwrap();

    result.unwrap();
    let cache = cache.borrow();
    let order = cache.order(&ClientOrderId::from(CLIENT_ORDER_ID)).unwrap();
    assert_eq!(order.quantity(), Quantity::from("0.00200000"));
    assert_eq!(order.price(), Some(Price::from("51000.0")));
    assert_eq!(order.filled_qty(), Quantity::from("0.00100000"));
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(
        order.trade_ids(),
        vec![&TradeId::from("90001"), &TradeId::from("90002")]
    );
    assert_eq!(
        order.commissions().get(&Currency::USD()),
        Some(&Money::from("0.05 USD"))
    );
    assert_eq!(
        cache.positions_open(None, None, None, None, None)[0].quantity,
        Quantity::from("0.00100000")
    );
}

fn fill_detail(qty: &str, fee: &str, fee_ccy: &str, trade_id: &str, ts: u64) -> Value {
    let mut fill: Value =
        serde_json::from_str(include_str!("../../test_data/http_transaction_detail.json")).unwrap();

    for (key, value) in [
        ("instId", "BTC-USD"),
        ("ordId", VENUE_ORDER_ID),
        ("clOrdId", ""),
        ("fillSz", qty),
        ("fillPx", "50000.0"),
        ("feeCcy", fee_ccy),
        ("fee", fee),
        ("tradeId", trade_id),
    ] {
        fill[key] = json!(value);
    }

    fill["billId"] = json!(format!("1{trade_id}"));
    fill["ts"] = json!(ts.to_string());
    fill
}
