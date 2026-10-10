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

//! Integration tests for the Kraken execution client.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{
        Query, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
    routing::{any, get},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_kraken::{
    common::{
        consts::{KRAKEN_CLIENT_ID, KRAKEN_VENUE},
        enums::{KrakenEnvironment, KrakenProductType},
    },
    config::KrakenExecutionClientConfig,
    execution::{KrakenFuturesExecutionClient, KrakenSpotExecutionClient},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId, Symbol,
        TradeId, TraderId, VenueOrderId,
    },
    instruments::{CurrencyPair, InstrumentAny},
    orders::{
        LimitOrder, Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs,
    },
    position::Position,
    reports::OrderStatusReport,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Default)]
enum SingleCancelResponse {
    #[default]
    Success,
    AmbiguousFailure,
    NonOrderApiError,
    StructuredReject,
}

#[derive(Debug, Clone, Copy, Default)]
enum BatchCancelResponse {
    #[default]
    Success,
    WholeFailure,
    Mixed,
}

#[derive(Debug, Clone, Copy, Default)]
enum OrderCommandResponse {
    #[default]
    Success,
    AmbiguousFailure,
    StructuredReject,
    UnknownStatus,
    IocWouldNotExecute,
}

#[derive(Debug, Clone, Copy, Default)]
enum BatchSubmitResponse {
    #[default]
    Success,
    WholeFailure,
    Mixed,
    UnknownStatus,
}

#[derive(Debug, Clone, Copy, Default)]
struct CommandResponses {
    submit: OrderCommandResponse,
    modify: OrderCommandResponse,
    batch_submit: BatchSubmitResponse,
    single_cancel: SingleCancelResponse,
    batch_cancel: BatchCancelResponse,
    cancel_all: BatchCancelResponse,
}

#[derive(Clone)]
struct TestServerState {
    command_responses: Arc<tokio::sync::Mutex<CommandResponses>>,
    submit_request_count: Arc<AtomicUsize>,
    modify_request_count: Arc<AtomicUsize>,
    batch_submit_request_count: Arc<AtomicUsize>,
    cancel_request_count: Arc<AtomicUsize>,
    batch_cancel_request_count: Arc<AtomicUsize>,
    cancel_all_request_count: Arc<AtomicUsize>,
    /// Last raw body posted to a batch-cancel endpoint, for asserting the submitted IDs.
    last_batch_cancel_body: Arc<tokio::sync::Mutex<Option<String>>>,
    collection_request_ts: Arc<tokio::sync::Mutex<Option<UnixNanos>>>,
    orders_status_response: Arc<tokio::sync::Mutex<Option<String>>>,
    orders_status_request_body: Arc<tokio::sync::Mutex<Option<String>>>,
    fills_response: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/derivatives/api/v3/openorders` returns this JSON.
    futures_open_orders_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Served by `/derivatives/api/v3/openorders` one per request, ahead of the override.
    futures_open_orders_sequence: Arc<tokio::sync::Mutex<VecDeque<String>>>,
    /// When set, `/derivatives/api/v3/openpositions` returns this JSON.
    futures_open_positions_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/api/history/v3/orders` returns this JSON; otherwise an empty page.
    futures_order_history_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/api/history/v3/orders` answers with this status instead of `200 OK`.
    futures_order_history_status: Arc<tokio::sync::Mutex<Option<StatusCode>>>,
    /// When set, `/0/private/OpenPositions` returns this JSON.
    spot_open_positions_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/0/private/TradesHistory` returns this JSON once, then empty pages.
    trades_history_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When true, the `TradesHistory` override is served on every request instead of once,
    /// mimicking a venue that never returns an empty page.
    trades_history_repeat: Arc<AtomicBool>,
    /// Counts `/0/private/TradesHistory` requests, so a test can assert the exact page count.
    trades_history_request_count: Arc<AtomicUsize>,
    /// When set, `/0/private/ClosedOrders` returns this JSON once, then empty pages.
    closed_orders_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When true, the `ClosedOrders` override is served on every request instead of once.
    closed_orders_repeat: Arc<AtomicBool>,
    /// Counts `/0/private/ClosedOrders` requests, so a test can assert the exact page count.
    closed_orders_request_count: Arc<AtomicUsize>,
    ws_message_tx: tokio::sync::broadcast::Sender<String>,
}

impl Default for TestServerState {
    fn default() -> Self {
        let (ws_message_tx, _) = tokio::sync::broadcast::channel(8);
        Self {
            command_responses: Arc::new(tokio::sync::Mutex::new(CommandResponses::default())),
            spot_open_positions_json: Arc::new(tokio::sync::Mutex::new(None)),
            trades_history_json: Arc::new(tokio::sync::Mutex::new(None)),
            trades_history_repeat: Arc::new(AtomicBool::new(false)),
            trades_history_request_count: Arc::new(AtomicUsize::new(0)),
            closed_orders_json: Arc::new(tokio::sync::Mutex::new(None)),
            closed_orders_repeat: Arc::new(AtomicBool::new(false)),
            closed_orders_request_count: Arc::new(AtomicUsize::new(0)),
            submit_request_count: Arc::new(AtomicUsize::new(0)),
            modify_request_count: Arc::new(AtomicUsize::new(0)),
            batch_submit_request_count: Arc::new(AtomicUsize::new(0)),
            cancel_request_count: Arc::new(AtomicUsize::new(0)),
            batch_cancel_request_count: Arc::new(AtomicUsize::new(0)),
            cancel_all_request_count: Arc::new(AtomicUsize::new(0)),
            last_batch_cancel_body: Arc::new(tokio::sync::Mutex::new(None)),
            collection_request_ts: Arc::new(tokio::sync::Mutex::new(None)),
            orders_status_response: Arc::new(tokio::sync::Mutex::new(None)),
            orders_status_request_body: Arc::new(tokio::sync::Mutex::new(None)),
            fills_response: Arc::new(tokio::sync::Mutex::new(None)),
            futures_open_orders_json: Arc::new(tokio::sync::Mutex::new(None)),
            futures_open_orders_sequence: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            futures_open_positions_json: Arc::new(tokio::sync::Mutex::new(None)),
            futures_order_history_json: Arc::new(tokio::sync::Mutex::new(None)),
            futures_order_history_status: Arc::new(tokio::sync::Mutex::new(None)),
            ws_message_tx,
        }
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_test_data(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|e| panic!("failed to read {filename}: {e}"))
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    let mut ws_message_rx = state.ws_message_tx.subscribe();

    loop {
        let message = tokio::select! {
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                message
            }
            message = ws_message_rx.recv() => {
                let Ok(message) = message else { continue };
                if socket.send(Message::Text(message.into())).await.is_err() {
                    break;
                }
                continue;
            }
        };

        match message {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                if payload.get("event").and_then(|v| v.as_str()) == Some("challenge") {
                    let response = json!({
                        "event": "challenge",
                        "message": "server-challenge",
                    });

                    if socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Message::Ping(data) => {
                let sent = socket.send(Message::Pong(data)).await;
                if sent.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn handle_http_request(State(state): State<TestServerState>, req: Request) -> Response {
    let path = req.uri().path().to_string();

    if matches!(
        path.as_str(),
        "/derivatives/api/v3/openorders" | "/0/private/OpenOrders"
    ) {
        *state.collection_request_ts.lock().await = Some(get_atomic_clock_realtime().get_time_ns());
    }

    match path.as_str() {
        "/health" => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("OK"))
            .unwrap(),
        "/derivatives/api/v3/instruments" => {
            let mut data: Value =
                serde_json::from_str(&load_test_data("http_futures_instruments.json")).unwrap();

            if !cfg!(feature = "high-precision") {
                data["instruments"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|instrument| instrument["symbol"] != "PF_PEPEUSD");
            }
            json_response(data.to_string())
        }
        "/derivatives/api/v3/accounts" => {
            json_response(r#"{"result":"success","accounts":{}}"#.to_string())
        }
        "/derivatives/api/v3/openorders" => {
            if let Some(response) = state.futures_open_orders_sequence.lock().await.pop_front() {
                return json_response(response);
            }
            let response = state.futures_open_orders_json.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| r#"{"result":"success","openOrders":[]}"#.to_string()),
            )
        }
        "/derivatives/api/v3/orders/status" => {
            let body = to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
            *state.orders_status_request_body.lock().await =
                Some(String::from_utf8_lossy(&body).to_string());
            let response = state.orders_status_response.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| r#"{"result":"success","orders":[]}"#.to_string()),
            )
        }
        "/derivatives/api/v3/openpositions" => {
            let response = state.futures_open_positions_json.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| r#"{"result":"success","openPositions":[]}"#.to_string()),
            )
        }
        "/derivatives/api/v3/fills" => {
            let response = state.fills_response.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| r#"{"result":"success","fills":[]}"#.to_string()),
            )
        }
        "/api/history/v3/orders" => {
            let body = state
                .futures_order_history_json
                .lock()
                .await
                .clone()
                .unwrap_or_else(|| r#"{"elements":[]}"#.to_string());

            match *state.futures_order_history_status.lock().await {
                Some(status) => Response::builder()
                    .status(status)
                    .body(Body::from(body))
                    .unwrap(),
                None => json_response(body),
            }
        }
        "/derivatives/api/v3/sendorder" => {
            state.submit_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.submit {
                OrderCommandResponse::Success => json_response(
                    r#"{"result":"success","sendStatus":{"status":"placed","order_id":"F-SUBMIT"}}"#
                        .to_string(),
                ),
                OrderCommandResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("submit failed"))
                    .unwrap(),
                OrderCommandResponse::StructuredReject => json_response(
                    r#"{"result":"error","error":"insufficientAvailableFunds","sendStatus":{"status":"insufficientAvailableFunds"}}"#
                        .to_string(),
                ),
                OrderCommandResponse::UnknownStatus => json_response(
                    r#"{"result":"success","sendStatus":{"status":"processing","order_id":"F-SUBMIT"}}"#
                        .to_string(),
                ),
                OrderCommandResponse::IocWouldNotExecute => json_response(
                    r#"{"result":"success","sendStatus":{"status":"iocWouldNotExecute"}}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/editorder" => {
            state.modify_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.modify {
                OrderCommandResponse::Success => json_response(
                    r#"{"result":"success","editStatus":{"status":"edited","order_id":"F-MODIFY"}}"#
                        .to_string(),
                ),
                OrderCommandResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("modify failed"))
                    .unwrap(),
                OrderCommandResponse::StructuredReject => json_response(
                    r#"{"result":"error","editStatus":{"status":"notFound","order_id":"F-MODIFY"}}"#
                        .to_string(),
                ),
                OrderCommandResponse::UnknownStatus
                | OrderCommandResponse::IocWouldNotExecute => json_response(
                    r#"{"result":"success","editStatus":{"status":"processing","order_id":"F-MODIFY"}}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/cancelorder" => {
            state.cancel_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.single_cancel {
                SingleCancelResponse::Success => json_response(
                    r#"{"result":"success","cancelStatus":{"status":"cancelled","order_id":"V-SINGLE"}}"#
                        .to_string(),
                ),
                SingleCancelResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("cancel failed"))
                    .unwrap(),
                SingleCancelResponse::NonOrderApiError => Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(Body::from("rate limit exceeded"))
                    .unwrap(),
                SingleCancelResponse::StructuredReject => json_response(
                    r#"{"result":"error","cancelStatus":{"status":"notFound","order_id":"V-SINGLE"}}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/batchorder" => {
            let body = to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
            if String::from_utf8_lossy(&body).contains(r#""order":"send""#) {
                state
                    .batch_submit_request_count
                    .fetch_add(1, Ordering::Relaxed);
                return match state.command_responses.lock().await.batch_submit {
                    BatchSubmitResponse::Success => json_response(
                        r#"{"result":"success","batchStatus":[{"order_tag":"0","status":"placed","order_id":"F-BATCH-0"},{"order_tag":"1","status":"placed","order_id":"F-BATCH-1"}]}"#
                            .to_string(),
                    ),
                    BatchSubmitResponse::WholeFailure => Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("batch submit failed"))
                        .unwrap(),
                    BatchSubmitResponse::Mixed => json_response(
                        r#"{"result":"success","batchStatus":[{"order_tag":"1","status":"insufficientAvailableFunds"},{"order_tag":"0","status":"placed","order_id":"F-BATCH-0"}]}"#
                            .to_string(),
                    ),
                    BatchSubmitResponse::UnknownStatus => json_response(
                        r#"{"result":"success","batchStatus":[{"order_tag":"0","status":"processing"},{"order_tag":"1","status":"processing"}]}"#
                            .to_string(),
                    ),
                };
            }

            state
                .batch_cancel_request_count
                .fetch_add(1, Ordering::Relaxed);
            *state.last_batch_cancel_body.lock().await =
                Some(String::from_utf8_lossy(&body).to_string());

            match state.command_responses.lock().await.batch_cancel {
                BatchCancelResponse::Success => json_response(
                    r#"{"result":"success","batchStatus":[{"orderId":"V-BATCH-1","status":"cancelled"},{"orderId":"V-BATCH-2","status":"cancelled"}]}"#
                        .to_string(),
                ),
                BatchCancelResponse::WholeFailure => json_response(
                    r#"{"result":"error","error":"batch failed","batchStatus":[]}"#.to_string(),
                ),
                BatchCancelResponse::Mixed => json_response(
                    r#"{"result":"success","batchStatus":[{"orderId":"V-BATCH-OK","status":"cancelled"},{"orderId":"V-BATCH-REJECT","status":"notFound"}]}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/cancelallorders" => {
            state
                .cancel_all_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.cancel_all {
                BatchCancelResponse::Success | BatchCancelResponse::Mixed => json_response(
                    r#"{"result":"success","cancelStatus":{"status":"cancelled","cancelledOrders":[]}}"#
                        .to_string(),
                ),
                BatchCancelResponse::WholeFailure => json_response(
                    r#"{"result":"error","cancelStatus":{"status":"noOrdersToCancel","cancelledOrders":[]}}"#
                        .to_string(),
                ),
            }
        }
        "/0/public/AssetPairs" => {
            let query =
                Query::<HashMap<String, String>>::try_from_uri(req.uri()).unwrap_or_default();
            match query.get("aclass_base").map(String::as_str) {
                Some("tokenized_asset") => {
                    json_response(load_test_data("http_asset_pairs_tokenized.json"))
                }
                _ => {
                    // The order and trade fixtures also reference ETHUSDT, so the listing has to
                    // carry it for every report to resolve to an instrument.
                    let mut data: serde_json::Value =
                        serde_json::from_str(&load_test_data("http_asset_pairs.json")).unwrap();
                    let mut eth = data["result"]["XBTUSDT"].clone();
                    eth["altname"] = serde_json::json!("ETHUSDT");
                    eth["wsname"] = serde_json::json!("ETH/USDT");
                    eth["base"] = serde_json::json!("ETH");
                    eth["quote"] = serde_json::json!("USDT");
                    data["result"]["ETHUSDT"] = eth;
                    json_response(data.to_string())
                }
            }
        }
        "/0/private/TradeVolume" => json_response(
            r#"{"error":[],"result":{"fees":{"XBTUSDT":{"fee":"0.2900"},"ETHUSDT":{"fee":"0.2900"},"AAPLZUSD.EQ":{"fee":"0.1900"}},"fees_maker":{"XBTUSDT":{"fee":"0.1700"},"ETHUSDT":{"fee":"0.1700"},"AAPLZUSD.EQ":{"fee":"0.0300"}}}}"#
                .to_string(),
        ),
        "/0/private/GetWebSocketsToken" => json_response(
            r#"{"error":[],"result":{"token":"TEST-TOKEN","expires":900}}"#.to_string(),
        ),
        "/0/private/OpenOrders" => json_response(load_test_data("http_open_orders.json")),
        "/0/private/ClosedOrders" => {
            state
                .closed_orders_request_count
                .fetch_add(1, Ordering::Relaxed);
            {
                let mut guard = state.closed_orders_json.lock().await;
                if state.closed_orders_repeat.load(Ordering::Relaxed) {
                    if let Some(json) = guard.clone() {
                        return json_response(json);
                    }
                } else if let Some(json) = guard.take() {
                    // Serve once, then empty pages so the caller's pagination terminates.
                    *guard = Some(r#"{"error":[],"result":{"closed":{},"count":0}}"#.to_string());
                    return json_response(json);
                }
            }

            json_response(r#"{"error":[],"result":{"closed":{},"count":0}}"#.to_string())
        }
        "/0/private/TradesHistory" => {
            state
                .trades_history_request_count
                .fetch_add(1, Ordering::Relaxed);
            {
                let mut guard = state.trades_history_json.lock().await;
                if state.trades_history_repeat.load(Ordering::Relaxed) {
                    if let Some(json) = guard.clone() {
                        return json_response(json);
                    }
                } else if let Some(json) = guard.take() {
                    // Serve once, then empty pages so the caller's pagination terminates.
                    *guard = Some(r#"{"error":[],"result":{"trades":{},"count":0}}"#.to_string());
                    return json_response(json);
                }
            }

            let mut value: Value =
                serde_json::from_str(&load_test_data("http_trades_history.json")).unwrap();
            value["result"]["trades"] = json!({});
            value["result"]["count"] = json!(0);
            json_response(value.to_string())
        }
        "/0/private/OpenPositions" => {
            let response = state.spot_open_positions_json.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| r#"{"error":[],"result":{}}"#.to_string()),
            )
        }
        "/0/private/TradeBalance" => {
            json_response(load_test_data("http_spot_trade_balance.json"))
        }
        "/0/private/Balance" => json_response(load_test_data("http_spot_balance.json")),
        "/0/private/BalanceEx" => json_response(load_test_data("http_spot_balance_ex.json")),
        "/0/private/AddOrder" => {
            state.submit_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.submit {
                OrderCommandResponse::Success => {
                    json_response(r#"{"error":[],"result":{"txid":["S-SUBMIT"]}}"#.to_string())
                }
                OrderCommandResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("submit failed"))
                    .unwrap(),
                OrderCommandResponse::StructuredReject => {
                    json_response(r#"{"error":["EOrder:Insufficient funds"]}"#.to_string())
                }
                OrderCommandResponse::UnknownStatus | OrderCommandResponse::IocWouldNotExecute => {
                    json_response(r#"{"error":[],"result":{"txid":[]}}"#.to_string())
                }
            }
        }
        "/0/private/AmendOrder" => {
            state.modify_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.modify {
                OrderCommandResponse::Success => {
                    json_response(r#"{"error":[],"result":{"amend_id":"S-MODIFY"}}"#.to_string())
                }
                OrderCommandResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("modify failed"))
                    .unwrap(),
                OrderCommandResponse::StructuredReject => {
                    json_response(r#"{"error":["EOrder:Unknown order"]}"#.to_string())
                }
                OrderCommandResponse::UnknownStatus | OrderCommandResponse::IocWouldNotExecute => {
                    json_response(r#"{"error":[],"result":{}}"#.to_string())
                }
            }
        }
        "/0/private/AddOrderBatch" => {
            state
                .batch_submit_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.batch_submit {
                BatchSubmitResponse::Success => json_response(
                    r#"{"error":[],"result":{"orders":[{"txid":"S-BATCH-0"},{"txid":"S-BATCH-1"}]}}"#
                        .to_string(),
                ),
                BatchSubmitResponse::WholeFailure => {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("batch submit failed"))
                        .unwrap()
                }
                BatchSubmitResponse::Mixed => json_response(
                    r#"{"error":[],"result":{"orders":[{"txid":"S-BATCH-0"},{"error":"EOrder:Insufficient funds"}]}}"#
                        .to_string(),
                ),
                BatchSubmitResponse::UnknownStatus => json_response(
                    r#"{"error":[],"result":{"orders":[{},{}]}}"#.to_string(),
                ),
            }
        }
        "/0/private/CancelOrder" => {
            state.cancel_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.single_cancel {
                SingleCancelResponse::Success => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
                SingleCancelResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("cancel failed"))
                    .unwrap(),
                SingleCancelResponse::NonOrderApiError => {
                    json_response(r#"{"error":["EAPI:Rate limit exceeded"]}"#.to_string())
                }
                SingleCancelResponse::StructuredReject => {
                    json_response(r#"{"error":["EOrder:Unknown order"]}"#.to_string())
                }
            }
        }
        "/0/private/CancelOrderBatch" => {
            state
                .batch_cancel_request_count
                .fetch_add(1, Ordering::Relaxed);

            let body = to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
            *state.last_batch_cancel_body.lock().await =
                Some(String::from_utf8_lossy(&body).to_string());

            match state.command_responses.lock().await.batch_cancel {
                BatchCancelResponse::Success => {
                    json_response(r#"{"error":[],"result":{"count":2}}"#.to_string())
                }
                BatchCancelResponse::WholeFailure => {
                    json_response(r#"{"error":["EOrder:Batch failed"]}"#.to_string())
                }
                BatchCancelResponse::Mixed => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
            }
        }
        "/0/private/CancelAll" => {
            state
                .cancel_all_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.cancel_all {
                BatchCancelResponse::Success | BatchCancelResponse::Mixed => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
                BatchCancelResponse::WholeFailure => {
                    json_response(r#"{"error":["EOrder:Cancel all failed"]}"#.to_string())
                }
            }
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

fn json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/ws", get(handle_ws_upgrade))
        .fallback(any(handle_http_request))
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr).await;

    Ok((addr, state))
}

async fn wait_for_server(addr: SocketAddr) {
    let health_url = format!("http://{addr}/health");
    let http_client = HttpClient::builder().build().unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn create_test_exec_config(addr: SocketAddr) -> KrakenExecutionClientConfig {
    KrakenExecutionClientConfig {
        account_id: test_account_id(),
        api_key: "test_key".into(),
        api_secret: "c2VjcmV0".into(),
        product_type: KrakenProductType::Futures,
        environment: KrakenEnvironment::Live,
        base_url: Some(format!("http://{addr}")),
        ws_url: Some(format!("ws://{addr}/ws")),
        timeout_secs: 2,
        ..Default::default()
    }
}

fn create_test_spot_exec_config(addr: SocketAddr) -> KrakenExecutionClientConfig {
    KrakenExecutionClientConfig {
        account_id: test_account_id(),
        api_key: "test_key".into(),
        api_secret: "c2VjcmV0".into(),
        product_type: KrakenProductType::Spot,
        environment: KrakenEnvironment::Live,
        base_url: Some(format!("http://{addr}")),
        ws_url: Some(format!("ws://{addr}/ws")),
        timeout_secs: 2,
        spot_account_type: AccountType::Cash,
        use_ws_trade: false,
        ..Default::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    KrakenFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let config = create_test_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenFuturesExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

/// Builds a spot client whose request rate is not throttled.
///
/// The pagination cap test issues one request per page, which the default rate limit would make
/// far too slow to run in CI.
fn create_unthrottled_spot_execution_client(
    addr: SocketAddr,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Cash,
        None,
        cache.clone(),
    );
    let config = KrakenExecutionClientConfig {
        max_requests_per_second: Some(100_000),
        ..create_test_spot_exec_config(addr)
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenSpotExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

fn create_test_spot_execution_client(
    addr: SocketAddr,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Cash,
        None,
        cache.clone(),
    );
    let config = create_test_spot_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenSpotExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

/// Builds a spot client in margin mode with no default leverage, which is the configuration that
/// submits unleveraged orders while `OpenPositions` reports leveraged positions only.
fn create_test_spot_margin_execution_client(
    addr: SocketAddr,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let config = KrakenExecutionClientConfig {
        spot_account_type: AccountType::Margin,
        default_leverage: None,
        ..create_test_spot_exec_config(addr)
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenSpotExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

/// Builds an unstarted spot client for a config, for assertions that need no event stream.
///
/// `set_exec_event_sender` can only be called once per thread, so a test comparing several
/// configurations cannot go through the started-client helpers.
fn unstarted_spot_client(
    addr: SocketAddr,
    account_type: AccountType,
    configure: impl FnOnce(&mut KrakenExecutionClientConfig),
) -> KrakenSpotExecutionClient {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        account_type,
        None,
        cache,
    );
    let mut config = create_test_spot_exec_config(addr);
    configure(&mut config);

    KrakenSpotExecutionClient::new(core, config).unwrap()
}

/// Builds a started spot client that derives position reports from wallet balances.
fn create_test_spot_wallet_execution_client(
    addr: SocketAddr,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Cash,
        None,
        cache.clone(),
    );
    let config = KrakenExecutionClientConfig {
        use_spot_position_reports: true,
        ..create_test_spot_exec_config(addr)
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenSpotExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

fn xbtusd_spot_instrument() -> (InstrumentId, InstrumentAny) {
    let instrument_id = InstrumentId::from("XBT/USD.KRAKEN");
    let instrument = InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(Symbol::new("XXBTZUSD"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USD())
            .price_precision(1)
            .size_precision(8)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.00000001"))
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap(),
    );
    (instrument_id, instrument)
}

async fn connected_client_with_command_responses(
    responses: CommandResponses,
) -> (
    KrakenFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    TestServerState,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.command_responses.lock().await = responses;

    let (mut client, rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache);
    client.connect().await.unwrap();

    (client, rx, cache, state)
}

async fn connected_spot_client_with_command_responses(
    responses: CommandResponses,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    TestServerState,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.command_responses.lock().await = responses;

    let (mut client, rx, cache) = create_test_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    (client, rx, cache, state)
}

fn spot_trades_json(pairs: &[&str]) -> String {
    let entries: Vec<String> = pairs
        .iter()
        .enumerate()
        .map(|(i, pair)| {
            format!(
                r#""TTRADE-{i}":{{"ordertxid":"O26VBY-ISGAE-JP5TLU","postxid":"TKH2SE-M7IF5-CFI7LT","pair":"{pair}","time":1688585840.8921,"type":"buy","ordertype":"limit","price":"29500.50","cost":"14750.25","fee":"23.60","vol":"0.50000000","margin":"0.00000","misc":"","trade_id":{i},"maker":true,"ledgers":["L4UESK-KG3EQ-BJM7HJ"]}}"#
            )
        })
        .collect();
    format!(
        r#"{{"error":[],"result":{{"trades":{{{}}},"count":{}}}}}"#,
        entries.join(","),
        pairs.len()
    )
}

fn spot_closed_orders_json(pairs: &[&str]) -> String {
    let entries: Vec<String> = pairs
        .iter()
        .enumerate()
        .map(|(i, pair)| {
            format!(
                r#""OCLOSED-{i}":{{"refid":null,"userref":0,"status":"closed","reason":"User requested","opentm":1688583840.8648,"closetm":1688590000.5432,"starttm":0,"expiretm":0,"descr":{{"pair":"{pair}","type":"buy","ordertype":"limit","price":"29500.0","price2":"0","leverage":"none","order":"buy 0.50000000 {pair} @ limit 29500.0","close":""}},"vol":"0.50000000","vol_exec":"0.50000000","cost":"14750.00000","fee":"22.12500","price":"29500.0","stopprice":"0.00000","limitprice":"0.00000","misc":"","oflags":""}}"#
            )
        })
        .collect();
    format!(
        r#"{{"error":[],"result":{{"closed":{{{}}},"count":{}}}}}"#,
        entries.join(","),
        pairs.len()
    )
}

fn spot_trades_json_with_unparsable_vol(pair: &str) -> String {
    format!(
        r#"{{"error":[],"result":{{"trades":{{"TTRADE-BAD":{{"ordertxid":"O26VBY-ISGAE-JP5TLU","postxid":"TKH2SE-M7IF5-CFI7LT","pair":"{pair}","time":1688585840.8921,"type":"buy","ordertype":"limit","price":"29500.50","cost":"14750.25","fee":"23.60","vol":"not_a_number","margin":"0.00000","misc":"","trade_id":1,"maker":true,"ledgers":["L4UESK-KG3EQ-BJM7HJ"]}}}},"count":1}}}}"#
    )
}

/// A historical row that cannot be parsed makes the bounded set incomplete.
///
/// The guide counts a required row that cannot be parsed or mapped as incomplete, the same as an
/// unresolved instrument.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_incomplete_when_historical_fill_unparsable() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    // Control: the same row with a parsable volume reports complete, so the flag below is driven
    // by the parse failure rather than by the row being rejected for some other reason.
    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT"]));
    let control = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();
    assert!(control.reports_complete());
    let control_fills: usize = control.fill_reports().values().map(Vec::len).sum();
    assert_eq!(control_fills, 1);

    *state.trades_history_json.lock().await = Some(spot_trades_json_with_unparsable_vol("XBTUSDT"));
    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot.reports_complete(),
        "an unparsable historical row must mark the bounded set incomplete"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 0);
}

/// A cached unleveraged spot position must survive a bulk read that cannot report it.
///
/// Under `spot_account_type=Margin` the only source is Kraken `OpenPositions`, which reports
/// leveraged positions only. Reporting the cached position FLAT because it is absent there asserts
/// a close the venue never confirmed, and the engine acts on that by fabricating a fill.
#[rstest]
#[tokio::test]
async fn test_spot_margin_bulk_reports_leave_an_unleveraged_cached_position_alone() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, cache) = create_test_spot_margin_execution_client(addr);

    let (instrument_id, instrument) = xbtusd_spot_instrument();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    // An unleveraged buy, which `OpenPositions` never returns.
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.5"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-SPOT-001")),
        Some(PositionId::from("P-SPOT-001")),
        Some(Price::from("29500.0")),
        Some(Quantity::from("0.5")),
        Some(LiquiditySide::Taker),
        Some(Money::from("1 USD")),
        None,
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, fill.into());
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    assert!(
        reports.is_empty(),
        "an empty `OpenPositions` must not report the cached spot position flat: {reports:?}"
    );
}

/// The startup mass status must not invent a FLAT for a cached position either.
///
/// The sweep ran from two call sites; the periodic one is covered above, this is the startup one.
/// It also pins the configuration the omission matters in: with a lookback declared, the engine
/// projects a closing fill for an instrument carrying no position report as order-only, so the
/// cached position keeps its quantity and its realized PnL. That is the shared engine's documented
/// behavior for a missing report, and it is why absence must not be reported as FLAT here.
#[rstest]
#[tokio::test]
async fn test_spot_margin_startup_mass_status_adds_no_synthetic_flat() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_spot_margin_execution_client(addr);
    add_test_spot_account_to_cache(&cache);

    let (instrument_id, instrument) = xbtusd_spot_instrument();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.5"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-STARTUP-001")),
        Some(PositionId::from("P-STARTUP-001")),
        Some(Price::from("29500.0")),
        Some(Quantity::from("0.5")),
        Some(LiquiditySide::Taker),
        Some(Money::from("1 USD")),
        None,
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, fill.into());
    let cached_qty = position.quantity;
    let cached_realized = position.realized_pnl;
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    client.connect().await.unwrap();

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        mass_status.position_reports().is_empty(),
        "an empty `OpenPositions` must not put a synthetic FLAT in the startup mass status: {:?}",
        mass_status.position_reports()
    );
    assert!(
        mass_status.lookback_start().is_some(),
        "the window must be declared, which is what makes an omitted report order-only"
    );

    let cache_ref = cache.borrow();
    let cached = cache_ref
        .position(&position.id)
        .expect("the cached position must survive the read");
    assert_eq!(cached.quantity, cached_qty);
    assert_eq!(cached.realized_pnl, cached_realized);
}

/// Bulk position coverage must follow what the wallet read can actually report.
///
/// The read enumerates only pairs quoted in `spot_positions_quote_currency`, so coverage is
/// per-instrument rather than per-mode. Claiming it for every instrument would let an absent
/// report force-close a holding the read could never have reported.
#[rstest]
#[tokio::test]
async fn test_spot_bulk_position_coverage_follows_the_wallet_read() {
    let (addr, _state) = start_test_server().await.unwrap();

    // Connected, so the instruments cache holds the listing the read would enumerate.
    let (mut wallet, _rx, cache) = create_test_spot_wallet_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    wallet.connect().await.unwrap();

    assert!(
        wallet.provides_bulk_position_coverage(InstrumentId::from("BTC/USDT.KRAKEN")),
        "a pair quoted in the configured currency is enumerated by the read"
    );
    assert!(
        !wallet.provides_bulk_position_coverage(InstrumentId::from("AAPLx/USD.KRAKEN")),
        "a pair quoted in anything else is skipped by the read, so its absence proves nothing"
    );
    assert!(
        !wallet.provides_bulk_position_coverage(InstrumentId::from("SOL/USDT.KRAKEN")),
        "an instrument the listing does not hold cannot be reported either"
    );

    // The two modes that report nothing the engine could read as flat.
    let margin = unstarted_spot_client(addr, AccountType::Margin, |config| {
        config.spot_account_type = AccountType::Margin;
        config.default_leverage = None;
    });
    assert!(
        !margin.provides_bulk_position_coverage(InstrumentId::from("BTC/USDT.KRAKEN")),
        "margin mode reads `OpenPositions`, which omits unleveraged holdings"
    );

    let cash = unstarted_spot_client(addr, AccountType::Cash, |_| {});
    assert!(
        !cash.provides_bulk_position_coverage(InstrumentId::from("BTC/USDT.KRAKEN")),
        "cash mode without `use_spot_position_reports` reports no positions at all"
    );
}

fn futures_open_positions_json(symbol: &str) -> String {
    format!(
        r#"{{"result":"success","openPositions":[{{"side":"long","symbol":"{symbol}","price":27500.5,"fillTime":"2023-04-07T15:45:10.739Z","size":1000,"unrealizedFunding":0.0}}]}}"#
    )
}

fn futures_open_orders_json(order_id: &str, symbol: &str) -> String {
    futures_open_orders_json_rows(&[(order_id, symbol)])
}

/// An open-orders response with one resting 1,000-contract buy per `(order_id, symbol)`.
fn futures_open_orders_json_rows(rows: &[(&str, &str)]) -> String {
    let orders: Vec<String> = rows
        .iter()
        .map(|(order_id, symbol)| {
            format!(
                r#"{{"order_id":"{order_id}","symbol":"{symbol}","side":"buy","orderType":"lmt","limitPrice":27500.5,"unfilledSize":1000.0,"receivedTime":"2023-04-07T14:15:30.250Z","status":"untouched","filledSize":0.0,"reduceOnly":false,"lastUpdateTime":"2023-04-07T14:15:30.250Z"}}"#
            )
        })
        .collect();
    format!(
        r#"{{"result":"success","openOrders":[{}]}}"#,
        orders.join(",")
    )
}

/// An in-scope open order the client cannot resolve fails the read, as it does on spot.
///
/// Dropped from a successful return, the order would read to reconciliation as one the venue never
/// had, and a live order could be resolved as missing.
#[rstest]
#[tokio::test]
async fn test_futures_open_order_reports_error_on_unresolved_symbol() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-UNRESOLVED", "PF_UNKNOWNUSD"));

    let error = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            true,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("an unresolvable in-scope open order must fail the read");

    assert!(
        error
            .to_string()
            .contains("OpenOrders: instrument not in cache for futures symbol PF_UNKNOWNUSD"),
        "unexpected error: {error}"
    );
}

/// An in-scope position the client cannot resolve fails the read as well.
#[rstest]
#[tokio::test]
async fn test_futures_position_reports_error_on_unresolved_symbol() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_positions_json.lock().await = Some(load_test_data(
        "http_futures_open_positions_unresolved.json",
    ));

    let error = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("an unresolvable in-scope position must fail the read");

    assert!(
        error
            .to_string()
            .contains("OpenPositions: instrument not in cache for futures symbol PF_UNKNOWNUSD"),
        "unexpected error: {error}"
    );
}

/// Out of scope, an unresolvable row is skipped: a scoped read only reports its own instrument.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_open_order_read_skips_an_unresolved_row_of_another_symbol() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(load_test_data(
        "http_futures_open_orders_held_and_unresolved.json",
    ));

    let reports = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            true,
            Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("a scoped read ignores rows of other symbols, resolvable or not");

    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("V-HELD"));
}

/// The single-order lookup is scoped to the command's instrument, so an unresolvable open order on
/// another contract is out of scope for it rather than failing it.
#[rstest]
#[tokio::test]
async fn test_futures_order_status_report_lookup_is_scoped_to_its_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(load_test_data(
        "http_futures_open_orders_held_and_unresolved.json",
    ));

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        None,
        Some(VenueOrderId::from("V-HELD")),
        None,
        None,
    );

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .expect("an unresolvable order on another contract is out of scope")
        .expect("the held order is reported");

    assert_eq!(report.venue_order_id, VenueOrderId::from("V-HELD"));
}

/// The same scope for `query_order`, which reads the queried instrument's open orders.
#[rstest]
#[tokio::test]
async fn test_futures_query_order_is_scoped_to_its_instrument() {
    let (client, mut rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(load_test_data(
        "http_futures_open_orders_held_and_unresolved.json",
    ));

    client
        .query_order(QueryOrder::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("S-001"),
            InstrumentId::from("PI_XBTUSD.KRAKEN"),
            ClientOrderId::new("O-QUERY-1"),
            Some(VenueOrderId::from("V-HELD")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let event = recv_until(&mut rx, |event| matches!(event, ExecutionEvent::Report(_))).await;
    let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
        panic!("expected an order status report, received {event:?}");
    };
    assert_eq!(report.venue_order_id, VenueOrderId::from("V-HELD"));
}

/// Out of scope, an unresolvable position is skipped: a scoped read only reports its own
/// instrument.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_position_read_skips_an_unresolved_row_of_another_symbol() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_positions_json.lock().await = Some(load_test_data(
        "http_futures_open_positions_scoped_unresolved.json",
    ));

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("a scoped read ignores rows of other symbols, resolvable or not");

    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(
        reports[0].instrument_id,
        InstrumentId::from("PI_XBTUSD.KRAKEN")
    );
}

/// An in-scope position that cannot be parsed fails the read, as on the spot client; dropped, it
/// would read to reconciliation as a position the venue does not hold.
#[rstest]
#[tokio::test]
async fn test_futures_position_read_fails_on_an_unparsable_position() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_positions_json.lock().await = Some(load_test_data(
        "http_futures_open_positions_negative_size.json",
    ));

    let error = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("an in-scope position that cannot be parsed must fail the read");

    assert!(
        error
            .to_string()
            .contains("OpenPositions: failed to parse futures position PI_XBTUSD"),
        "unexpected error: {error}"
    );
}

/// One order history element in the documented shape, with `lastUpdateTimestamp` at `ts_ms`.
fn futures_history_element(
    kind: &str,
    uid: &str,
    order_uid: &str,
    tradeable: &str,
    quantity: &str,
    filled: &str,
    ts_ms: i64,
) -> String {
    futures_history_element_sided(
        kind, uid, order_uid, tradeable, "Buy", quantity, filled, ts_ms,
    )
}

/// As [`futures_history_element`], for an order in `direction` (`Buy` or `Sell`).
#[expect(clippy::too_many_arguments)]
fn futures_history_element_sided(
    kind: &str,
    uid: &str,
    order_uid: &str,
    tradeable: &str,
    direction: &str,
    quantity: &str,
    filled: &str,
    ts_ms: i64,
) -> String {
    let order = format!(
        r#"{{"uid":"{order_uid}","accountUid":"acc","tradeable":"{tradeable}","direction":"{direction}","quantity":"{quantity}","filled":"{filled}","timestamp":1680876930250,"limitPrice":"27500.5","orderType":"Limit","clientId":"","reduceOnly":false,"lastUpdateTimestamp":{ts_ms}}}"#
    );
    let payload = match kind {
        "OrderUpdated" => format!(r#"{{"newOrder":{order}}}"#),
        _ => format!(r#"{{"order":{order}}}"#),
    };
    format!(r#"{{"uid":"{uid}","timestamp":{ts_ms},"event":{{"{kind}":{payload}}}}}"#)
}

fn futures_order_history_json(elements: &[String]) -> String {
    format!(
        r#"{{"accountUid":"acc","len":{},"elements":[{}],"serverTime":"2023-04-07T16:30:45.678Z"}}"#,
        elements.len(),
        elements.join(",")
    )
}

fn history_orders_cmd() -> GenerateOrderStatusReports {
    GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false, // open_only=false, so the history read runs alongside the open-order read
        None,
        None,
        None,
        None,
        None,
    )
}

/// A history row names the contract as the venue spells it, which can differ in case from the
/// listing, so the row resolves to the listed instrument either way.
#[rstest]
#[case::mixed_case_listing("PF_AAPLxUSD", "PF_AAPLxUSD.KRAKEN")]
#[case::lowercase_row("pi_xbtusd", "PI_XBTUSD.KRAKEN")]
#[tokio::test]
async fn test_futures_order_status_reports_resolve_the_history_tradeable(
    #[case] tradeable: &str,
    #[case] expected: &str,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json(&[futures_history_element(
            "OrderPlaced",
            "e1",
            "H-CASE-1",
            tradeable,
            "2",
            "0",
            1680877245500,
        )]));

    let reports = client
        .generate_order_status_reports(&history_orders_cmd())
        .await
        .unwrap();

    assert_eq!(reports.len(), 1, "the row must resolve: {reports:?}");
    assert_eq!(reports[0].instrument_id, InstrumentId::from(expected));
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("H-CASE-1"));
}

/// The history lists every lifecycle event of an order, and each report reconciles against the
/// same cached state, so the read hands back one report per order: the latest state.
#[rstest]
#[tokio::test]
async fn test_futures_order_status_reports_fold_history_events_per_order() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element(
            "OrderPlaced",
            "e1",
            "H-FOLD-1",
            "PI_XBTUSD",
            "2",
            "0",
            1680877245500,
        ),
        futures_history_element(
            "OrderUpdated",
            "e2",
            "H-FOLD-1",
            "PI_XBTUSD",
            "3",
            "0",
            1680877245600,
        ),
        futures_history_element(
            "OrderUpdated",
            "e3",
            "H-FOLD-1",
            "PI_XBTUSD",
            "3",
            "2",
            1680877245700,
        ),
    ]));

    let reports = client
        .generate_order_status_reports(&history_orders_cmd())
        .await
        .unwrap();

    assert_eq!(reports.len(), 1, "one report per order: {reports:?}");
    let report = &reports[0];
    assert_eq!(report.venue_order_id, VenueOrderId::from("H-FOLD-1"));
    assert_eq!(report.quantity, Quantity::from("3"));
    assert_eq!(report.filled_qty, Quantity::from("2"));
    assert_eq!(report.ts_last, UnixNanos::from(1_680_877_245_700_000_000));
}

/// An order the venue still lists as open is reported from that snapshot alone; its history
/// rows describe earlier states and must not reach reconciliation beside it.
#[rstest]
#[tokio::test]
async fn test_futures_order_status_reports_keep_the_open_snapshot_over_history() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-OPEN-1", "PI_XBTUSD"));
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json(&[futures_history_element(
            "OrderPlaced",
            "e1",
            "V-OPEN-1",
            "PI_XBTUSD",
            "1000",
            "500",
            1680877245500,
        )]));

    let reports = client
        .generate_order_status_reports(&history_orders_cmd())
        .await
        .unwrap();

    assert_eq!(
        reports.len(),
        1,
        "one report for the open order: {reports:?}"
    );
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("V-OPEN-1"));
    assert_eq!(
        reports[0].filled_qty,
        Quantity::from("0"),
        "the open snapshot is the current evidence"
    );
}

/// A fills page of `PI_XBTUSD` buys, each given as `(fill_id, order_id, size, price)`.
fn futures_fills_json(fills: &[(&str, &str, &str, &str)]) -> String {
    let fills: Vec<String> = fills
        .iter()
        .map(|(fill_id, order_id, size, price)| {
            format!(
                r#"{{"fill_id":"{fill_id}","symbol":"PI_XBTUSD","side":"buy","order_id":"{order_id}","fillTime":"2023-04-07T15:20:45.500Z","size":{size},"price":{price},"fillType":"taker","fee_paid":0.0,"fee_currency":"USD"}}"#
            )
        })
        .collect();
    format!(r#"{{"result":"success","fills":[{}]}}"#, fills.join(","))
}

/// A fully filled history row for a 1,000-contract buy, limit or market.
fn filled_history_json(order_uid: &str, order_type: &str) -> String {
    futures_order_history_json(&[futures_history_element(
        "OrderUpdated",
        "e1",
        order_uid,
        "PI_XBTUSD",
        "1000",
        "1000",
        1680877245500,
    )
    .replace(
        r#""orderType":"Limit""#,
        &format!(r#""orderType":"{order_type}""#),
    )])
}

/// The two reads that reach the order history.
#[derive(Clone, Copy, Debug)]
enum HistoryRead {
    SingleOrder,
    Bulk,
}

/// Reads the report for `venue_order_id` through `read`.
async fn read_history_report(
    client: &KrakenFuturesExecutionClient,
    read: HistoryRead,
    venue_order_id: &str,
) -> anyhow::Result<Option<OrderStatusReport>> {
    let venue_order_id = VenueOrderId::from(venue_order_id);

    match read {
        HistoryRead::SingleOrder => {
            client
                .generate_order_status_report(&GenerateOrderStatusReport::new(
                    UUID4::new(),
                    UnixNanos::default(),
                    Some(test_instrument_id()),
                    None,
                    Some(venue_order_id),
                    None,
                    None,
                ))
                .await
        }
        HistoryRead::Bulk => client
            .generate_order_status_reports(&history_orders_cmd())
            .await
            .map(|reports| {
                reports
                    .into_iter()
                    .find(|report| report.venue_order_id == venue_order_id)
            }),
    }
}

/// A terminal history row carries no average price, so both reads price it as the
/// quantity-weighted average of the order's fills, not at its limit price. A market order has no
/// limit price, so its fills are its only price.
#[rstest]
#[case::filled_single_order(HistoryRead::SingleOrder, "Limit", Some("27500.5"))]
#[case::filled_bulk(HistoryRead::Bulk, "Limit", Some("27500.5"))]
#[case::market_single_order(HistoryRead::SingleOrder, "Market", None)]
#[case::market_bulk(HistoryRead::Bulk, "Market", None)]
#[tokio::test]
async fn test_futures_history_filled_report_is_priced_from_its_fills(
    #[case] read: HistoryRead,
    #[case] order_type: &str,
    #[case] expected_price: Option<&str>,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await =
        Some(filled_history_json("H-FILLED-1", order_type));
    *state.fills_response.lock().await = Some(futures_fills_json(&[
        ("f-1", "H-FILLED-1", "600", "27000.0"),
        ("f-2", "H-FILLED-1", "400", "27100.5"),
        ("f-other", "H-OTHER-1", "5", "1.0"),
    ]));

    let report = read_history_report(&client, read, "H-FILLED-1")
        .await
        .unwrap()
        .expect("the filled history row is reported");

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1000"));
    assert_eq!(report.price, expected_price.map(Price::from));
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("27040.2").unwrap()),
        "(600 x 27000.0 + 400 x 27100.5) / 1000"
    );
}

/// The same pricing holds for a canceled row with an executed quantity, which is terminal too.
#[rstest]
#[case::single_order(HistoryRead::SingleOrder)]
#[case::bulk(HistoryRead::Bulk)]
#[tokio::test]
async fn test_futures_history_canceled_partial_report_is_priced_from_its_fills(
    #[case] read: HistoryRead,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json(&[futures_history_element(
            "OrderCancelled",
            "e1",
            "H-CANCELED-1",
            "PI_XBTUSD",
            "1000",
            "600",
            1680877245500,
        )]));
    *state.fills_response.lock().await = Some(futures_fills_json(&[
        ("f-1", "H-CANCELED-1", "200", "27000.0"),
        ("f-2", "H-CANCELED-1", "400", "27100.5"),
    ]));

    let report = read_history_report(&client, read, "H-CANCELED-1")
        .await
        .unwrap()
        .expect("the canceled history row is reported");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.filled_qty, Quantity::from("600"));
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("27067").unwrap()),
        "(200 x 27000.0 + 400 x 27100.5) / 600"
    );
}

/// A terminal history row whose fills do not cover its filled quantity is deferred: the
/// single-order query fails so the engine retries it, and the bulk read leaves the order out, so
/// the engine counts it missing and resolves it through the single-order query.
#[rstest]
#[case::single_order(HistoryRead::SingleOrder)]
#[case::bulk(HistoryRead::Bulk)]
#[tokio::test]
async fn test_futures_history_filled_report_short_of_fills_is_deferred(#[case] read: HistoryRead) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await =
        Some(filled_history_json("H-SHORT-1", "Limit"));
    *state.fills_response.lock().await = Some(futures_fills_json(&[(
        "f-1",
        "H-SHORT-1",
        "600",
        "27000.0",
    )]));

    let result = read_history_report(&client, read, "H-SHORT-1").await;

    match read {
        HistoryRead::SingleOrder => {
            let error = result.expect_err("400 contracts are unpriced, so the query must fail");
            assert!(
                error
                    .to_string()
                    .contains("fills covering 600 of 1000; deferring"),
                "unexpected error: {error}"
            );
        }
        HistoryRead::Bulk => assert_eq!(
            result.expect("the bulk read succeeds without the order"),
            None,
            "the unpriced order must be left out of the bulk response"
        ),
    }
}

/// A failed fills read leaves a terminal history row unpriced: the single-order query fails, and
/// the bulk read withholds that order while still reporting the others.
#[rstest]
#[tokio::test]
async fn test_futures_history_filled_report_is_withheld_when_the_fills_read_fails() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element(
            "OrderUpdated",
            "e2",
            "H-NOFILLS-1",
            "PI_XBTUSD",
            "1000",
            "1000",
            1680877245600,
        ),
        futures_history_element(
            "OrderPlaced",
            "e1",
            "H-RESTING-1",
            "PI_XBTUSD",
            "500",
            "0",
            1680877245500,
        ),
    ]));
    *state.fills_response.lock().await =
        Some(r#"{"result":"error","error":"apiLimitExceeded"}"#.to_string());

    let error = read_history_report(&client, HistoryRead::SingleOrder, "H-NOFILLS-1")
        .await
        .expect_err("an unpriced terminal row must not be reported");
    assert!(
        error.to_string().contains("Failed to get fills"),
        "unexpected error: {error}"
    );

    let reports = client
        .generate_order_status_reports(&history_orders_cmd())
        .await
        .unwrap();
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>(),
        vec!["H-RESTING-1"],
        "the unpriced order is withheld and the rest of the read survives"
    );
}

/// Caches a 1,000-contract limit buy with venue order ID `venue_order_id` holding `fills`, each
/// given as `(trade_id, qty, price)`.
fn cache_order_with_fills(
    cache: &Rc<RefCell<Cache>>,
    venue_order_id: &str,
    fills: &[(&str, &str, &str)],
) {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(test_instrument_id())
        .client_order_id(ClientOrderId::new(format!("O-{venue_order_id}")))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000"))
        .price(Price::from("27500.5"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    mark_cached_order_submitted(cache, &order);
    set_venue_order_id_on_cached_order(cache, &order, venue_order_id);

    for (trade_id, qty, price) in fills {
        let filled = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::from(venue_order_id),
            test_account_id(),
            TradeId::from(*trade_id),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from(*qty),
            Price::from(*price),
            Currency::USD(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            None,
            None,
        );
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Filled(filled))
            .unwrap();
    }
}

/// An execution the engine inferred carries a synthetic trade ID, so it is not matched to the
/// venue fill it stands for; the venue's fills, covering the order on their own, price the report
/// instead of being added to it.
#[rstest]
#[case::single_order(HistoryRead::SingleOrder)]
#[case::bulk(HistoryRead::Bulk)]
#[tokio::test]
async fn test_futures_history_filled_report_does_not_count_an_inferred_fill_twice(
    #[case] read: HistoryRead,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    cache_order_with_fills(&cache, "H-INFERRED-1", &[("inferred-1", "600", "27500.5")]);
    *state.futures_order_history_json.lock().await =
        Some(filled_history_json("H-INFERRED-1", "Limit"));
    *state.fills_response.lock().await = Some(futures_fills_json(&[
        ("f-1", "H-INFERRED-1", "600", "27000.0"),
        ("f-2", "H-INFERRED-1", "400", "27100.5"),
    ]));

    let report = read_history_report(&client, read, "H-INFERRED-1")
        .await
        .unwrap()
        .expect("the venue's fills cover the order");

    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("27040.2").unwrap()),
        "(600 x 27000.0 + 400 x 27100.5) / 1000, without the inferred fill"
    );
}

/// A cached order whose recorded executions cover the report prices it, so the read succeeds even
/// when the fills read fails.
#[rstest]
#[case::single_order(HistoryRead::SingleOrder)]
#[case::bulk(HistoryRead::Bulk)]
#[tokio::test]
async fn test_futures_history_filled_report_is_priced_from_a_covering_cached_order(
    #[case] read: HistoryRead,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    cache_order_with_fills(
        &cache,
        "H-COVERED-1",
        &[("f-1", "600", "27000.0"), ("f-2", "400", "27100.5")],
    );
    *state.futures_order_history_json.lock().await =
        Some(filled_history_json("H-COVERED-1", "Limit"));
    *state.fills_response.lock().await =
        Some(r#"{"result":"error","error":"apiLimitExceeded"}"#.to_string());

    let report = read_history_report(&client, read, "H-COVERED-1")
        .await
        .unwrap()
        .expect("the cached order covers the report");

    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("27040.2").unwrap())
    );
}

/// Fills a cached order has recorded count toward coverage at their recorded price, and once
/// each, so an execution that has left the single fills page still prices the report.
#[rstest]
#[case::single_order_first_fill_off_page(HistoryRead::SingleOrder, false)]
#[case::single_order_first_fill_on_page(HistoryRead::SingleOrder, true)]
#[case::bulk_first_fill_off_page(HistoryRead::Bulk, false)]
#[case::bulk_first_fill_on_page(HistoryRead::Bulk, true)]
#[tokio::test]
async fn test_futures_history_filled_report_counts_the_cached_order_fills(
    #[case] read: HistoryRead,
    #[case] first_fill_on_page: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(test_instrument_id())
        .client_order_id(ClientOrderId::new("O-CACHED-1"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1000"))
        .price(Price::from("27500.5"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    mark_cached_order_submitted(&cache, &order);
    set_venue_order_id_on_cached_order(&cache, &order, "H-CACHED-1");
    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from("H-CACHED-1"),
        test_account_id(),
        TradeId::from("f-1"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("600"),
        Price::from("27000.0"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(filled))
        .unwrap();

    let mut fills = vec![("f-2", "H-CACHED-1", "400", "27100.5")];
    if first_fill_on_page {
        fills.insert(0, ("f-1", "H-CACHED-1", "600", "27000.0"));
    }
    *state.futures_order_history_json.lock().await =
        Some(filled_history_json("H-CACHED-1", "Limit"));
    *state.fills_response.lock().await = Some(futures_fills_json(&fills));

    let report = read_history_report(&client, read, "H-CACHED-1")
        .await
        .unwrap()
        .expect("the cached fills complete the coverage");

    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("27040.2").unwrap()),
        "(600 x 27000.0 + 400 x 27100.5) / 1000"
    );
}

/// A fills page of `PI_XBTUSD` fills stamped a second ago, inside any lookback, each given as
/// `(fill_id, order_id, side, size, price)`.
fn futures_recent_fills_json(fills: &[(&str, &str, &str, &str, &str)]) -> String {
    let fill_time = jiff::Timestamp::now() - jiff::Span::new().seconds(1);
    let fills: Vec<String> = fills
        .iter()
        .map(|(fill_id, order_id, side, size, price)| {
            format!(
                r#"{{"fill_id":"{fill_id}","symbol":"PI_XBTUSD","side":"{side}","order_id":"{order_id}","fillTime":"{fill_time}","size":{size},"price":{price},"fillType":"taker","fee_paid":0.0,"fee_currency":"USD"}}"#
            )
        })
        .collect();
    format!(r#"{{"result":"success","fills":[{}]}}"#, fills.join(","))
}

/// A history page holding one fully filled 1,000-contract `PI_XBTUSD` buy.
fn executed_history_json(order_uid: &str) -> String {
    futures_order_history_json(&[futures_history_element(
        "OrderUpdated",
        "e1",
        order_uid,
        "PI_XBTUSD",
        "1000",
        "1000",
        1680877245500,
    )])
}

/// Startup mass status reads the order history, and a terminal history order whose fills cover
/// its filled quantity is kept, priced from those fills rather than at its 27500.5 limit.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_executed_history_order_with_a_fill() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-EXEC"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-exec", "F-EXEC", "buy", "1000", "49000.0",
    )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    let report = snapshot
        .order_reports()
        .get(&VenueOrderId::from("F-EXEC"))
        .cloned()
        .expect("a covered execution must be kept");
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("49000").unwrap())
    );
    assert!(snapshot.reports_complete());
}

/// A terminal history order with no fill on the page is withheld and leaves the set incomplete:
/// unpriced, reconciliation would infer the execution at the order's limit price.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_an_execution_with_no_fill() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-EXEC"));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-EXEC")),
        "an execution with no covering fill must be withheld, not priced at the limit"
    );
    assert!(
        !snapshot.reports_complete(),
        "withholding a report leaves the set incomplete"
    );
}

/// Fills must cover the whole filled quantity, not merely exist, and an uncached order's fills go
/// with its withheld report.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_a_partially_covered_execution() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-PART"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-part-1", "F-PART", "buy", "400", "49000.0",
    )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-PART")),
        "a partially covered execution must be withheld, not priced at the limit"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 0, "a withheld uncached order's fills go with it");
    assert!(
        !snapshot.reports_complete(),
        "withholding a report leaves the set incomplete"
    );
}

/// Coverage is exact: fills that exceed the order's executed quantity do not price it, and the
/// report is withheld.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_an_execution_its_fills_over_cover() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-OVER"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[
        ("f-over-1", "F-OVER", "buy", "1000", "49000.0"),
        ("f-over-2", "F-OVER", "buy", "200", "49100.0"),
    ]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-OVER")),
        "fills of 1200 do not price a 1000 execution"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 0, "a withheld uncached order's fills go with it");
    assert!(!snapshot.reports_complete());
}

/// Coverage can be reached across several fills, priced at their quantity-weighted average.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_an_execution_covered_across_two_fills() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-PART"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[
        ("f-part-1", "F-PART", "buy", "400", "49000.0"),
        ("f-part-2", "F-PART", "buy", "600", "49100.0"),
    ]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    let report = snapshot
        .order_reports()
        .get(&VenueOrderId::from("F-PART"))
        .cloned()
        .expect("fills summing to the filled quantity must keep the report");
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("49060").unwrap()),
        "(400 x 49000.0 + 600 x 49100.0) / 1000"
    );
    assert!(snapshot.reports_complete());
}

/// A canceled order with an executed quantity is terminal, so it is withheld the same way when
/// nothing covers that quantity.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_a_canceled_partial_with_no_fill() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json(&[futures_history_element(
            "OrderCancelled",
            "e1",
            "F-CANCEL",
            "PI_XBTUSD",
            "1000",
            "400",
            1680877245500,
        )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-CANCEL")),
        "a canceled order with an uncovered partial must be withheld"
    );
    assert!(!snapshot.reports_complete());
}

/// An open partially filled order is kept with no fill: the venue still lists it, and dropping it
/// would let reconciliation resolve it as missing.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_an_open_partial_with_no_fill() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(
        r#"{"result":"success","openOrders":[{"order_id":"F-OPEN","symbol":"PI_XBTUSD","side":"buy","orderType":"lmt","limitPrice":27500.5,"unfilledSize":600.0,"receivedTime":"2023-04-07T14:15:30.250Z","status":"partiallyFilled","filledSize":400.0,"reduceOnly":false,"lastUpdateTime":"2023-04-07T14:15:30.250Z"}]}"#
            .to_string(),
    );

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-OPEN")),
        "an open partially filled order must be kept"
    );
    assert!(snapshot.reports_complete());
}

/// A cached order's recorded fills count toward coverage, so with 600 of 1,000 contracts recorded
/// the final 400 on the page keep the report and reach reconciliation at their real price.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_cached_partial_order_covered_by_remaining_fills() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    cache_order_with_fills(&cache, "F-PART", &[("f-part-recorded", "600", "49000.0")]);
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-PART"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-part-remaining",
        "F-PART",
        "buy",
        "400",
        "49100.0",
    )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    let report = snapshot
        .order_reports()
        .get(&VenueOrderId::from("F-PART"))
        .cloned()
        .expect("the cached order covers the rest, so the report must be kept");
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("49040").unwrap()),
        "(600 x 49000.0 + 400 x 49100.0) / 1000"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(
        fills, 1,
        "the remaining execution must reach reconciliation"
    );
    assert!(snapshot.reports_complete());
}

/// A cached order's page fills stay when its report is withheld: the engine reconciles priced
/// fills against the cached order without a report. With 200 of 1,000 recorded and 400 on the
/// page, 400 contracts are unpriced, so only the report goes.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_cached_order_fills_when_its_report_is_withheld() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    cache_order_with_fills(&cache, "F-INC", &[("f-inc-recorded", "200", "49000.0")]);
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-INC"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-inc-on-page",
        "F-INC",
        "buy",
        "400",
        "49100.0",
    )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-INC")),
        "400 contracts are unpriced, so the report must be withheld"
    );
    let fill_reports = snapshot.fill_reports();
    let fills: Vec<_> = fill_reports.values().flatten().collect();
    assert_eq!(
        fills.len(),
        1,
        "the priced fill stays for the cached order: {fills:?}"
    );
    assert_eq!(fills[0].venue_order_id, VenueOrderId::from("F-INC"));
    assert_eq!(fills[0].trade_id, TradeId::from("f-inc-on-page"));
    assert!(!snapshot.reports_complete());
}

/// A history page holding a filled round trip on `PI_XBTUSD`: a 1,000-contract buy opened a long
/// and a 1,000-contract sell closed it.
fn flat_round_trip_history_json() -> String {
    futures_order_history_json(&[
        futures_history_element_sided(
            "OrderUpdated",
            "e2",
            "F-FLAT-CLOSE",
            "PI_XBTUSD",
            "Sell",
            "1000",
            "1000",
            1680879600000,
        ),
        futures_history_element_sided(
            "OrderUpdated",
            "e1",
            "F-FLAT-OPEN",
            "PI_XBTUSD",
            "Buy",
            "1000",
            "1000",
            1680876000000,
        ),
    ])
}

/// On an instrument the venue reports flat, an unbounded read keeps no terminal order whose fills
/// would open a position: with the opening fill off the fills page, neither side of the round
/// trip nor its fills reach reconciliation, and the set is incomplete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_a_closing_order_on_a_flat_instrument_when_the_open_is_off_page()
 {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(flat_round_trip_history_json());
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-flat-close",
        "F-FLAT-CLOSE",
        "sell",
        "1000",
        "50000.0",
    )]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(
        snapshot.order_reports().is_empty(),
        "neither side of the round trip may reach reconciliation: {:?}",
        snapshot.order_reports().keys().collect::<Vec<_>>()
    );
    assert!(snapshot.fill_reports().values().all(Vec::is_empty));
    assert!(!snapshot.reports_complete());
}

/// A round trip whose fills are all on the page nets to zero on the flat instrument and is kept.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_a_flat_round_trip_whose_fills_net_to_zero() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(flat_round_trip_history_json());
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[
        ("f-flat-open", "F-FLAT-OPEN", "buy", "1000", "49000.0"),
        ("f-flat-close", "F-FLAT-CLOSE", "sell", "1000", "50000.0"),
    ]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert_eq!(
        snapshot.order_reports().len(),
        2,
        "{:?}",
        snapshot.order_reports()
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 2);
    assert!(snapshot.reports_complete());
}

/// A closing order its own fill covers exactly, whose opening order is on neither page, is
/// withheld with that fill on a flat instrument, and the flat rule marks the set incomplete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_withholds_a_lone_closing_order_on_a_flat_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element_sided(
            "OrderUpdated",
            "e2",
            "F-FLAT-CLOSE",
            "PI_XBTUSD",
            "Sell",
            "1000",
            "1000",
            1680879600000,
        ),
    ]));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-flat-close",
        "F-FLAT-CLOSE",
        "sell",
        "1000",
        "50000.0",
    )]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(
        snapshot.order_reports().is_empty(),
        "the closing order must not reach reconciliation: {:?}",
        snapshot.order_reports().keys().collect::<Vec<_>>()
    );
    assert!(snapshot.fill_reports().values().all(Vec::is_empty));
    assert!(!snapshot.reports_complete());
}

/// An open `PI_XBTUSD` buy of 1,000 contracts with 500 of them executed.
fn futures_half_filled_open_order_json(order_id: &str) -> String {
    format!(
        r#"{{"result":"success","openOrders":[{{"order_id":"{order_id}","symbol":"PI_XBTUSD","side":"buy","orderType":"lmt","limitPrice":27500.5,"unfilledSize":500.0,"receivedTime":"2023-04-07T14:15:30.250Z","status":"partiallyFilled","filledSize":500.0,"reduceOnly":false,"lastUpdateTime":"2023-04-07T14:15:30.250Z"}}]}}"#
    )
}

/// On a flat instrument, the fills of an open order the cache does not hold count toward the net,
/// so a terminal order whose fill offsets them stays with both fills and the set stays complete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_nets_an_open_order_fill_against_a_terminal_one_on_a_flat_instrument()
 {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_half_filled_open_order_json("F-FLAT-O1"));
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element_sided(
            "OrderUpdated",
            "e1",
            "F-FLAT-T1",
            "PI_XBTUSD",
            "Sell",
            "500",
            "500",
            1680879600000,
        ),
    ]));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[
        ("f-flat-o1", "F-FLAT-O1", "buy", "500", "49000.0"),
        ("f-flat-t1", "F-FLAT-T1", "sell", "500", "50000.0"),
    ]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    let order_ids: Vec<_> = snapshot.order_reports().keys().copied().collect();
    assert!(
        order_ids.contains(&VenueOrderId::from("F-FLAT-T1"))
            && order_ids.contains(&VenueOrderId::from("F-FLAT-O1")),
        "both orders must reach reconciliation: {order_ids:?}"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 2);
    assert!(snapshot.reports_complete());
}

/// When the fills on a flat instrument do not net to zero, the flat rule withholds the terminal
/// orders with their fills and keeps the open order with its fill, marking the set incomplete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_an_open_order_beside_a_withheld_terminal_one_on_a_flat_instrument()
 {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_half_filled_open_order_json("F-FLAT-O1"));
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element_sided(
            "OrderUpdated",
            "e1",
            "F-FLAT-T1",
            "PI_XBTUSD",
            "Sell",
            "1000",
            "1000",
            1680879600000,
        ),
    ]));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[
        ("f-flat-o1", "F-FLAT-O1", "buy", "500", "49000.0"),
        ("f-flat-t1", "F-FLAT-T1", "sell", "1000", "50000.0"),
    ]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    let order_ids: Vec<_> = snapshot.order_reports().keys().copied().collect();
    assert_eq!(
        order_ids,
        vec![VenueOrderId::from("F-FLAT-O1")],
        "the open order stays and the terminal one is withheld"
    );
    let fill_orders: Vec<_> = snapshot.fill_reports().keys().copied().collect();
    assert_eq!(fill_orders, vec![VenueOrderId::from("F-FLAT-O1")]);
    assert!(!snapshot.reports_complete());
}

/// An open order the cache does not hold keeps its fill on a flat instrument that has no terminal
/// order, since the flat rule withholds terminal orders only, and the set stays complete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_an_open_order_fill_on_a_flat_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_half_filled_open_order_json("F-FLAT-O1"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-flat-o1",
        "F-FLAT-O1",
        "buy",
        "500",
        "49000.0",
    )]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(
        snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-FLAT-O1")),
        "an open order is not withheld by the flat rule"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 1);
    assert!(snapshot.reports_complete());
}

/// An instrument with a position report is left to that report, so the flat rule keeps the
/// closing order there.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_leaves_a_held_instrument_to_its_position_report() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(flat_round_trip_history_json());
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-flat-close",
        "F-FLAT-CLOSE",
        "sell",
        "1000",
        "50000.0",
    )]));
    *state.futures_open_positions_json.lock().await =
        Some(futures_open_positions_json("PI_XBTUSD"));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(
        snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-FLAT-CLOSE")),
        "the position report governs a held instrument"
    );
    assert!(
        !snapshot.reports_complete(),
        "the opening order is still withheld for coverage"
    );
}

/// A cached order is left out of the flat rule, since its fills reconcile against the cached order
/// and its position: its terminal report and fills stay on an instrument the venue reports flat.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_leaves_a_cached_order_on_a_flat_instrument_to_the_engine() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    cache_order_with_fills(&cache, "F-CACHED", &[]);
    *state.futures_order_history_json.lock().await = Some(executed_history_json("F-CACHED"));
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-cached", "F-CACHED", "buy", "1000", "49000.0",
    )]));

    let snapshot = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(
        snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-CACHED")),
        "a cached order is not withheld by the flat rule"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 1);
    assert!(snapshot.reports_complete());
}

/// With a bounded lookback the engine projects such orders onto order state only, so the flat
/// rule does not apply and the closing order is kept.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_keeps_the_closing_order_under_a_bounded_lookback() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_order_history_json.lock().await = Some(flat_round_trip_history_json());
    *state.fills_response.lock().await = Some(futures_recent_fills_json(&[(
        "f-flat-close",
        "F-FLAT-CLOSE",
        "sell",
        "1000",
        "50000.0",
    )]));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        snapshot
            .order_reports()
            .contains_key(&VenueOrderId::from("F-FLAT-CLOSE")),
        "a bounded read leaves the order-only projection to the engine"
    );
}

/// A history read the venue refuses, with HTTP 429 or an error body, degrades the mass status to
/// the open-only read rather than failing it: the open orders are reported, nothing from the
/// history is, and the set is incomplete.
#[rstest]
#[case::rate_limited(Some(StatusCode::TOO_MANY_REQUESTS), "rate limit exceeded")]
#[case::venue_error_body(None, r#"{"result":"error","error":"apiLimitExceeded"}"#)]
#[tokio::test]
async fn test_futures_mass_status_reads_open_orders_only_when_the_history_read_is_refused(
    #[case] status: Option<StatusCode>,
    #[case] body: &str,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-OPEN-1", "PI_XBTUSD"));
    *state.futures_order_history_json.lock().await = Some(body.to_string());
    *state.futures_order_history_status.lock().await = status;

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .expect("a refused history read must not fail the mass status")
        .unwrap();

    assert_eq!(
        snapshot.order_reports().keys().collect::<Vec<_>>(),
        vec![&VenueOrderId::from("V-OPEN-1")],
        "the open-only read supplies the open orders"
    );
    assert!(
        !snapshot.reports_complete(),
        "a mass status without its history is incomplete"
    );
}

/// A history read that faults rather than being refused fails the mass status, as a failed
/// open-order read does: an HTTP error status other than 429, including an authentication
/// status, and a body that cannot be parsed are not the venue declining the request.
#[rstest]
#[case::server_error(
    Some(StatusCode::INTERNAL_SERVER_ERROR),
    "history unavailable",
    "HTTP error 500"
)]
#[case::unauthorized(Some(StatusCode::UNAUTHORIZED), "invalid key", "Authentication error")]
#[case::malformed_body(
    None,
    r#"{"serverTime":"2023-04-07T16:30:45.678Z","elements":"nope"}"#,
    "Parse error"
)]
#[tokio::test]
async fn test_futures_mass_status_fails_when_the_history_read_faults(
    #[case] status: Option<StatusCode>,
    #[case] body: &str,
    #[case] expected: &str,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-OPEN-1", "PI_XBTUSD"));
    *state.futures_order_history_json.lock().await = Some(body.to_string());
    *state.futures_order_history_status.lock().await = status;

    let error = client
        .generate_mass_status(Some(60))
        .await
        .expect_err("a faulted history read must fail the mass status");

    let message = error.to_string();
    assert!(
        message.contains("get_order_events failed") && message.contains(expected),
        "unexpected error: {message}"
    );
}

/// A failed open-order read fails the mass status, including the open-only read that follows a
/// refused history read: a mass status without its open orders is not reported.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_fails_when_the_open_only_read_after_a_refusal_fails() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    state
        .futures_open_orders_sequence
        .lock()
        .await
        .push_back(futures_open_orders_json("V-OPEN-1", "PI_XBTUSD"));
    *state.futures_open_orders_json.lock().await =
        Some(r#"{"result":"error","error":"apiLimitExceeded"}"#.to_string());
    *state.futures_order_history_status.lock().await = Some(StatusCode::TOO_MANY_REQUESTS);

    let error = client
        .generate_mass_status(Some(60))
        .await
        .expect_err("a mass status without open orders must fail");

    assert!(
        error
            .to_string()
            .contains("Failed to get open orders: apiLimitExceeded"),
        "unexpected error: {error}"
    );
}

/// A failed open-order read fails the mass status.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_fails_when_the_open_order_read_fails() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(r#"{"result":"error","error":"apiLimitExceeded"}"#.to_string());

    let error = client
        .generate_mass_status(Some(60))
        .await
        .expect_err("a mass status without open orders must fail");

    assert!(
        error
            .to_string()
            .contains("Failed to get open orders: apiLimitExceeded"),
        "unexpected error: {error}"
    );
}

/// A scoped read rejects rows of another instrument the client holds, on both the open-order and
/// history reads. The spot-id case returns at the early guard, so this pins the per-row
/// comparison.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_order_reads_reject_rows_of_another_held_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(futures_open_orders_json_rows(&[
        ("V-SCOPE-XBT-OPEN", "PI_XBTUSD"),
        ("V-SCOPE-ETH-OPEN", "PF_ETHUSD"),
    ]));
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        futures_history_element(
            "OrderPlaced",
            "e1",
            "V-SCOPE-XBT-HIST",
            "PI_XBTUSD",
            "1000",
            "0",
            1680877245500,
        ),
        futures_history_element(
            "OrderPlaced",
            "e2",
            "V-SCOPE-ETH-HIST",
            "PF_ETHUSD",
            "1000",
            "0",
            1680877245500,
        ),
    ]));

    let scoped_cmd = |instrument_id: &str| {
        GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(InstrumentId::from(instrument_id)),
            None,
            None,
            None,
            None,
        )
    };

    let eth = client
        .generate_order_status_reports(&scoped_cmd("PF_ETHUSD.KRAKEN"))
        .await
        .unwrap();
    let mut eth_ids: Vec<&str> = eth.iter().map(|r| r.venue_order_id.as_str()).collect();
    eth_ids.sort_unstable();
    assert_eq!(
        eth_ids,
        vec!["V-SCOPE-ETH-HIST", "V-SCOPE-ETH-OPEN"],
        "only the scoped instrument's rows, from both reads: {eth:?}"
    );
    assert!(
        eth.iter()
            .all(|r| r.instrument_id == InstrumentId::from("PF_ETHUSD.KRAKEN"))
    );

    let xbt = client
        .generate_order_status_reports(&scoped_cmd("PI_XBTUSD.KRAKEN"))
        .await
        .unwrap();
    let mut xbt_ids: Vec<&str> = xbt.iter().map(|r| r.venue_order_id.as_str()).collect();
    xbt_ids.sort_unstable();
    assert_eq!(xbt_ids, vec!["V-SCOPE-XBT-HIST", "V-SCOPE-XBT-OPEN"]);
}

/// A scoped futures position read must match the resolved instrument.
///
/// This read is the one that used to return every futures position for a spot ID, since spot and
/// futures instrument ids share the `KRAKEN` venue.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_position_reports_match_the_resolved_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_positions_json.lock().await =
        Some(futures_open_positions_json("PI_XBTUSD"));

    let positions_cmd = |instrument_id: Option<InstrumentId>| {
        GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            instrument_id,
            None,
            None,
            None,
            None,
        )
    };

    // Control: scoped to the instrument that holds the position, it is returned.
    let scoped = client
        .generate_position_status_reports(&positions_cmd(Some(InstrumentId::from(
            "PI_XBTUSD.KRAKEN",
        ))))
        .await
        .unwrap();
    assert_eq!(
        scoped.len(),
        1,
        "the instrument's own position must be returned"
    );
    assert_eq!(
        scoped[0].instrument_id,
        InstrumentId::from("PI_XBTUSD.KRAKEN")
    );

    let absent = client
        .generate_position_status_reports(&positions_cmd(Some(InstrumentId::from(
            "BTC/USD.KRAKEN",
        ))))
        .await
        .unwrap();
    assert!(
        absent.is_empty(),
        "a spot id must match no futures position: {absent:?}"
    );
}

/// The same rule for the futures open-order read with `open_only=false`.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_order_reports_match_the_resolved_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-SCOPED-001", "PI_XBTUSD"));

    let orders_cmd = |instrument_id: Option<InstrumentId>| {
        GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false, // open_only=false, so the history read runs alongside the open-order read
            instrument_id,
            None,
            None,
            None,
            None,
        )
    };

    // Control: scoped to the instrument that holds the order, it is returned.
    let scoped = client
        .generate_order_status_reports(&orders_cmd(Some(InstrumentId::from("PI_XBTUSD.KRAKEN"))))
        .await
        .unwrap();
    assert_eq!(
        scoped.len(),
        1,
        "the instrument's own order must be returned"
    );
    assert_eq!(
        scoped[0].instrument_id,
        InstrumentId::from("PI_XBTUSD.KRAKEN")
    );

    let absent = client
        .generate_order_status_reports(&orders_cmd(Some(InstrumentId::from("BTC/USD.KRAKEN"))))
        .await
        .unwrap();
    assert!(
        absent.is_empty(),
        "a spot id must match no futures order: {absent:?}"
    );
}

fn futures_fills_for_symbol(symbol: &str) -> String {
    let fill_time = jiff::Timestamp::now() - jiff::Span::new().seconds(1);
    format!(
        r#"{{"result":"success","fills":[{{"fill_id":"f-window-1","symbol":"{symbol}","side":"buy","order_id":"V-WINDOW","fillTime":"{fill_time}","size":1,"price":50000.5,"fillType":"taker","cli_ord_id":"futures-window-001","fee_paid":0.0,"fee_currency":"USD"}}]}}"#
    )
}

/// A scoped futures read must match the resolved instrument and hold for one not held.
///
/// Spot and futures instrument ids share the `KRAKEN` venue, so a spot id can reach the futures
/// client. It must match nothing rather than falling through and returning every instrument's rows.
#[rstest]
#[tokio::test]
async fn test_futures_scoped_fill_reports_match_the_resolved_instrument() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.fills_response.lock().await = Some(futures_fills_for_symbol("PI_XBTUSD"));

    let fills_cmd = |instrument_id: Option<InstrumentId>| {
        GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            instrument_id,
            None,
            None,
            None,
            None,
            None,
        )
    };

    // Control: unscoped, the fill is read. Without this the assertions below could pass because
    // the venue returned nothing.
    let control = client.generate_fill_reports(fills_cmd(None)).await.unwrap();
    assert_eq!(control.len(), 1);

    let scoped = client
        .generate_fill_reports(fills_cmd(Some(InstrumentId::from("PI_XBTUSD.KRAKEN"))))
        .await
        .unwrap();
    assert_eq!(
        scoped.len(),
        1,
        "the instrument's own fill must be returned"
    );

    let absent = client
        .generate_fill_reports(fills_cmd(Some(InstrumentId::from("BTC/USD.KRAKEN"))))
        .await
        .unwrap();
    assert!(
        absent.is_empty(),
        "a spot id must match nothing on the futures client: {absent:?}"
    );
}

/// A bounded futures mass status must declare the cutoff it applied.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_declares_lookback_window() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.fills_response.lock().await = Some(futures_fills_for_symbol("PI_XBTUSD"));

    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        snapshot.lookback_start().is_some(),
        "a bounded mass status must record its cutoff"
    );
    assert!(snapshot.reports_complete());
}

/// An unresolved futures fill marks the bounded set incomplete.
#[rstest]
#[tokio::test]
async fn test_futures_mass_status_incomplete_when_historical_fill_unresolved() {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    // Control: the same fill on a listed symbol reports complete, so the flag below is driven by
    // the unresolved symbol rather than by the payload itself.
    *state.fills_response.lock().await = Some(futures_fills_for_symbol("PI_XBTUSD"));
    let control = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();
    assert!(control.reports_complete());
    let control_fills: usize = control.fill_reports().values().map(Vec::len).sum();
    assert_eq!(control_fills, 1);

    *state.fills_response.lock().await = Some(futures_fills_for_symbol("PI_NOTLISTED"));
    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot.reports_complete(),
        "an unresolved futures fill must mark the bounded set incomplete"
    );
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 0);
}

/// Mirrors `MAX_REPORT_PAGES` in the spot HTTP client, which is private to that crate.
const SPOT_REPORT_PAGE_CAP: usize = 500;

/// Report pagination must terminate even if the venue never returns an empty page.
///
/// The loops advance an offset until an empty page arrives. Without a cap, a venue that kept
/// returning a non-empty page would leave a startup reconciliation read spinning, which is worse
/// than failing it.
#[rstest]
#[tokio::test]
async fn test_spot_fill_pagination_stops_at_the_cap_and_reports_incomplete() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_unthrottled_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    // Control: the same page served once terminates on the empty page and reports complete.
    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT"]));
    let control = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();
    assert!(control.reports_complete());
    let control_fills: usize = control.fill_reports().values().map(Vec::len).sum();
    assert_eq!(control_fills, 1);

    let control_requests = state.trades_history_request_count.load(Ordering::Relaxed);

    // The same page on every request. The read must still return, and must not claim completeness.
    state.trades_history_repeat.store(true, Ordering::Relaxed);
    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT"]));

    let snapshot = tokio::time::timeout(
        Duration::from_secs(120),
        client.generate_mass_status(Some(60)),
    )
    .await
    .expect("a paginated read must terminate when the venue never returns an empty page")
    .unwrap()
    .unwrap();

    assert!(
        !snapshot.reports_complete(),
        "a read cut short by the page cap must not report as complete"
    );

    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert!(
        fills > control_fills,
        "the capped read should return the pages it did read: {fills}"
    );

    // The bound itself, not just its existence: the read stops on the page after the cap, so the
    // request count is exactly the cap. A one-page drift would not be visible in the assertions
    // above.
    assert_eq!(
        state.trades_history_request_count.load(Ordering::Relaxed) - control_requests,
        SPOT_REPORT_PAGE_CAP,
    );
}

/// The closed-order read is capped on the same terms as the fill read.
///
/// This drives the loop directly through a non-open report request. Startup mass status reaches it
/// as well, so the bound matters on both paths.
#[rstest]
#[tokio::test]
async fn test_spot_closed_order_pagination_stops_at_the_cap() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_unthrottled_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false, // open_only=false, so the closed-order pages are read
        None,
        None,
        None,
        None,
        None,
    );

    // Control: one page then an empty one terminates the loop, so only the closed order is read.
    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));
    let control = client.generate_order_status_reports(&cmd).await.unwrap();
    let control_requests = state.closed_orders_request_count.load(Ordering::Relaxed);
    assert_eq!(control_requests, 2, "one page, then the empty page");
    assert!(
        control
            .iter()
            .any(|report| report.order_status == OrderStatus::Filled),
        "the control must actually read the closed order, or the cap run proves nothing"
    );

    // The same page on every request: the read can only return by way of the cap.
    state.closed_orders_repeat.store(true, Ordering::Relaxed);
    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));

    tokio::time::timeout(
        Duration::from_secs(120),
        client.generate_order_status_reports(&cmd),
    )
    .await
    .expect("a paginated read must terminate when the venue never returns an empty page")
    .unwrap();

    assert_eq!(
        state.closed_orders_request_count.load(Ordering::Relaxed) - control_requests,
        SPOT_REPORT_PAGE_CAP,
    );
}

/// Startup mass status must read closed orders, not open orders alone.
///
/// An order that reached a terminal state while the node was down is only visible through
/// `ClosedOrders`, so an open-only mass status cannot reconcile it.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_includes_orders_only_in_closed_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_unthrottled_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(mass_status.reports_complete());

    let reports = mass_status.order_reports();

    // `OCLOSED-0` exists only in the ClosedOrders payload; the OpenOrders fixture uses entirely
    // different ids, so finding it proves the closed-order read ran rather than the open one.
    let closed = reports
        .get(&VenueOrderId::from("OCLOSED-0"))
        .expect("a closed order must reach mass status");
    assert_eq!(closed.order_status, OrderStatus::Filled);

    // Open orders must still be there: reading closed orders is an addition, not a swap.
    assert!(
        reports.contains_key(&VenueOrderId::from("O26VBY-ISGAE-JP5TLU")),
        "open orders must survive the closed-order read"
    );
}

/// A closed-order read cut short by the page cap must leave the mass status incomplete.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_incomplete_when_closed_orders_hit_the_cap() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_unthrottled_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    // Control: one page then an empty one, so the read terminates normally and declares complete.
    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));
    let control = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();
    assert!(
        control.reports_complete(),
        "the control must be complete, or the capped run proves nothing"
    );

    // The same page on every request: the read can only return by way of the cap.
    state.closed_orders_repeat.store(true, Ordering::Relaxed);
    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));

    let capped = tokio::time::timeout(
        Duration::from_secs(120),
        client.generate_mass_status(Some(60)),
    )
    .await
    .expect("a paginated read must terminate when the venue never returns an empty page")
    .unwrap()
    .unwrap();

    assert!(
        !capped.reports_complete(),
        "a mass status whose closed-order read hit the cap must not report as complete"
    );
}

/// A bounded mass status must declare the cutoff it applied.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_declares_lookback_window() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT"]));
    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        snapshot.lookback_start().is_some(),
        "a bounded mass status must record its cutoff"
    );
    assert!(snapshot.reports_complete());
}

/// A skipped historical row makes the bounded set incomplete.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_incomplete_when_historical_fill_unresolved() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    // Control: the identical payload with a resolvable pair reports complete, so the flag below
    // is driven by the unresolved instrument rather than by the payload itself.
    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT", "ETHUSDT"]));
    let control = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();
    assert!(control.reports_complete());
    // `fill_reports` groups by venue order id, so count the fills themselves.
    let control_fills: usize = control.fill_reports().values().map(Vec::len).sum();
    assert_eq!(control_fills, 2);

    *state.trades_history_json.lock().await = Some(spot_trades_json(&["XBTUSDT", "DELISTEDPAIR"]));
    let snapshot = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(
        !snapshot.reports_complete(),
        "an unresolved historical row must mark the bounded set incomplete"
    );
    assert!(snapshot.lookback_start().is_some());
    let fills: usize = snapshot.fill_reports().values().map(Vec::len).sum();
    assert_eq!(fills, 1);
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>) {
    let account_state = AccountState::new(
        test_account_id(),
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("1.0 BTC"),
            Money::from("0 BTC"),
            Money::from("1.0 BTC"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

fn add_test_spot_account_to_cache(cache: &Rc<RefCell<Cache>>) {
    let account_state = AccountState::new(
        test_account_id(),
        AccountType::Cash,
        vec![
            AccountBalance::new(
                Money::from("10000 USDT"),
                Money::from("0 USDT"),
                Money::from("10000 USDT"),
            ),
            AccountBalance::new(
                Money::from("1 BTC"),
                Money::from("0 BTC"),
                Money::from("1 BTC"),
            ),
        ],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Cash(CashAccount::new(account_state, true, false));
    cache.borrow_mut().add_account(account).unwrap();
}

fn add_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    add_futures_limit_order_to_cache(
        cache,
        client_order_id,
        test_instrument_id(),
        OrderSide::Buy,
        test_strategy_id(),
    )
}

fn add_futures_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    side: OrderSide,
    strategy_id: StrategyId,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        strategy_id,
        instrument_id,
        client_order_id,
        side,
        Quantity::from("1"),
        Price::from("50000"),
        TimeInForce::Gtc,
        None,
        true,
        false,
        false,
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
        UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    order_any
}

fn add_spot_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    add_spot_limit_order_on_instrument_to_cache(cache, client_order_id, test_spot_instrument_id())
}

fn add_spot_limit_order_on_instrument_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
) -> OrderAny {
    add_spot_limit_order_with_side_to_cache(cache, client_order_id, instrument_id, OrderSide::Buy)
}

fn add_spot_limit_order_with_side_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    side: OrderSide,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        instrument_id,
        client_order_id,
        side,
        Quantity::from("0.1"),
        Price::from("50000"),
        TimeInForce::Gtc,
        None,
        true,
        false,
        false,
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
        UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    order_any
}

fn submit_order_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn modify_order_command(order: &OrderAny, venue_order_id: VenueOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(venue_order_id),
        Some(Quantity::from("0.2")),
        Some(Price::from("49000")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn submit_order_list_command(
    order_list_id: &str,
    instrument_id: InstrumentId,
    orders: &[OrderAny],
) -> SubmitOrderList {
    let order_list = OrderList::new(
        OrderListId::from(order_list_id),
        instrument_id,
        test_strategy_id(),
        orders.iter().map(Order::client_order_id).collect(),
        UnixNanos::default(),
    );
    let order_inits = orders
        .iter()
        .map(|order| order.init_event().clone())
        .collect();

    SubmitOrderList::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn test_trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn test_strategy_id() -> StrategyId {
    StrategyId::from("S-001")
}

fn test_account_id() -> AccountId {
    AccountId::from("KRAKEN-001")
}

fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("PI_XBTUSD.KRAKEN")
}

fn test_spot_instrument_id() -> InstrumentId {
    InstrumentId::from("BTC/USDT.KRAKEN")
}

fn cancel_order_command(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_cancel_order_command(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn batch_cancel_command(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
    BatchCancelOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_batch_cancel_command(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
    BatchCancelOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_all_orders_command() -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_all_orders_command_with_side(order_side: Option<OrderSide>) -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        order_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_cancel_all_orders_command() -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_cancel_all_orders_command_with_side(order_side: Option<OrderSide>) -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        order_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

async fn wait_for_count(count: &AtomicUsize, expected: usize) {
    wait_until_async(
        || async { count.load(Ordering::Relaxed) >= expected },
        Duration::from_secs(5),
    )
    .await;
}

async fn assert_no_order_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) where
    F: Fn(&OrderEventAny) -> bool,
{
    let unexpected = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if let ExecutionEvent::Order(order_event) = &event
                && predicate(order_event)
            {
                return event;
            }
        }
    })
    .await;

    if let Ok(event) = unexpected {
        panic!("Unexpected order event: {event:?}");
    }
}

async fn recv_until<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) -> ExecutionEvent
where
    F: Fn(&ExecutionEvent) -> bool,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("Timed out waiting for execution event")
}

#[rstest]
#[tokio::test]
async fn test_spot_local_submit_failure_emits_rejected_without_request() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new("spot-submit-local-001");
    let order = add_spot_limit_order_on_instrument_to_cache(
        &cache,
        client_order_id,
        InstrumentId::from("UNKNOWN.KRAKEN"),
    );

    client.submit_order(submit_order_command(&order)).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("not found"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_spot_ambiguous_submit_failure_does_not_emit_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("spot-submit-ambiguous-001");
    let order = add_spot_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();
    wait_for_count(&state.submit_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if event.client_order_id == client_order_id)
    })
    .await;
    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_spot_structured_submit_rejection_emits_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::StructuredReject,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("spot-submit-rejected-001");
    let order = add_spot_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("EOrder:Insufficient funds"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_post_submit_lookup_failure_is_resolved_by_later_stream_acceptance() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::Success,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-submit-ambiguous-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();
    wait_for_count(&state.submit_request_count, 1).await;
    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if event.client_order_id == client_order_id)
    })
    .await;

    state
        .ws_message_tx
        .send(
            json!({
                "feed": "open_orders",
                "order": {
                    "instrument": "PI_XBTUSD",
                    "time": 1_567_702_877_410_i64,
                    "last_update_time": 1_567_702_877_410_i64,
                    "qty": 1,
                    "filled": 0,
                    "limit_price": 50_000,
                    "stop_price": 0,
                    "type": "limit",
                    "order_id": "F-LATER-ACCEPT",
                    "cli_ord_id": client_order_id.as_str(),
                    "direction": 0,
                    "reduce_only": false
                },
                "is_cancel": false,
                "reason": "new_placed_order_by_user"
            })
            .to_string(),
        )
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Accepted(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert_eq!(event.venue_order_id, VenueOrderId::from("F-LATER-ACCEPT"));
        }
        other => panic!("Expected OrderAccepted event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_futures_structured_submit_rejection_emits_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::StructuredReject,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-submit-rejected-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("insufficientAvailableFunds"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_unknown_submit_status_does_not_emit_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::UnknownStatus,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-submit-unknown-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();
    wait_for_count(&state.submit_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if event.client_order_id == client_order_id)
    })
    .await;
    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_ioc_would_not_execute_submit_emits_rejected() {
    // Maker Protection outcome: `iocWouldNotExecute` is the venue's terminal
    // answer for an order that cannot trade (including a converted hold that
    // finds no liquidity at release). It must reject the order, not leave it
    // ambiguous like an unknown status.
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            submit: OrderCommandResponse::IocWouldNotExecute,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("futures-submit-ioc-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("iocWouldNotExecute"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }

    assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 1);
}

/// Applies an `OrderSubmitted`, mirroring an order the venue may already hold while the cache
/// still records it as submitted.
fn mark_cached_order_submitted(cache: &Rc<RefCell<Cache>>, order: &OrderAny) {
    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        test_account_id(),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .unwrap();
}

/// Applies an `OrderAccepted` carrying the venue order ID, mirroring how the
/// engine records a venue ID on a cached order once the venue acknowledges it.
fn set_venue_order_id_on_cached_order(
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    venue_order_id: &str,
) {
    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from(venue_order_id),
        test_account_id(),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
}

const ORDERS_STATUS_PART_FILLED_CANCEL: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-001",
                "cliOrdId": "futures-held-001",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 5,
                "filled": 2,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.300Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "CANCELLED",
            "updateReason": "PARTIAL_FILL",
            "error": null
        }
    ]
}"#;

const ORDERS_STATUS_ENTERED_BOOK: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-002",
                "cliOrdId": "futures-held-002",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 1,
                "filled": 0,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.100Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "ENTERED_BOOK",
            "updateReason": null,
            "error": null
        },
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-004",
                "cliOrdId": "futures-held-004",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 2,
                "filled": 1,
                "limitPrice": 50100.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.200Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "ENTERED_BOOK",
            "updateReason": "PARTIAL_FILL",
            "error": null
        }
    ]
}"#;

const ORDERS_STATUS_HELD_BY_CLIENT_ID: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-003",
                "cliOrdId": "cli+ord&id=001",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 1,
                "filled": 0,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.100Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "ENTERED_BOOK",
            "updateReason": null,
            "error": null
        }
    ]
}"#;

#[rstest]
#[case::spot(true)]
#[case::futures(false)]
#[tokio::test]
async fn test_mass_status_captures_collection_start(#[case] spot: bool) {
    let (addr, state) = start_test_server().await.unwrap();
    let before = get_atomic_clock_realtime().get_time_ns();

    let snapshot = if spot {
        // Connect first so instruments are cached, as they are on the production path: an
        // unresolvable pair now fails the read rather than being dropped from it.
        let (mut client, _rx, cache) = create_test_spot_execution_client(addr);
        add_test_spot_account_to_cache(&cache);
        client.connect().await.unwrap();
        client.generate_mass_status(None).await.unwrap().unwrap()
    } else {
        let (client, _rx, _cache) = create_test_execution_client(addr);
        client.generate_mass_status(None).await.unwrap().unwrap()
    };

    let request_ts = state.collection_request_ts.lock().await.unwrap();

    assert!(snapshot.ts_init >= before);
    assert!(snapshot.ts_init <= request_ts);
}

#[rstest]
#[tokio::test]
async fn test_futures_mass_status_converges_part_filled_hold_from_orders_status() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-001"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-001");
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_PART_FILLED_CANCEL.to_string());

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status available");

    let order_reports = mass_status.order_reports();
    let report = order_reports
        .get(&VenueOrderId::from("V-HELD-001"))
        .expect("held order resolved from the orders-status window");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.filled_qty, Quantity::from("2"));
    assert_eq!(report.cancel_reason.as_deref(), Some("PARTIAL_FILL"));
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("futures-held-001"))
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_open_order_reports_include_orders_status_window() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-002"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-002");
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_ENTERED_BOOK.to_string());

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    let report = reports
        .iter()
        .find(|report| report.venue_order_id == VenueOrderId::from("V-HELD-002"))
        .expect("held order reported from the orders-status window");

    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.filled_qty, Quantity::from("0"));

    // Same window also reports a part-filled open order, not a fresh accept
    let part_filled = reports
        .iter()
        .find(|report| report.venue_order_id == VenueOrderId::from("V-HELD-004"))
        .expect("part-filled order reported from the orders-status window");

    assert_eq!(part_filled.order_status, OrderStatus::PartiallyFilled);
    assert_eq!(part_filled.filled_qty, Quantity::from("1"));
}

#[rstest]
#[tokio::test]
async fn test_futures_targeted_order_status_resolves_held_order_by_client_id() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    add_limit_order_to_cache(&cache, ClientOrderId::new("cli+ord&id=001"));
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_HELD_BY_CLIENT_ID.to_string());

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(ClientOrderId::new("cli+ord&id=001")),
        None,
        None,
        None,
    );

    let report = client.generate_order_status_report(&cmd).await.unwrap();

    let report = report.expect("held order resolved by client order ID");
    assert_eq!(report.venue_order_id, VenueOrderId::from("V-HELD-003"));
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("cli+ord&id=001"))
    );

    // Reserved characters in the client order ID must stay inside one param
    let body = state
        .orders_status_request_body
        .lock()
        .await
        .clone()
        .expect("orders-status request recorded");
    assert_eq!(body, "cliOrdIds=cli%2Bord%26id%3D001");
}

#[rstest]
#[tokio::test]
async fn test_futures_orders_status_failure_fails_report_generation() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-005"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-005");
    *state.orders_status_response.lock().await =
        Some(r#"{"result":"error","error":"maintenance"}"#.to_string());

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );

    let result = client.generate_order_status_reports(&cmd).await;

    let error = result.expect_err("orders-status failure must propagate");
    assert!(
        error.to_string().contains("maintenance"),
        "unexpected error: {error}"
    );
}

const ORDERS_STATUS_FULLY_EXECUTED: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-006",
                "cliOrdId": "futures-filled-006",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 1,
                "filled": 1,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.200Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "FULLY_EXECUTED",
            "updateReason": "FULL_FILL",
            "error": null
        }
    ]
}"#;

fn fills_fully_executed_now() -> String {
    let fill_time = jiff::Timestamp::now() - jiff::Span::new().seconds(1);
    format!(
        r#"{{"result":"success","fills":[{{"fill_id":"f-006-1","symbol":"PI_XBTUSD","side":"buy","order_id":"V-HELD-006","fillTime":"{fill_time}","size":1,"price":50000.5,"fillType":"taker","cli_ord_id":"futures-filled-006","fee_paid":0.0,"fee_currency":"USD"}}]}}"#
    )
}

#[rstest]
#[tokio::test]
async fn test_futures_targeted_fully_executed_prefers_fill_pricing() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    add_limit_order_to_cache(&cache, ClientOrderId::new("futures-filled-006"));
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_FULLY_EXECUTED.to_string());
    *state.fills_response.lock().await = Some(fills_fully_executed_now());

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(ClientOrderId::new("futures-filled-006")),
        None,
        None,
        None,
    );

    let report = client.generate_order_status_report(&cmd).await.unwrap();

    let report = report.expect("fully executed order resolved");
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(
        report.avg_px,
        Some(rust_decimal::Decimal::from_str_exact("50000.5").unwrap()),
        "the fills-derived report must price the execution, was {:?}",
        report.avg_px
    );
}

const ORDERS_STATUS_OPEN_AND_FILLED: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-007",
                "cliOrdId": "futures-held-007",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 1,
                "filled": 0,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.100Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "ENTERED_BOOK",
            "updateReason": null,
            "error": null
        },
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-008",
                "cliOrdId": "futures-filled-008",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": 1,
                "filled": 1,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.200Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "FULLY_EXECUTED",
            "updateReason": "FULL_FILL",
            "error": null
        }
    ]
}"#;

const ORDERS_STATUS_UNPARSABLE_QUANTITY: &str = r#"{
    "result": "success",
    "orders": [
        {
            "order": {
                "type": "ORDER",
                "orderId": "V-HELD-009",
                "cliOrdId": "futures-held-009",
                "symbol": "PI_XBTUSD",
                "side": "buy",
                "quantity": null,
                "filled": 0,
                "limitPrice": 50000.0,
                "reduceOnly": false,
                "timestamp": "2026-09-12T04:05:06.100Z",
                "lastUpdateTimestamp": "2026-09-12T04:05:06.100Z",
                "algoId": null,
                "priceTriggerOptions": null,
                "triggerTime": null
            },
            "status": "ENTERED_BOOK",
            "updateReason": null,
            "error": null
        }
    ]
}"#;

#[rstest]
#[tokio::test]
async fn test_futures_open_order_reports_exclude_unpriced_filled() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let open_order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-007"));
    set_venue_order_id_on_cached_order(&cache, &open_order, "V-HELD-007");
    let filled_order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-filled-008"));
    set_venue_order_id_on_cached_order(&cache, &filled_order, "V-HELD-008");
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_OPEN_AND_FILLED.to_string());

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert!(
        reports
            .iter()
            .any(|report| report.venue_order_id == VenueOrderId::from("V-HELD-007")),
        "open order reported from the orders-status window, was {reports:?}"
    );
    assert!(
        reports
            .iter()
            .all(|report| report.venue_order_id != VenueOrderId::from("V-HELD-008")),
        "fully executed entry must defer to the fills-paired targeted path"
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_targeted_fully_executed_without_fills_defers() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    add_limit_order_to_cache(&cache, ClientOrderId::new("futures-filled-006"));
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_FULLY_EXECUTED.to_string());

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(ClientOrderId::new("futures-filled-006")),
        None,
        None,
        None,
    );

    let result = client.generate_order_status_report(&cmd).await;

    let error = result.expect_err("unpriced fully executed order must defer");
    assert!(
        error.to_string().contains("without visible fills"),
        "unexpected error: {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_orders_status_parse_failure_fails_lookup() {
    // Ok(None) would let recon close an order that is live at the venue
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-009"));
    *state.orders_status_response.lock().await =
        Some(ORDERS_STATUS_UNPARSABLE_QUANTITY.to_string());

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(ClientOrderId::new("futures-held-009")),
        None,
        None,
        None,
    );

    let result = client.generate_order_status_report(&cmd).await;

    let error = result.expect_err("unparsable entry must fail the lookup");
    assert!(
        error
            .to_string()
            .contains("rather than the order as absent"),
        "unexpected error: {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_targeted_absent_order_returns_none() {
    let (client, _rx, cache, _state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    add_limit_order_to_cache(&cache, ClientOrderId::new("futures-absent-010"));

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(ClientOrderId::new("futures-absent-010")),
        None,
        None,
        None,
    );

    let report = client.generate_order_status_report(&cmd).await.unwrap();

    assert_eq!(report, None, "venue absence must surface as Ok(None)");
}

#[rstest]
#[tokio::test]
async fn test_futures_mass_status_excludes_unpriced_filled() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-filled-006"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-006");
    *state.orders_status_response.lock().await = Some(ORDERS_STATUS_FULLY_EXECUTED.to_string());

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status available");

    assert!(
        !mass_status
            .order_reports()
            .contains_key(&VenueOrderId::from("V-HELD-006")),
        "fully executed entry must defer to fills-paired pricing"
    );
}

#[rstest]
#[tokio::test]
async fn test_spot_ambiguous_modify_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            modify: OrderCommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("spot-modify-ambiguous-001");
    let order = add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(&order, VenueOrderId::from("S-MODIFY")))
        .unwrap();
    wait_for_count(&state.modify_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
    assert_eq!(state.modify_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_structured_modify_rejection_emits_modify_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            modify: OrderCommandResponse::StructuredReject,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-modify-rejected-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(&order, VenueOrderId::from("F-MODIFY")))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("notFound"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
    assert_eq!(state.modify_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_ambiguous_modify_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            modify: OrderCommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-modify-ambiguous-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(&order, VenueOrderId::from("F-MODIFY")))
        .unwrap();
    wait_for_count(&state.modify_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
    assert_eq!(state.modify_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_unknown_modify_status_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            modify: OrderCommandResponse::UnknownStatus,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new("futures-modify-unknown-001");
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(&order, VenueOrderId::from("F-MODIFY")))
        .unwrap();
    wait_for_count(&state.modify_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
    assert_eq!(state.modify_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_spot_whole_batch_submit_failure_does_not_reject_children() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::WholeFailure,
            ..Default::default()
        })
        .await;
    let first_id = ClientOrderId::new("spot-batch-ambiguous-001");
    let second_id = ClientOrderId::new("spot-batch-ambiguous-002");
    let orders = vec![
        add_spot_limit_order_to_cache(&cache, first_id),
        add_spot_limit_order_to_cache(&cache, second_id),
    ];

    client
        .submit_order_list(submit_order_list_command(
            "SPOT-BATCH-AMBIGUOUS",
            test_spot_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| matches!(event, OrderEventAny::Rejected(_)))
        .await;
    assert_eq!(state.batch_submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_spot_mixed_batch_submit_rejects_only_affected_child() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::Mixed,
            ..Default::default()
        })
        .await;
    let placed_id = ClientOrderId::new("spot-batch-placed-001");
    let rejected_id = ClientOrderId::new("spot-batch-rejected-001");
    let orders = vec![
        add_spot_limit_order_to_cache(&cache, placed_id),
        add_spot_limit_order_to_cache(&cache, rejected_id),
    ];

    client
        .submit_order_list(submit_order_list_command(
            "SPOT-BATCH-MIXED",
            test_spot_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == rejected_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, rejected_id);
            assert!(event.reason.contains("Insufficient funds"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if event.client_order_id == placed_id)
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_futures_whole_batch_submit_failure_does_not_reject_children() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::WholeFailure,
            ..Default::default()
        })
        .await;
    let first_id = ClientOrderId::new("futures-batch-ambiguous-001");
    let second_id = ClientOrderId::new("futures-batch-ambiguous-002");
    let orders = vec![
        add_limit_order_to_cache(&cache, first_id),
        add_limit_order_to_cache(&cache, second_id),
    ];

    client
        .submit_order_list(submit_order_list_command(
            "FUTURES-BATCH-AMBIGUOUS",
            test_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| matches!(event, OrderEventAny::Rejected(_)))
        .await;
}

#[rstest]
#[tokio::test]
async fn test_futures_failed_batch_chunk_rejects_only_unsent_tail() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::WholeFailure,
            ..Default::default()
        })
        .await;
    let mut orders = Vec::new();
    let mut sent_ids = Vec::new();

    for index in 0..10 {
        let client_order_id = ClientOrderId::new(format!("futures-batch-sent-{index:02}"));
        sent_ids.push(client_order_id);
        orders.push(add_limit_order_to_cache(&cache, client_order_id));
    }
    let unsent_id = ClientOrderId::new("futures-batch-unsent-10");
    orders.push(add_limit_order_to_cache(&cache, unsent_id));

    client
        .submit_order_list(submit_order_list_command(
            "FUTURES-BATCH-CHUNK-FAILURE",
            test_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == unsent_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, unsent_id);
            assert!(event.reason.contains("not sent after an earlier chunk"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if sent_ids.contains(&event.client_order_id))
    })
    .await;
    assert_eq!(state.batch_submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_futures_mixed_batch_submit_correlates_rejection_by_order_tag() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::Mixed,
            ..Default::default()
        })
        .await;
    let placed_id = ClientOrderId::new("futures-batch-placed-001");
    let rejected_id = ClientOrderId::new("futures-batch-rejected-001");
    let orders = vec![
        add_limit_order_to_cache(&cache, placed_id),
        add_limit_order_to_cache(&cache, rejected_id),
    ];

    client
        .submit_order_list(submit_order_list_command(
            "FUTURES-BATCH-MIXED",
            test_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == rejected_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, rejected_id);
            assert!(event.reason.contains("insufficientAvailableFunds"));
        }
        other => panic!("Expected OrderRejected event, was {other:?}"),
    }
    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::Rejected(event) if event.client_order_id == placed_id)
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_futures_unknown_batch_status_does_not_reject_children() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_submit: BatchSubmitResponse::UnknownStatus,
            ..Default::default()
        })
        .await;
    let first_id = ClientOrderId::new("futures-batch-unknown-001");
    let second_id = ClientOrderId::new("futures-batch-unknown-002");
    let orders = vec![
        add_limit_order_to_cache(&cache, first_id),
        add_limit_order_to_cache(&cache, second_id),
    ];

    client
        .submit_order_list(submit_order_list_command(
            "FUTURES-BATCH-UNKNOWN",
            test_instrument_id(),
            &orders,
        ))
        .unwrap();
    wait_for_count(&state.batch_submit_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| matches!(event, OrderEventAny::Rejected(_)))
        .await;
    assert_eq!(state.batch_submit_request_count.load(Ordering::Relaxed), 1);
}

#[rstest]
#[tokio::test]
async fn test_spot_local_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("spot-local-cancel-invalid-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("SPOT-SINGLE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(command).unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_spot_ambiguous_single_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-ambiguous-cancel-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_non_order_api_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::NonOrderApiError,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-rate-limit-cancel-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_explicit_single_cancel_api_error_emits_cancel_rejected() {
    let (client, mut rx, cache, _state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::StructuredReject,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-venue-cancel-reject-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("Unknown order"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_spot_whole_batch_cancel_failure_does_not_emit_one_reject_per_order() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    let first_client_order_id = ClientOrderId::new("spot-batch-cancel-whole-fail-001");
    let second_client_order_id = ClientOrderId::new("spot-batch-cancel-whole-fail-002");
    add_spot_limit_order_to_cache(&cache, first_client_order_id);
    add_spot_limit_order_to_cache(&cache, second_client_order_id);

    client
        .batch_cancel_orders(spot_batch_cancel_command(vec![
            spot_cancel_order_command(first_client_order_id, VenueOrderId::from("SPOT-BATCH-1")),
            spot_cancel_order_command(second_client_order_id, VenueOrderId::from("SPOT-BATCH-2")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_whole_cancel_all_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    // Cancel-all now selects open orders and cancels them by id, so the cache needs one.
    let order = add_spot_limit_order_to_cache(&cache, ClientOrderId::new("cancel-all-whole-001"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-WHOLE");

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

/// Futures side-filtered cancellation goes through the explicit-id batch path.
///
/// The unsided path keeps its symbol-scoped bulk cancellation, which is already correct.
#[rstest]
#[tokio::test]
async fn test_futures_cancel_all_side_filter_uses_batch_path() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-side-001"));
    set_venue_order_id_on_cached_order(&cache, &order, "V-FUT-BUY");

    client
        .cancel_all_orders(cancel_all_orders_command_with_side(Some(OrderSide::Buy)))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_eq!(
        state.cancel_request_count.load(Ordering::Relaxed),
        0,
        "side-filtered cancellation must not send per-order cancels"
    );
}

/// Returns the `orders` and `cl_ord_ids` arrays from the captured batch-cancel body.
///
/// Kraken keys transaction IDs and client order IDs separately, so a test that only looks for the
/// identifier anywhere in the body would pass even if it were sent in the wrong field.
async fn captured_batch_cancel_fields(state: &TestServerState) -> (Vec<String>, Vec<String>) {
    let body = state
        .last_batch_cancel_body
        .lock()
        .await
        .clone()
        .expect("batch cancel body");
    let value: Value = serde_json::from_str(&body).expect("batch cancel body is JSON");

    let read = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    (read("orders"), read("cl_ord_ids"))
}

/// Cancel-all must reach an order the venue may already hold while the cache still records it
/// as submitted, without widening beyond the requested instrument.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_includes_submitted_orders_within_instrument_scope() {
    let (client, _rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    // Submitted, so not open: the venue may already hold it.
    let submitted = add_spot_limit_order_to_cache(&cache, ClientOrderId::new("submitted-001"));
    mark_cached_order_submitted(&cache, &submitted);

    // Submitted on another instrument, which the request never named.
    let other = add_spot_limit_order_on_instrument_to_cache(
        &cache,
        ClientOrderId::new("submitted-other"),
        InstrumentId::from("ETH/USDT.KRAKEN"),
    );
    mark_cached_order_submitted(&cache, &other);

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    let (orders, cl_ord_ids) = captured_batch_cancel_fields(&state).await;

    // A submitted order has no venue ID yet, so Kraken requires it under `cl_ord_ids`.
    assert!(
        cl_ord_ids.contains(&"submitted-001".to_string()),
        "a submitted order must be cancelled by client order ID: orders={orders:?} cl_ord_ids={cl_ord_ids:?}"
    );
    assert!(
        !orders.contains(&"submitted-001".to_string()),
        "a client order ID must not be sent as a transaction ID: orders={orders:?}"
    );
    assert!(
        !orders.contains(&"submitted-other".to_string())
            && !cl_ord_ids.contains(&"submitted-other".to_string()),
        "another instrument must stay untouched: orders={orders:?} cl_ord_ids={cl_ord_ids:?}"
    );
}

/// Futures side-filtered cancellation submits the matching IDs only, and a per-order rejection
/// carries the strategy that owns the order.
#[rstest]
#[tokio::test]
async fn test_futures_cancel_all_submits_ids_and_preserves_owning_strategy() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::Mixed,
            ..Default::default()
        })
        .await;

    let strategy_ok = StrategyId::from("S-101");
    let strategy_rejected = StrategyId::from("S-202");

    // Two buy orders on the requested instrument, owned by different strategies. The mock
    // cancels V-BATCH-OK and rejects V-BATCH-REJECT.
    let ok = add_futures_limit_order_to_cache(
        &cache,
        ClientOrderId::new("fut-ok-001"),
        test_instrument_id(),
        OrderSide::Buy,
        strategy_ok,
    );
    set_venue_order_id_on_cached_order(&cache, &ok, "V-BATCH-OK");

    let rejected = add_futures_limit_order_to_cache(
        &cache,
        ClientOrderId::new("fut-reject-001"),
        test_instrument_id(),
        OrderSide::Buy,
        strategy_rejected,
    );
    set_venue_order_id_on_cached_order(&cache, &rejected, "V-BATCH-REJECT");

    // Opposite side on the same instrument.
    let sell = add_futures_limit_order_to_cache(
        &cache,
        ClientOrderId::new("fut-sell-001"),
        test_instrument_id(),
        OrderSide::Sell,
        strategy_ok,
    );
    set_venue_order_id_on_cached_order(&cache, &sell, "V-FUT-SELL");

    // A different instrument.
    let other = add_futures_limit_order_to_cache(
        &cache,
        ClientOrderId::new("fut-other-001"),
        InstrumentId::from("PI_ETHUSD.KRAKEN"),
        OrderSide::Buy,
        strategy_ok,
    );
    set_venue_order_id_on_cached_order(&cache, &other, "V-FUT-OTHER");

    client
        .cancel_all_orders(cancel_all_orders_command_with_side(Some(OrderSide::Buy)))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    let body = state
        .last_batch_cancel_body
        .lock()
        .await
        .clone()
        .expect("batch cancel body");
    assert!(body.contains("V-BATCH-OK"), "body: {body}");
    assert!(body.contains("V-BATCH-REJECT"), "body: {body}");
    assert!(
        !body.contains("V-FUT-SELL"),
        "the opposite side must be untouched: {body}"
    );
    assert!(
        !body.contains("V-FUT-OTHER"),
        "another instrument must be untouched: {body}"
    );

    let event = recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected))
                if rejected.client_order_id == ClientOrderId::new("fut-reject-001")
        )
    })
    .await;

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) => {
            assert_eq!(
                rejected.strategy_id, strategy_rejected,
                "the rejection must carry the strategy that owns the order"
            );
        }
        other => panic!("expected a cancel rejection, was {other:?}"),
    }
}

/// An unsided cancel-all must stay scoped to the instrument it names.
///
/// Kraken's account-wide `CancelAll` ignores the instrument, so it could cancel orders the
/// request never named.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_unsided_scopes_to_requested_instrument() {
    let (client, _rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    let target = add_spot_limit_order_to_cache(&cache, ClientOrderId::new("scoped-target-001"));
    set_venue_order_id_on_cached_order(&cache, &target, "V-TARGET");

    let other = add_spot_limit_order_on_instrument_to_cache(
        &cache,
        ClientOrderId::new("scoped-other-001"),
        InstrumentId::from("ETH/USDT.KRAKEN"),
    );
    set_venue_order_id_on_cached_order(&cache, &other, "V-OTHER");

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    let (orders, cl_ord_ids) = captured_batch_cancel_fields(&state).await;

    // An accepted order carries a venue ID, which Kraken expects under `orders`.
    assert!(
        orders.contains(&"V-TARGET".to_string()),
        "orders={orders:?} cl_ord_ids={cl_ord_ids:?}"
    );
    assert!(
        !cl_ord_ids.contains(&"V-TARGET".to_string()),
        "a transaction ID must not be sent as a client order ID: cl_ord_ids={cl_ord_ids:?}"
    );
    assert!(
        !orders.contains(&"V-OTHER".to_string()),
        "an order on another instrument must be untouched: orders={orders:?}"
    );
    assert_eq!(
        state.cancel_all_request_count.load(Ordering::Relaxed),
        0,
        "account-wide CancelAll must not be used"
    );
}

/// A side-filtered cancel-all submits only the matching side, through the batch path.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_side_filter_selects_only_that_side() {
    let (client, _rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    let buy = add_spot_limit_order_with_side_to_cache(
        &cache,
        ClientOrderId::new("side-buy-001"),
        test_spot_instrument_id(),
        OrderSide::Buy,
    );
    set_venue_order_id_on_cached_order(&cache, &buy, "V-BUY");

    let sell = add_spot_limit_order_with_side_to_cache(
        &cache,
        ClientOrderId::new("side-sell-001"),
        test_spot_instrument_id(),
        OrderSide::Sell,
    );
    set_venue_order_id_on_cached_order(&cache, &sell, "V-SELL");

    client
        .cancel_all_orders(spot_cancel_all_orders_command_with_side(Some(
            OrderSide::Sell,
        )))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    let body = state
        .last_batch_cancel_body
        .lock()
        .await
        .clone()
        .expect("batch cancel body");
    assert!(body.contains("V-SELL"), "body: {body}");
    assert!(
        !body.contains("V-BUY"),
        "the opposite side must be untouched: {body}"
    );
}

/// With nothing to cancel the adapter must not send a request at all.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_without_matching_orders_sends_no_request() {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(state.batch_cancel_request_count.load(Ordering::Relaxed), 0);
    assert_eq!(state.cancel_all_request_count.load(Ordering::Relaxed), 0);
}

/// Selected ids are chunked at the venue batch limit.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_chunks_at_venue_batch_limit() {
    let (client, _rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    for i in 0..51 {
        let order = add_spot_limit_order_to_cache(&cache, ClientOrderId::new(format!("chunk-{i}")));
        set_venue_order_id_on_cached_order(&cache, &order, &format!("V-CHUNK-{i}"));
    }

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    // 51 selected ids exceed the 50-order venue limit, so two requests are sent.
    wait_for_count(&state.batch_cancel_request_count, 2).await;
}

/// A partial batch result is left to reconciliation rather than rejecting individual orders.
#[rstest]
#[tokio::test]
async fn test_spot_cancel_all_partial_result_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::Mixed,
            ..Default::default()
        })
        .await;

    for i in 0..2 {
        let order =
            add_spot_limit_order_to_cache(&cache, ClientOrderId::new(format!("partial-{i}")));
        set_venue_order_id_on_cached_order(&cache, &order, &format!("V-PARTIAL-{i}"));
    }

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_single_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-cancel-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(
            client_order_id,
            VenueOrderId::from("V-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_structured_single_cancel_rejection_emits_cancel_rejected() {
    let (client, mut rx, cache, _state) =
        connected_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::StructuredReject,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-cancel-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(
            client_order_id,
            VenueOrderId::from("V-SINGLE"),
        ))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.contains("notFound"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("V-SINGLE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(command).unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_local_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("batch-local-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("V-BATCH-LOCAL")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .batch_cancel_orders(BatchCancelOrders::new(
            test_trader_id(),
            Some(*KRAKEN_CLIENT_ID),
            test_strategy_id(),
            test_instrument_id(),
            vec![command],
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.batch_cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_whole_batch_cancel_failure_does_not_emit_one_reject_per_order() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    let first_client_order_id = ClientOrderId::new("batch-cancel-whole-fail-001");
    let second_client_order_id = ClientOrderId::new("batch-cancel-whole-fail-002");
    add_limit_order_to_cache(&cache, first_client_order_id);
    add_limit_order_to_cache(&cache, second_client_order_id);

    client
        .batch_cancel_orders(batch_cancel_command(vec![
            cancel_order_command(first_client_order_id, VenueOrderId::from("V-BATCH-1")),
            cancel_order_command(second_client_order_id, VenueOrderId::from("V-BATCH-2")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_mixed_per_item_batch_cancel_result_rejects_only_failed_item() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::Mixed,
            ..Default::default()
        })
        .await;

    let ok_client_order_id = ClientOrderId::new("batch-cancel-ok-001");
    let reject_client_order_id = ClientOrderId::new("batch-cancel-reject-001");
    add_limit_order_to_cache(&cache, ok_client_order_id);
    add_limit_order_to_cache(&cache, reject_client_order_id);

    client
        .batch_cancel_orders(batch_cancel_command(vec![
            cancel_order_command(ok_client_order_id, VenueOrderId::from("V-BATCH-OK")),
            cancel_order_command(reject_client_order_id, VenueOrderId::from("V-BATCH-REJECT")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == reject_client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, reject_client_order_id);
            assert!(event.reason.contains("notFound"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == ok_client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_whole_cancel_all_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses {
            cancel_all: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    client
        .cancel_all_orders(cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.cancel_all_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}
