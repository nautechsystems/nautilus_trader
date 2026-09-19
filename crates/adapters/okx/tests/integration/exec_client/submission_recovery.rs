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

//! Real OKX adapter and LiveNode regression for ambiguous WebSocket submissions.

use nautilus_common::{
    actor::DataActor,
    logging::logger::LoggerConfig,
    msgbus::{self, MessagingSwitchboard, stubs::get_any_saving_handler},
};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{
        LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig,
        SubmittedOrderExhaustionPolicy,
    },
    execution::submission::SubmissionRecoveryExhausted,
};
use nautilus_okx::factories::OKXExecutionClientFactory;
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};

use super::*;

struct AmbiguousVenue {
    code: &'static str,
    submits: AtomicUsize,
    queries: AtomicUsize,
    messages: tokio::sync::broadcast::Sender<serde_json::Value>,
}

async fn ambiguous_ws(ws: WebSocketUpgrade, State(state): State<Arc<AmbiguousVenue>>) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        let mut messages = state.messages.subscribe();

        loop {
            let response = tokio::select! {
                frame = socket.next() => {
                    let Some(Ok(Message::Text(text))) = frame else { break; };
                    if text == "ping" {
                        if socket.send(Message::Text("pong".into())).await.is_err() { break; }
                        continue;
                    }
                    let request: serde_json::Value = serde_json::from_str(&text).unwrap();
                    match request["op"].as_str() {
                        Some("login") => json!({"event": "login", "code": "0", "msg": "", "connId": "recovery-test"}),
                        Some("order") => {
                            state.submits.fetch_add(1, Ordering::Relaxed);
                            json!({"id": request["id"], "op": "order", "code": "1", "msg": "",
                                "data": [{"clOrdId": request["args"][0]["clOrdId"], "ordId": "",
                                    "sCode": state.code, "sMsg": "Outcome is unknown"}]})
                        }
                        Some("subscribe") => json!({"event": "subscribe", "arg": request["args"][0], "connId": "recovery-test"}),
                        _ => continue,
                    }
                }
                message = messages.recv() => match message { Ok(message) => message, Err(_) => break },
            };

            if socket.send(Message::Text(response.to_string().into())).await.is_err() { break; }
        }
    })
}

#[derive(Debug)]
struct AmbiguousSubmissionStrategy {
    core: StrategyCore,
    order: OrderAny,
}

impl DataActor for AmbiguousSubmissionStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.submit_order(self.order.clone(), None, None, None)
    }
}
nautilus_strategy!(AmbiguousSubmissionStrategy);

#[rstest]
#[case::timeout_default("50004", false, false)]
#[case::processing_default("51149", false, false)]
#[case::timeout_retained("50004", true, false)]
#[case::processing_retained("51149", true, false)]
#[case::timeout_late_fill("50004", true, true)]
#[case::processing_late_fill("51149", true, true)]
#[case::timeout_default_late_fill("50004", false, true)]
#[case::processing_default_late_fill("51149", false, true)]
#[tokio::test]
async fn ambiguous_submission_recovery_through_okx_node(
    #[case] code: &'static str,
    #[case] retain: bool,
    #[case] late_fill: bool,
) {
    let (messages, _) = tokio::sync::broadcast::channel(16);
    let state = Arc::new(AmbiguousVenue {
        code,
        submits: AtomicUsize::new(0),
        queries: AtomicUsize::new(0),
        messages,
    });
    let query_state = Arc::clone(&state);
    let router = create_exec_test_router()
        .route(
            "/ws/v5/private",
            get(ambiguous_ws).with_state(Arc::clone(&state)),
        )
        .route(
            "/ws/v5/business",
            get(ambiguous_ws).with_state(Arc::clone(&state)),
        )
        .route(
            "/api/v5/public/instruments",
            get(|| async { Json(load_test_data("http_get_instruments_swap.json")) }),
        )
        .route(
            "/api/v5/account/trade-fee",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})) }),
        )
        .route(
            "/api/v5/trade/order",
            get(move || {
                let state = Arc::clone(&query_state);
                async move {
                    state.queries.fetch_add(1, Ordering::Relaxed);
                    Json(json!({"code": "51603", "msg": "Order does not exist", "data": []}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 20,
            inflight_check_threshold_ms: 100,
            inflight_check_retries: 3,
            ..Default::default()
        },
        risk_engine: LiveRiskEngineConfig {
            bypass: true,
            ..Default::default()
        },
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    if retain {
        config.exec_engine.submitted_order_exhaustion_policy =
            SubmittedOrderExhaustionPolicy::RetainUnresolved;
    }

    let exec_config = OKXExecutionClientConfig {
        account_id: AccountId::from("OKX-001"),
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/ws/v5/private")),
        base_url_ws_business: Some(format!("ws://{addr}/ws/v5/business")),
        api_key: Some("test_key".into()),
        api_secret: Some("test_secret".into()),
        api_passphrase: Some("test_passphrase".into()),
        instrument_types: vec![OKXInstrumentType::Swap],
        max_retries: 0,
        ..Default::default()
    };
    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .add_exec_client(
            Some("OKX".into()),
            Box::new(OKXExecutionClientFactory::new()),
            Box::new(exec_config),
        )
        .unwrap()
        .build()
        .unwrap();
    let instrument = query_order_instrument();
    let strategy_id = StrategyId::from("RECOVERY-001");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(node.trader_id())
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("OAMBIGUOUS001"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.01"))
        .price(Price::from("100.00"))
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .build();
    let client_order_id = order.client_order_id();
    let cache = node.kernel().cache();
    let exec_engine = node.kernel().exec_engine().clone();
    cache.borrow_mut().add_instrument(instrument).unwrap();
    node.add_strategy(AmbiguousSubmissionStrategy {
        core: StrategyCore::new(StrategyConfig {
            strategy_id: Some(strategy_id),
            ..Default::default()
        }),
        order,
    })
    .unwrap();
    let (handler, notifications) = get_any_saving_handler::<SubmissionRecoveryExhausted>(None);
    msgbus::subscribe_any(
        MessagingSwitchboard::submission_recovery_exhausted_topic().into(),
        handler,
        None,
    );
    let handle = node.handle();
    let driver = async {
        wait_until_async(
            || async {
                if retain {
                    !notifications.get_messages().is_empty()
                } else {
                    cache
                        .borrow()
                        .order(&client_order_id)
                        .is_some_and(|order| order.status() == OrderStatus::Rejected)
                }
            },
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(state.submits.load(Ordering::Relaxed), 1);
        assert_eq!(state.queries.load(Ordering::Relaxed), 2);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(state.queries.load(Ordering::Relaxed), 2);

        if late_fill {
            let mut message = load_test_data("ws_orders.json");
            let row = &mut message["data"][0];
            row["instId"] = json!("ETH-USDT-SWAP");
            row["clOrdId"] = json!(client_order_id.as_str());
            row["ordId"] = json!("4152001");
            row["state"] = json!("filled");
            row["side"] = json!("buy");
            row["ordType"] = json!("limit");
            row["sz"] = json!("0.01");
            row["px"] = json!("100");
            row["avgPx"] = json!("100");
            row["accFillSz"] = json!("0.01");
            row["fillSz"] = json!("0.01");
            row["fillPx"] = json!("100");
            row["tradeId"] = json!("4152002");
            row["fee"] = json!("0");
            row["fillFee"] = json!("0");
            row["feeCcy"] = json!("USDT");
            row["fillFeeCcy"] = json!("USDT");
            let ts = get_atomic_clock_realtime().get_time_ms().to_string();
            for field in ["cTime", "uTime", "fillTime"] {
                row[field] = json!(ts);
            }
            let events_before_fill = exec_engine.borrow().event_count();
            state.messages.send(message.clone()).unwrap();
            wait_until_async(
                || async {
                    exec_engine.borrow().event_count() > events_before_fill
                        && (!retain
                            || cache.borrow().order(&client_order_id).unwrap().status()
                                == OrderStatus::Filled)
                },
                Duration::from_secs(5),
            )
            .await;
            state.messages.send(message).unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        handle.stop();
    };
    let (result, ()) = tokio::join!(node.run(), driver);
    server.abort();

    if retain && !late_fill {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Incomplete submission recovery")
        );
        assert_eq!(
            node.exec_manager().unresolved_submission_ids(),
            vec![client_order_id]
        );
    } else {
        result.unwrap();
    }
    assert_eq!(notifications.get_messages().len(), usize::from(retain));
    let order = cache.borrow().order_owned(&client_order_id).unwrap();
    assert_eq!(
        order.status(),
        if late_fill && retain {
            OrderStatus::Filled
        } else if retain {
            OrderStatus::Submitted
        } else {
            OrderStatus::Rejected
        }
    );
    assert_eq!(state.submits.load(Ordering::Relaxed), 1);
    if late_fill {
        assert_eq!(
            order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            usize::from(retain)
        );
        assert_eq!(order.filled_qty().is_zero(), !retain);
        assert!(node.exec_manager().unresolved_submission_ids().is_empty());
    }
}
