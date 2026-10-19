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
    events::{
        AccountState, OrderAccepted, OrderDenied, OrderEventAny, OrderFilled, OrderPendingCancel,
        OrderRejected, OrderSubmitted,
    },
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
    /// The venue rejects the item it echoes by `LONG_CLIENT_ORDER_ID` alone.
    RejectedByClientOrderId,
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
    /// The path, query and body of each futures send, edit and cancel request, in order.
    command_requests: Arc<tokio::sync::Mutex<Vec<String>>>,
    orders_status_response: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Served by `/derivatives/api/v3/orders/status` one per request, ahead of the override.
    orders_status_sequence: Arc<tokio::sync::Mutex<VecDeque<String>>>,
    orders_status_request_body: Arc<tokio::sync::Mutex<Option<String>>>,
    fills_response: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/derivatives/api/v3/openorders` returns this JSON.
    futures_open_orders_json: Arc<tokio::sync::Mutex<Option<String>>>,
    /// When set, `/0/private/OpenOrders` returns this JSON instead of `http_open_orders.json`.
    spot_open_orders_json: Arc<tokio::sync::Mutex<Option<String>>>,
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
            command_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            orders_status_response: Arc::new(tokio::sync::Mutex::new(None)),
            orders_status_sequence: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            orders_status_request_body: Arc::new(tokio::sync::Mutex::new(None)),
            fills_response: Arc::new(tokio::sync::Mutex::new(None)),
            futures_open_orders_json: Arc::new(tokio::sync::Mutex::new(None)),
            spot_open_orders_json: Arc::new(tokio::sync::Mutex::new(None)),
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

    let req = if matches!(
        path.as_str(),
        "/derivatives/api/v3/sendorder"
            | "/derivatives/api/v3/editorder"
            | "/derivatives/api/v3/cancelorder"
    ) {
        let (parts, body) = req.into_parts();
        let body = to_bytes(body, 1024 * 1024).await.unwrap();
        state.command_requests.lock().await.push(format!(
            "{path} {} {}",
            parts.uri.query().unwrap_or_default(),
            String::from_utf8_lossy(&body)
        ));
        Request::from_parts(parts, Body::from(body))
    } else {
        req
    };

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

            if let Some(response) = state.orders_status_sequence.lock().await.pop_front() {
                return json_response(response);
            }
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
                BatchCancelResponse::RejectedByClientOrderId => json_response(format!(
                    r#"{{"result":"success","batchStatus":[{{"cliOrdId":"{LONG_CLIENT_ORDER_ID}","status":"notFound"}}]}}"#
                )),
            }
        }
        "/derivatives/api/v3/cancelallorders" => {
            state
                .cancel_all_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.cancel_all {
                BatchCancelResponse::Success
                | BatchCancelResponse::Mixed
                | BatchCancelResponse::RejectedByClientOrderId => json_response(
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
        "/0/private/OpenOrders" => {
            let response = state.spot_open_orders_json.lock().await;
            json_response(
                response
                    .clone()
                    .unwrap_or_else(|| load_test_data("http_open_orders.json")),
            )
        }
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
                BatchCancelResponse::Success | BatchCancelResponse::RejectedByClientOrderId => {
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
                BatchCancelResponse::Success
                | BatchCancelResponse::Mixed
                | BatchCancelResponse::RejectedByClientOrderId => {
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

/// The spot single-order status lookup reads only the queried instrument's orders, so an
/// unresolvable open order on another pair is out of scope for it rather than failing it. An
/// unscoped lookup still reads that order and fails.
#[rstest]
#[case::scoped(Some(InstrumentId::from("BTC/USDT.KRAKEN")), true)]
#[case::unscoped(None, false)]
#[tokio::test]
async fn test_spot_order_status_report_lookup_is_scoped_to_its_instrument(
    #[case] instrument_id: Option<InstrumentId>,
    #[case] reported: bool,
) {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    *state.spot_open_orders_json.lock().await =
        Some(load_test_data("http_open_orders_held_and_unresolved.json"));

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        instrument_id,
        None,
        Some(VenueOrderId::from("OHELD1-AAAAA-AAAAAA")),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    if reported {
        let report = result
            .expect("an unresolvable order on another pair is out of scope")
            .expect("the held order is reported");
        assert_eq!(
            report.venue_order_id,
            VenueOrderId::from("OHELD1-AAAAA-AAAAAA")
        );
    } else {
        let error = result.expect_err("an unscoped lookup reads the unresolvable order");
        assert!(
            format!("{error:#}").contains("instrument not in cache for pair NOSUCHPAIR"),
            "{error:#}"
        );
    }
}

/// A scoped spot lookup reads past an unresolvable open order on another pair into the closed
/// orders, so an order that has closed is still found.
#[rstest]
#[tokio::test]
async fn test_spot_order_status_report_lookup_reaches_closed_orders_past_an_unresolved_pair() {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    *state.spot_open_orders_json.lock().await =
        Some(load_test_data("http_open_orders_held_and_unresolved.json"));
    *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_spot_instrument_id()),
        None,
        Some(VenueOrderId::from("OCLOSED-0")),
        None,
        None,
    );

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .expect("an unresolvable order on another pair is out of scope")
        .expect("the closed order is reported");
    assert_eq!(report.venue_order_id, VenueOrderId::from("OCLOSED-0"));
}

/// A spot lookup scoped to a futures instrument, which the spot client never holds, reads nothing
/// and reports no order, even for a venue order ID one of its own pairs carries.
#[rstest]
#[tokio::test]
async fn test_spot_order_status_report_lookup_for_an_unheld_instrument_reads_nothing() {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("PI_XBTUSD.KRAKEN")),
        None,
        Some(VenueOrderId::from("O26VBY-ISGAE-JP5TLU")),
        None,
        None,
    );
    let report = client.generate_order_status_report(&cmd).await.unwrap();

    assert!(report.is_none(), "{report:?}");
    assert!(
        state.collection_request_ts.lock().await.is_none(),
        "no open orders are read for an instrument the client does not hold"
    );
}

#[rstest]
#[tokio::test]
async fn test_spot_query_order_is_scoped_to_its_instrument() {
    let (client, mut rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    *state.spot_open_orders_json.lock().await =
        Some(load_test_data("http_open_orders_held_and_unresolved.json"));

    client
        .query_order(QueryOrder::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("S-001"),
            test_spot_instrument_id(),
            ClientOrderId::new("O-QUERY-1"),
            Some(VenueOrderId::from("OHELD1-AAAAA-AAAAAA")),
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
    assert_eq!(
        report.venue_order_id,
        VenueOrderId::from("OHELD1-AAAAA-AAAAAA")
    );
}

/// A read over the open orders, by the path the engine takes.
#[derive(Clone, Copy, Debug)]
enum OpenOrderRead {
    /// `generate_order_status_reports` with `open_only`, scoped to the instrument when given.
    Bulk(Option<&'static str>),
    /// `generate_order_status_report` for a venue order ID, scoped to its instrument.
    Lookup(&'static str, &'static str),
    MassStatus,
}

/// Runs `read` and returns the venue order IDs of the order reports it yields.
async fn read_open_orders(
    client: &dyn ExecutionClient,
    read: OpenOrderRead,
) -> anyhow::Result<Vec<VenueOrderId>> {
    let venue_order_ids = match read {
        OpenOrderRead::Bulk(instrument_id) => {
            let cmd = GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                true,
                instrument_id.map(InstrumentId::from),
                None,
                None,
                None,
                None,
            );
            client
                .generate_order_status_reports(&cmd)
                .await?
                .into_iter()
                .map(|report| report.venue_order_id)
                .collect()
        }
        OpenOrderRead::Lookup(instrument_id, venue_order_id) => {
            let cmd = GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(InstrumentId::from(instrument_id)),
                None,
                Some(VenueOrderId::from(venue_order_id)),
                None,
                None,
            );
            client
                .generate_order_status_report(&cmd)
                .await?
                .into_iter()
                .map(|report| report.venue_order_id)
                .collect()
        }
        OpenOrderRead::MassStatus => client
            .generate_mass_status(None)
            .await?
            .expect("mass status available")
            .order_reports()
            .keys()
            .copied()
            .collect(),
    };
    Ok(venue_order_ids)
}

/// An in-scope spot open order that resolves but cannot be parsed fails the read on every path,
/// since dropped it would read to reconciliation as an order the venue does not have. A read scoped
/// to another pair leaves it out of scope.
#[rstest]
#[case::bulk_unscoped(
    OpenOrderRead::Bulk(None),
    Err("OpenOrders: failed to parse order OBADVL-CCCCC-CCCCCC")
)]
#[case::bulk_scoped_to_another_pair(OpenOrderRead::Bulk(Some("BTC/USDT.KRAKEN")), Ok(&["OHELD1-AAAAA-AAAAAA"][..]))]
#[case::lookup_on_its_pair(
    OpenOrderRead::Lookup("ETH/USDT.KRAKEN", "OBADVL-CCCCC-CCCCCC"),
    Err("OpenOrders: failed to parse order OBADVL-CCCCC-CCCCCC")
)]
#[case::mass_status(
    OpenOrderRead::MassStatus,
    Err("OpenOrders: failed to parse order OBADVL-CCCCC-CCCCCC")
)]
#[tokio::test]
async fn test_spot_open_order_reads_fail_on_an_unparsable_order(
    #[case] read: OpenOrderRead,
    #[case] expected: Result<&[&str], &str>,
) {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    *state.spot_open_orders_json.lock().await =
        Some(load_test_data("http_open_orders_unparsable.json"));

    let result = read_open_orders(&client, read).await;

    match expected {
        Ok(venue_order_ids) => {
            let expected: Vec<VenueOrderId> = venue_order_ids
                .iter()
                .map(|id| VenueOrderId::from(*id))
                .collect();
            assert_eq!(result.unwrap(), expected);
        }
        Err(message) => {
            let error = result.expect_err("the unparsable open order fails the read");
            assert!(format!("{error:#}").contains(message), "{error:#}");
        }
    }
}

/// A spot closed order that cannot be parsed is skipped and leaves the set incomplete, while the
/// rest of the read stands.
#[rstest]
#[tokio::test]
async fn test_spot_mass_status_skips_an_unparsable_closed_order_as_incomplete() {
    let (client, _rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;
    *state.closed_orders_json.lock().await =
        Some(load_test_data("http_closed_orders_unparsable.json"));

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status available");

    let order_reports = mass_status.order_reports();
    assert!(order_reports.contains_key(&VenueOrderId::from("OCLGOD-DDDDD-DDDDDD")));
    assert!(!order_reports.contains_key(&VenueOrderId::from("OCLBAD-EEEEE-EEEEEE")));
    assert!(!mass_status.reports_complete());
}

/// What a single-order lookup answers for an order its read did not return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LookupAnswer {
    /// `Ok(None)`: the venue has no such order.
    Absent,
    /// An error, so reconciliation defers.
    Deferred,
    /// The order's report.
    Found,
}

fn assert_lookup_answer(
    result: anyhow::Result<Option<OrderStatusReport>>,
    expected: LookupAnswer,
    venue_order_id: &str,
) {
    match (expected, result) {
        (LookupAnswer::Absent, Ok(None)) => {}
        (LookupAnswer::Found, Ok(Some(report))) => {
            assert_eq!(report.venue_order_id, VenueOrderId::from(venue_order_id));
        }
        (LookupAnswer::Deferred, Err(e)) => {
            assert!(
                format!("{e:#}").contains("does not rule"),
                "unexpected error: {e:#}"
            );
        }
        (expected, result) => panic!("expected {expected:?}, received {result:?}"),
    }
}

/// How the spot closed-order read in a lookup test is set up.
#[derive(Clone, Copy, Debug)]
enum SpotLookupRead {
    /// Every closed order parses.
    Complete,
    /// One closed order, `OCLBAD-EEEEE-EEEEEE`, cannot be parsed.
    UnparsableClosedOrder,
    /// The venue never returns an empty page, so the walk stops at its cap.
    PageCap,
    /// One closed order, `OCLOSED-0`, names a pair the client cannot resolve.
    UnresolvedClosedOrder,
    /// One open order, `OUNRES-BBBBB-BBBBBB`, names a pair the client cannot resolve.
    UnresolvedOpenOrder,
}

/// The spot lookup answers that the venue has no such order only from a read that rules the order
/// out: a closed order that cannot be parsed or resolved, or an open one that cannot be resolved,
/// rules out every other order with a venue order ID, but not itself nor an order without one, and
/// a walk cut short at its page cap rules out none.
#[rstest]
#[case::complete_read(
    SpotLookupRead::Complete,
    Some("OTARGT-AAAAA-AAAAAA"),
    LookupAnswer::Absent
)]
#[case::unparsable_row_of_another_order(
    SpotLookupRead::UnparsableClosedOrder,
    Some("OTARGT-AAAAA-AAAAAA"),
    LookupAnswer::Absent
)]
#[case::unparsable_row_of_the_order(
    SpotLookupRead::UnparsableClosedOrder,
    Some("OCLBAD-EEEEE-EEEEEE"),
    LookupAnswer::Deferred
)]
#[case::order_without_venue_id_beside_an_unparsable_row(
    SpotLookupRead::UnparsableClosedOrder,
    None,
    LookupAnswer::Deferred
)]
#[case::page_cap(
    SpotLookupRead::PageCap,
    Some("OTARGT-AAAAA-AAAAAA"),
    LookupAnswer::Deferred
)]
#[case::unresolved_row_of_another_order(
    SpotLookupRead::UnresolvedClosedOrder,
    Some("OTARGT-AAAAA-AAAAAA"),
    LookupAnswer::Absent
)]
#[case::unresolved_row_of_the_order(
    SpotLookupRead::UnresolvedClosedOrder,
    Some("OCLOSED-0"),
    LookupAnswer::Deferred
)]
#[case::unresolved_open_row_of_another_order(
    SpotLookupRead::UnresolvedOpenOrder,
    Some("OTARGT-AAAAA-AAAAAA"),
    LookupAnswer::Absent
)]
#[case::unresolved_open_row_of_the_order(
    SpotLookupRead::UnresolvedOpenOrder,
    Some("OUNRES-BBBBB-BBBBBB"),
    LookupAnswer::Deferred
)]
#[tokio::test]
async fn test_spot_order_lookup_reports_absence_only_from_a_read_that_rules_the_order_out(
    #[case] read: SpotLookupRead,
    #[case] venue_order_id: Option<&str>,
    #[case] expected: LookupAnswer,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_unthrottled_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("spot-lookup-1");
    let order = add_spot_limit_order_to_cache(&cache, client_order_id);
    if let Some(venue_order_id) = venue_order_id {
        set_venue_order_id_on_cached_order(&cache, &order, venue_order_id);
    }

    match read {
        SpotLookupRead::Complete => {
            *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));
        }
        SpotLookupRead::UnparsableClosedOrder => {
            *state.closed_orders_json.lock().await =
                Some(load_test_data("http_closed_orders_unparsable.json"));
        }
        SpotLookupRead::PageCap => {
            state.closed_orders_repeat.store(true, Ordering::Relaxed);
            *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));
        }
        SpotLookupRead::UnresolvedClosedOrder => {
            *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["NOSUCHPAIR"]));
        }
        SpotLookupRead::UnresolvedOpenOrder => {
            *state.spot_open_orders_json.lock().await =
                Some(load_test_data("http_open_orders_held_and_unresolved.json"));
            *state.closed_orders_json.lock().await = Some(spot_closed_orders_json(&["XBTUSDT"]));
        }
    }

    // As the engine's targeted query does, the command carries the order's venue order ID
    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_spot_instrument_id()),
        Some(client_order_id),
        venue_order_id.map(VenueOrderId::from),
        None,
        None,
    );
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        client.generate_order_status_report(&cmd),
    )
    .await
    .expect("the lookup must return");

    assert_lookup_answer(result, expected, venue_order_id.unwrap_or_default());
}

/// The earliest venue time of the futures order in a lookup test, in milliseconds.
const FUTURES_LOOKUP_ORDER_MS: i64 = 1_700_000_000_000;

/// How the futures order read in a lookup test is set up.
#[derive(Clone, Copy, Debug)]
enum FuturesLookupRead {
    /// The history holds another order's event and no continuation token.
    Complete,
    /// A page with a continuation token whose oldest event lies this many seconds before the
    /// order's earliest venue time.
    TokenPageFrom(i64),
    /// A page with a continuation token and no events.
    TokenPageWithoutEvents,
    /// The order's own event on a page with a continuation token.
    OrderOnTokenPage,
    /// A history row of a kind the documentation does not list, which names no order.
    UndocumentedRow,
    /// A history row whose direction the venue could not decode, of this order.
    UndecodableRowOf(&'static str),
    /// The open orders list an unsized reduce-only trigger, `V-TRIGGER`, the read skips.
    SkippedTrigger,
    /// A history row of this order on a contract the client cannot resolve.
    UnresolvedRowOf(&'static str),
    /// An open order, `V-UNRESOLVED`, on a contract the client cannot resolve.
    UnresolvedOpenOrder,
    /// A history row of this order that cannot be parsed, its quantity negative.
    UnparsableRowOf(&'static str),
}

/// The futures lookup answers that the venue has no such order only from a read that rules the
/// order out. A history page handed back with a continuation token holds the newest events, so it
/// rules out an order placed after its oldest event by more than the clock skew allowance, and a
/// page with no events rules out none. A skipped row, one the client cannot resolve or parse
/// included, rules out every other order with a venue order ID, and a row naming no order rules out
/// none. An order the cut-short read returns is reported.
#[rstest]
#[case::complete_read(FuturesLookupRead::Complete, "V-LOOKUP-1", LookupAnswer::Absent)]
#[case::token_page_reaching_past_the_order(
    FuturesLookupRead::TokenPageFrom(61),
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::token_page_within_the_skew(
    FuturesLookupRead::TokenPageFrom(60),
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::token_page_after_the_order(
    FuturesLookupRead::TokenPageFrom(-10),
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::token_page_without_events(
    FuturesLookupRead::TokenPageWithoutEvents,
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::order_on_a_token_page(
    FuturesLookupRead::OrderOnTokenPage,
    "V-LOOKUP-1",
    LookupAnswer::Found
)]
#[case::undocumented_row(
    FuturesLookupRead::UndocumentedRow,
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::undecodable_row_of_another_order(
    FuturesLookupRead::UndecodableRowOf("V-OTHER-1"),
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::undecodable_row_of_the_order(
    FuturesLookupRead::UndecodableRowOf("V-LOOKUP-1"),
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::skipped_trigger_of_another_order(
    FuturesLookupRead::SkippedTrigger,
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::skipped_trigger_is_the_order(
    FuturesLookupRead::SkippedTrigger,
    "V-TRIGGER",
    LookupAnswer::Deferred
)]
#[case::unresolved_row_of_another_order(
    FuturesLookupRead::UnresolvedRowOf("V-OTHER-1"),
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::unresolved_row_of_the_order(
    FuturesLookupRead::UnresolvedRowOf("V-LOOKUP-1"),
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[case::unresolved_open_order_of_another_order(
    FuturesLookupRead::UnresolvedOpenOrder,
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::unresolved_open_order_is_the_order(
    FuturesLookupRead::UnresolvedOpenOrder,
    "V-UNRESOLVED",
    LookupAnswer::Deferred
)]
#[case::unparsable_row_of_another_order(
    FuturesLookupRead::UnparsableRowOf("V-OTHER-1"),
    "V-LOOKUP-1",
    LookupAnswer::Absent
)]
#[case::unparsable_row_of_the_order(
    FuturesLookupRead::UnparsableRowOf("V-LOOKUP-1"),
    "V-LOOKUP-1",
    LookupAnswer::Deferred
)]
#[tokio::test]
async fn test_futures_order_lookup_reports_absence_only_from_a_read_that_rules_the_order_out(
    #[case] read: FuturesLookupRead,
    #[case] venue_order_id: &str,
    #[case] expected: LookupAnswer,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new("futures-lookup-1");
    let placed = UnixNanos::from(FUTURES_LOOKUP_ORDER_MS as u64 * 1_000_000);
    let order = add_futures_limit_order_created_at(
        &cache,
        client_order_id,
        test_instrument_id(),
        OrderSide::Buy,
        test_strategy_id(),
        placed,
    );
    accept_cached_order_at(&cache, &order, venue_order_id, placed);

    let other_event = |ts_ms: i64| {
        futures_history_element(
            "OrderCancelled",
            "e-other",
            "V-OTHER-2",
            "PI_XBTUSD",
            "1000",
            "0",
            ts_ms,
        )
    };
    let history = match read {
        FuturesLookupRead::Complete => {
            futures_order_history_json(&[other_event(FUTURES_LOOKUP_ORDER_MS)])
        }
        FuturesLookupRead::TokenPageFrom(seconds_before) => {
            futures_order_history_json_with_token(&[other_event(
                FUTURES_LOOKUP_ORDER_MS - seconds_before * 1_000,
            )])
        }
        FuturesLookupRead::TokenPageWithoutEvents => futures_order_history_json_with_token(&[]),
        FuturesLookupRead::OrderOnTokenPage => {
            futures_order_history_json_with_token(&[futures_history_element(
                "OrderCancelled",
                "e-own",
                venue_order_id,
                "PI_XBTUSD",
                "1000",
                "0",
                FUTURES_LOOKUP_ORDER_MS + 1_000,
            )])
        }
        FuturesLookupRead::UndocumentedRow => {
            futures_order_history_json(&[futures_history_element(
                "OrderUndocumentedKind",
                "e-undocumented",
                "V-OTHER-3",
                "PI_XBTUSD",
                "1000",
                "0",
                FUTURES_LOOKUP_ORDER_MS,
            )])
        }
        FuturesLookupRead::UndecodableRowOf(order_uid) => {
            futures_order_history_json(&[futures_history_element_sided(
                "OrderCancelled",
                "e-undecodable",
                order_uid,
                "PI_XBTUSD",
                "Sideways",
                "1000",
                "0",
                FUTURES_LOOKUP_ORDER_MS,
            )])
        }
        FuturesLookupRead::SkippedTrigger => {
            *state.futures_open_orders_json.lock().await = Some(load_test_data(
                "http_futures_open_orders_unsized_trigger.json",
            ));
            futures_order_history_json(&[])
        }
        FuturesLookupRead::UnparsableRowOf(order_uid) => {
            futures_order_history_json(&[futures_history_element(
                "OrderCancelled",
                "e-unparsable",
                order_uid,
                "PI_XBTUSD",
                "-1",
                "0",
                FUTURES_LOOKUP_ORDER_MS,
            )])
        }
        FuturesLookupRead::UnresolvedOpenOrder => {
            *state.futures_open_orders_json.lock().await =
                Some(futures_open_orders_json("V-UNRESOLVED", "PF_NOSUCHUSD"));
            futures_order_history_json(&[])
        }
        FuturesLookupRead::UnresolvedRowOf(order_uid) => {
            futures_order_history_json(&[futures_history_element(
                "OrderCancelled",
                "e-unresolved",
                order_uid,
                "PF_NOSUCHUSD",
                "1000",
                "0",
                FUTURES_LOOKUP_ORDER_MS,
            )])
        }
    };
    *state.futures_order_history_json.lock().await = Some(history);

    // As the engine's targeted query does, the command carries the order's venue order ID
    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        Some(client_order_id),
        Some(VenueOrderId::from(venue_order_id)),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    assert_lookup_answer(result, expected, venue_order_id);
}

/// A cut-short read rules out an order only if it reaches back past the earlier of the order's
/// creation and acceptance. An order adopted from a venue report is created on the node after the
/// venue accepted it, and one the node submits is created before it, so each is dated from the
/// earlier time.
#[rstest]
#[case::adopted_page_reaching_past_the_acceptance(120, -61, LookupAnswer::Absent)]
#[case::adopted_page_starting_after_the_acceptance(120, 30, LookupAnswer::Deferred)]
#[case::submitted_page_reaching_past_the_creation(-120, -181, LookupAnswer::Absent)]
#[case::submitted_page_starting_after_the_creation(-120, -90, LookupAnswer::Deferred)]
#[tokio::test]
async fn test_futures_order_lookup_dates_an_order_from_its_earlier_known_time(
    #[case] created_after_acceptance_secs: i64,
    #[case] page_starts_after_acceptance_secs: i64,
    #[case] expected: LookupAnswer,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new("futures-lookup-dated");
    let accepted_ms = FUTURES_LOOKUP_ORDER_MS;
    let order = add_futures_limit_order_created_at(
        &cache,
        client_order_id,
        test_instrument_id(),
        OrderSide::Buy,
        test_strategy_id(),
        UnixNanos::from((accepted_ms + created_after_acceptance_secs * 1_000) as u64 * 1_000_000),
    );
    accept_cached_order_at(
        &cache,
        &order,
        "V-DATED-1",
        UnixNanos::from(accepted_ms as u64 * 1_000_000),
    );
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json_with_token(&[
            futures_history_element(
                "OrderCancelled",
                "e-other",
                "V-OTHER-2",
                "PI_XBTUSD",
                "1000",
                "0",
                accepted_ms + page_starts_after_acceptance_secs * 1_000,
            ),
        ]));

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        Some(client_order_id),
        Some(VenueOrderId::from("V-DATED-1")),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    assert_lookup_answer(result, expected, "V-DATED-1");
}

/// Spot and Futures share the `KRAKEN` venue, so each client is asked about the other's orders and
/// answers that it has none. An instrument of its own product the listing lacks reads nothing, so
/// a lookup there rules out nothing and defers.
#[rstest]
#[case::spot_client_other_product(false, "PI_XBTUSD.KRAKEN", LookupAnswer::Absent)]
#[case::spot_client_own_product_unlisted(false, "DOGE/EUR.KRAKEN", LookupAnswer::Deferred)]
#[case::futures_client_other_product(true, "BTC/USDT.KRAKEN", LookupAnswer::Absent)]
#[case::futures_client_own_product_unlisted(true, "PF_NOSUCHUSD.KRAKEN", LookupAnswer::Deferred)]
#[tokio::test]
async fn test_order_lookup_on_an_instrument_the_client_does_not_hold(
    #[case] futures: bool,
    #[case] instrument_id: &str,
    #[case] expected: LookupAnswer,
) {
    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from(instrument_id)),
        None,
        Some(VenueOrderId::from("V-UNHELD-1")),
        None,
        None,
    );

    let result = if futures {
        let (client, _rx, _cache, _state) =
            connected_client_with_command_responses(CommandResponses::default()).await;
        client.generate_order_status_report(&cmd).await
    } else {
        let (client, _rx, _cache, _state) =
            connected_spot_client_with_command_responses(CommandResponses::default()).await;
        client.generate_order_status_report(&cmd).await
    };

    assert_lookup_answer(result, expected, "V-UNHELD-1");
}

/// Without a cached order the futures lookup has no placement time to set against a cut-short
/// read, so it reports the order absent only from a complete one.
#[rstest]
#[case::complete_read(false, LookupAnswer::Absent)]
#[case::token_page(true, LookupAnswer::Deferred)]
#[tokio::test]
async fn test_futures_order_lookup_without_a_cached_order_needs_a_complete_read(
    #[case] token_page: bool,
    #[case] expected: LookupAnswer,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let event = futures_history_element(
        "OrderCancelled",
        "e-other",
        "V-OTHER-2",
        "PI_XBTUSD",
        "1000",
        "0",
        FUTURES_LOOKUP_ORDER_MS,
    );
    *state.futures_order_history_json.lock().await = Some(if token_page {
        futures_order_history_json_with_token(&[event])
    } else {
        futures_order_history_json(&[event])
    });

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        None,
        Some(VenueOrderId::from("V-UNCACHED-1")),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    assert_lookup_answer(result, expected, "V-UNCACHED-1");
}

/// An in-scope futures open order that resolves but cannot be parsed fails the read on every path.
/// A read scoped to another contract leaves it out of scope.
#[rstest]
#[case::bulk_unscoped(
    "http_futures_open_orders_unparsable.json",
    OpenOrderRead::Bulk(None),
    Err("OpenOrders: failed to parse futures order V-UNPARSABLE")
)]
#[case::bulk_scoped_to_another_contract("http_futures_open_orders_unparsable.json", OpenOrderRead::Bulk(Some("PI_XBTUSD.KRAKEN")), Ok(&["V-HELD"][..]))]
#[case::lookup_on_its_contract(
    "http_futures_open_orders_unparsable.json",
    OpenOrderRead::Lookup("PF_ETHUSD.KRAKEN", "V-UNPARSABLE"),
    Err("OpenOrders: failed to parse futures order V-UNPARSABLE")
)]
#[case::mass_status(
    "http_futures_open_orders_unparsable.json",
    OpenOrderRead::MassStatus,
    Err("OpenOrders: failed to parse futures order V-UNPARSABLE")
)]
#[tokio::test]
async fn test_futures_open_order_reads_fail_on_an_unparsable_order(
    #[case] fixture: &str,
    #[case] read: OpenOrderRead,
    #[case] expected: Result<&[&str], &str>,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(load_test_data(fixture));

    let result = read_open_orders(&client, read).await;

    match expected {
        Ok(venue_order_ids) => {
            let expected: Vec<VenueOrderId> = venue_order_ids
                .iter()
                .map(|id| VenueOrderId::from(*id))
                .collect();
            assert_eq!(result.unwrap(), expected);
        }
        Err(message) => {
            let error = result.expect_err("the unparsable open order fails the read");
            assert!(format!("{error:#}").contains(message), "{error:#}");
        }
    }
}

/// What a futures read makes of an unsized trigger.
#[derive(Clone, Copy, Debug)]
enum UnsizedTrigger {
    Skipped,
    SizedAt(&'static str),
    FailsTheRead,
}

/// An untouched reduce-only trigger the venue lists without an unfilled size takes its size from
/// the open position on its contract. With none it has no size to report, so the read skips it,
/// its history rows with it, and leaves the set incomplete rather than failing every futures order
/// read; when the open positions read fails, its size is unknown and the read fails.
#[rstest]
#[case::flat_contract(None, false, UnsizedTrigger::Skipped)]
#[case::flat_contract_with_a_history_row(None, true, UnsizedTrigger::Skipped)]
#[case::sized_by_its_contract_position(
    Some("http_futures_open_positions_xbtusd.json"),
    false,
    UnsizedTrigger::SizedAt("8000")
)]
#[case::positions_read_fails(
    Some("http_futures_open_positions_error.json"),
    false,
    UnsizedTrigger::FailsTheRead
)]
#[tokio::test]
async fn test_futures_mass_status_skips_an_unsized_trigger_only_on_a_flat_contract(
    #[case] positions_fixture: Option<&str>,
    #[case] history_row: bool,
    #[case] expected: UnsizedTrigger,
) {
    let (client, _rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    *state.futures_open_orders_json.lock().await = Some(load_test_data(
        "http_futures_open_orders_unsized_trigger.json",
    ));

    if let Some(fixture) = positions_fixture {
        *state.futures_open_positions_json.lock().await = Some(load_test_data(fixture));
    }

    if history_row {
        // A zero-quantity placement would read as filled were it reported from history
        *state.futures_order_history_json.lock().await =
            Some(futures_order_history_json(&[futures_history_element(
                "OrderPlaced",
                "e1",
                "V-TRIGGER",
                "PI_XBTUSD",
                "0",
                "0",
                1680877245500,
            )]));
    }

    let result = client.generate_mass_status(None).await;

    match expected {
        UnsizedTrigger::Skipped => {
            let mass_status = result.unwrap().expect("mass status available");
            assert!(
                !mass_status
                    .order_reports()
                    .contains_key(&VenueOrderId::from("V-TRIGGER"))
            );
            assert!(!mass_status.reports_complete());
        }
        UnsizedTrigger::SizedAt(quantity) => {
            let mass_status = result.unwrap().expect("mass status available");
            let order_reports = mass_status.order_reports();
            let report = order_reports
                .get(&VenueOrderId::from("V-TRIGGER"))
                .expect("the sized trigger is reported");
            assert_eq!(report.quantity, Quantity::from(quantity));
        }
        UnsizedTrigger::FailsTheRead => {
            let error =
                result.expect_err("an unsized trigger with no positions read fails the read");
            assert!(
                format!("{error:#}").contains(
                    "OpenOrders: futures order V-TRIGGER lists no unfilled size and the open \
                     positions read failed"
                ),
                "{error:#}"
            );
        }
    }
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

/// As [`futures_order_history_json`], for a page that hands back a continuation token.
fn futures_order_history_json_with_token(elements: &[String]) -> String {
    format!(
        r#"{{"accountUid":"acc","len":{},"elements":[{}],"serverTime":"2023-04-07T16:30:45.678Z","continuationToken":"c2ltYjE3OA=="}}"#,
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
    add_futures_limit_order_created_at(
        cache,
        client_order_id,
        instrument_id,
        side,
        strategy_id,
        UnixNanos::default(),
    )
}

fn add_futures_limit_order_created_at(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    side: OrderSide,
    strategy_id: StrategyId,
    ts_init: UnixNanos,
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
        ts_init,
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

/// Applies an `OrderSubmitted`, then an `OrderRejected`, closing the order without a venue order ID
/// as a venue rejection of the submission does.
fn mark_cached_order_rejected(cache: &Rc<RefCell<Cache>>, order: &OrderAny) {
    mark_cached_order_submitted(cache, order);
    let rejected = OrderRejected::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        test_account_id(),
        "post only would execute".into(),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        false,
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Rejected(rejected))
        .unwrap();
}

/// Leaves a cached order open with no venue order ID, as a cancel sent while the venue holds a
/// submission does, so the orders-status window can only be asked by client order ID.
fn mark_cached_order_pending_cancel_while_held(cache: &Rc<RefCell<Cache>>, order: &OrderAny) {
    mark_cached_order_submitted(cache, order);
    let pending_cancel = OrderPendingCancel::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(test_account_id()),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None,
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::PendingCancel(pending_cancel))
        .unwrap();
}

/// An `/orders/status` body with one entry per `(orderId, cliOrdId, symbol, side, status, filled)`.
fn orders_status_body(entries: &[(&str, &str, &str, &str, &str, u32)]) -> String {
    orders_status_body_updated_at(entries, "2026-09-12T04:05:06.300Z")
}

/// As `orders_status_body`, with every entry last updated at `last_update`.
fn orders_status_body_updated_at(
    entries: &[(&str, &str, &str, &str, &str, u32)],
    last_update: &str,
) -> String {
    let orders: Vec<Value> = entries
        .iter()
        .map(|(order_id, cli_ord_id, symbol, side, status, filled)| {
            json!({
                "order": {
                    "type": "ORDER",
                    "orderId": order_id,
                    "cliOrdId": cli_ord_id,
                    "symbol": symbol,
                    "side": side,
                    "quantity": 5,
                    "filled": filled,
                    "limitPrice": 50000.0,
                    "reduceOnly": false,
                    "timestamp": "2026-09-12T04:05:06.100Z",
                    "lastUpdateTimestamp": last_update,
                    "algoId": null,
                    "priceTriggerOptions": null,
                    "triggerTime": null
                },
                "status": status,
                "updateReason": null,
                "error": null
            })
        })
        .collect();
    json!({ "result": "success", "orders": orders }).to_string()
}

/// Applies an `OrderAccepted` carrying the venue order ID, mirroring how the
/// engine records a venue ID on a cached order once the venue acknowledges it.
fn set_venue_order_id_on_cached_order(
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    venue_order_id: &str,
) {
    accept_cached_order_at(cache, order, venue_order_id, UnixNanos::default());
}

/// As [`set_venue_order_id_on_cached_order`], with the acceptance stamped at `ts_event`.
fn accept_cached_order_at(
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    venue_order_id: &str,
    ts_event: UnixNanos,
) {
    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from(venue_order_id),
        test_account_id(),
        UUID4::new(),
        ts_event,
        ts_event,
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
    let part_filled_order =
        add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-004"));
    set_venue_order_id_on_cached_order(&cache, &part_filled_order, "V-HELD-004");
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

/// A client order ID longer than 18 characters.
const LONG_CLIENT_ORDER_ID: &str = "O-20260912-040506-001-001-1";
/// Another order's ID that shortens to the same 18 characters.
const LONG_CLIENT_ORDER_ID_OTHER_DAY: &str = "O-20260911-040506-001-001-1";
/// `O` plus the last 17 characters of either ID above, the form the venue holds an order sent with
/// a shortened ID under.
const SHORTENED_CLIENT_ORDER_ID: &str = "O-040506-001-001-1";

/// A window report carries the full client order ID of the cached order it describes, whether the
/// adapter asks about that order by venue order ID or, holding none, by both forms of its client
/// order ID: reconciliation matches a report carrying a client order ID by that ID alone. A
/// canceled order with executions passes unpriced.
#[rstest]
#[case::bulk_by_venue_order_id(false, true)]
#[case::bulk_by_client_order_id(false, false)]
#[case::mass_status_by_venue_order_id(true, true)]
#[case::mass_status_by_client_order_id(true, false)]
#[tokio::test]
async fn test_futures_orders_status_window_report_carries_the_full_client_order_id(
    #[case] mass_status: bool,
    #[case] has_venue_order_id: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
    if has_venue_order_id {
        set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-010");
    } else {
        mark_cached_order_pending_cancel_while_held(&cache, &order);
    }
    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
        "V-HELD-010",
        SHORTENED_CLIENT_ORDER_ID,
        "PI_XBTUSD",
        "buy",
        "CANCELLED",
        2,
    )]));

    let reports: Vec<OrderStatusReport> = if mass_status {
        let mass_status = client
            .generate_mass_status(None)
            .await
            .unwrap()
            .expect("mass status available");
        mass_status.order_reports().values().cloned().collect()
    } else {
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
        client.generate_order_status_reports(&cmd).await.unwrap()
    };

    let report = reports
        .iter()
        .find(|report| report.venue_order_id == VenueOrderId::from("V-HELD-010"))
        .expect("held order reported from the orders-status window");

    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from(LONG_CLIENT_ORDER_ID))
    );
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.filled_qty, Quantity::from("2"));
    assert_eq!(report.avg_px, None);

    let body = state
        .orders_status_request_body
        .lock()
        .await
        .clone()
        .expect("orders-status request recorded");
    // A held order is asked about under both forms the venue can hold it under
    let expected_body = if has_venue_order_id {
        "orderIds=V-HELD-010".to_string()
    } else {
        format!("cliOrdIds={LONG_CLIENT_ORDER_ID}&cliOrdIds={SHORTENED_CLIENT_ORDER_ID}")
    };
    assert_eq!(body, expected_body);
}

/// A client order ID, in full or shortened, names an order queried by client order ID only when
/// exactly one queried order and exactly one report without an indexed venue order ID hold it, and
/// the report's instrument and side are the order's. Two reports that would name one order, one
/// echoing each form, name neither. Any other report is withheld, since it cannot be tied to
/// exactly one cached order. A report whose venue order ID the cache indexes keeps that order's ID.
#[rstest]
#[case::one_queried_order_and_one_report(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy")],
    &[("V-HELD-011", Some(LONG_CLIENT_ORDER_ID), 1)],
)]
#[case::two_queried_orders_share_it(
    &[LONG_CLIENT_ORDER_ID, LONG_CLIENT_ORDER_ID_OTHER_DAY],
    &[],
    &[("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy")],
    &[("V-HELD-011", None, 0)],
)]
#[case::two_reports_share_it(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[
        ("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
        ("V-HELD-012", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
    ],
    &[("V-HELD-011", None, 0), ("V-HELD-012", None, 0)],
)]
#[case::indexed_report_keeps_its_order(
    &[LONG_CLIENT_ORDER_ID],
    &[(LONG_CLIENT_ORDER_ID_OTHER_DAY, "V-HELD-012")],
    &[
        ("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
        ("V-HELD-012", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
    ],
    // Both requests return both entries, which the response holds once each
    &[("V-HELD-011", Some(LONG_CLIENT_ORDER_ID), 1), ("V-HELD-012", Some(LONG_CLIENT_ORDER_ID_OTHER_DAY), 1)],
)]
#[case::unshortened_client_order_id(
    &["futures-held-014"],
    &[],
    &[("V-HELD-014", "futures-held-014", "PI_XBTUSD", "buy")],
    &[("V-HELD-014", Some("futures-held-014"), 1)],
)]
#[case::another_instrument(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PF_ETHUSD", "buy")],
    &[("V-HELD-011", None, 0)],
)]
#[case::other_side(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-011", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "sell")],
    &[("V-HELD-011", None, 0)],
)]
#[case::unqueried_report(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-013", "futures-held-013", "PI_XBTUSD", "buy")],
    &[("V-HELD-013", None, 0)],
)]
#[case::full_id_one_queried_order_and_one_report(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-016", LONG_CLIENT_ORDER_ID, "PI_XBTUSD", "buy")],
    &[("V-HELD-016", Some(LONG_CLIENT_ORDER_ID), 1)],
)]
#[case::full_id_two_reports_share_it(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[
        ("V-HELD-016", LONG_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
        ("V-HELD-017", LONG_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
    ],
    &[("V-HELD-016", None, 0), ("V-HELD-017", None, 0)],
)]
#[case::full_and_shortened_reports_name_one_order(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[
        ("V-HELD-016", LONG_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
        ("V-HELD-018", SHORTENED_CLIENT_ORDER_ID, "PI_XBTUSD", "buy"),
    ],
    &[("V-HELD-016", None, 0), ("V-HELD-018", None, 0)],
)]
#[case::full_id_another_instrument(
    &[LONG_CLIENT_ORDER_ID],
    &[],
    &[("V-HELD-016", LONG_CLIENT_ORDER_ID, "PF_ETHUSD", "buy")],
    &[("V-HELD-016", None, 0)],
)]
#[tokio::test]
async fn test_futures_orders_status_window_report_names_only_an_unambiguous_order(
    #[case] held: &[&str],
    #[case] accepted: &[(&str, &str)],
    #[case] window: &[(&str, &str, &str, &str)],
    #[case] expected: &[(&str, Option<&str>, usize)],
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    for client_order_id in held {
        let order = add_limit_order_to_cache(&cache, ClientOrderId::new(client_order_id));
        mark_cached_order_pending_cancel_while_held(&cache, &order);
    }

    for (client_order_id, venue_order_id) in accepted {
        let order = add_limit_order_to_cache(&cache, ClientOrderId::new(client_order_id));
        set_venue_order_id_on_cached_order(&cache, &order, venue_order_id);
    }

    let entries: Vec<(&str, &str, &str, &str, &str, u32)> = window
        .iter()
        .map(|(venue_order_id, cli_ord_id, symbol, side)| {
            (
                *venue_order_id,
                *cli_ord_id,
                *symbol,
                *side,
                "ENTERED_BOOK",
                0,
            )
        })
        .collect();
    *state.orders_status_response.lock().await = Some(orders_status_body(&entries));

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

    for (venue_order_id, client_order_id, count) in expected {
        let named: Vec<Option<ClientOrderId>> = reports
            .iter()
            .filter(|report| report.venue_order_id == VenueOrderId::from(*venue_order_id))
            .map(|report| report.client_order_id)
            .collect();
        let expected_named = vec![client_order_id.map(ClientOrderId::from); *count];
        assert_eq!(named, expected_named, "{venue_order_id}");
    }
}

/// The window entry's placement time, 2026-09-12T04:05:06.100Z.
const WINDOW_ENTRY_PLACED_NS: u64 = 1_789_185_906_100_000_000;

/// A client order ID, full or shortened, names only a venue order placed no more than five
/// seconds, the allowance for skew between the node's and the venue's clocks, before the cached
/// order's creation, so an older order whose client order ID shortens alike, or that reused it, is
/// withheld.
#[rstest]
#[case::created_before_placement(WINDOW_ENTRY_PLACED_NS - 1_000_000_000, true)]
#[case::created_at_placement(WINDOW_ENTRY_PLACED_NS, true)]
#[case::created_within_clock_skew(WINDOW_ENTRY_PLACED_NS + 200_000_000, true)]
#[case::created_at_clock_skew_bound(WINDOW_ENTRY_PLACED_NS + 5_000_000_000, true)]
#[case::created_past_clock_skew(WINDOW_ENTRY_PLACED_NS + 5_000_000_001, false)]
#[case::created_a_day_after_placement(WINDOW_ENTRY_PLACED_NS + 86_400_000_000_000, false)]
#[tokio::test]
async fn test_futures_orders_status_window_report_names_an_order_placed_after_its_creation(
    #[case] created_ns: u64,
    #[case] named: bool,
    #[values(LONG_CLIENT_ORDER_ID, SHORTENED_CLIENT_ORDER_ID)] echoed: &str,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let order = add_futures_limit_order_created_at(
        &cache,
        ClientOrderId::new(LONG_CLIENT_ORDER_ID),
        test_instrument_id(),
        OrderSide::Buy,
        test_strategy_id(),
        UnixNanos::from(created_ns),
    );
    mark_cached_order_pending_cancel_while_held(&cache, &order);
    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
        "V-HELD-015",
        echoed,
        "PI_XBTUSD",
        "buy",
        "ENTERED_BOOK",
        0,
    )]));

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

    let expected: Vec<Option<ClientOrderId>> = if named {
        vec![Some(ClientOrderId::from(LONG_CLIENT_ORDER_ID))]
    } else {
        Vec::new()
    };
    let reported: Vec<Option<ClientOrderId>> = reports
        .iter()
        .filter(|report| report.venue_order_id == VenueOrderId::from("V-HELD-015"))
        .map(|report| report.client_order_id)
        .collect();
    assert_eq!(reported, expected);
}

/// When the venue-ID and client-order-ID requests both return one order, one report survives, the
/// order's latest window state whichever request ran later: a closed state over an open one, else
/// the later venue update, else the larger filled quantity. The rules that follow apply to it, so a
/// fully executed one is withheld and leaves mass status incomplete.
#[rstest]
#[case::later_request_cancels(
    ("ENTERED_BOOK", 0, 300),
    ("CANCELLED", 0, 400),
    Some((OrderStatus::Canceled, "0", 400)),
)]
#[case::closed_state_outranks_a_later_open_update(
    ("CANCELLED", 0, 300),
    ("ENTERED_BOOK", 0, 400),
    Some((OrderStatus::Canceled, "0", 300)),
)]
#[case::later_update_carries_a_fill(
    ("ENTERED_BOOK", 0, 300),
    ("ENTERED_BOOK", 2, 400),
    Some((OrderStatus::PartiallyFilled, "2", 400)),
)]
#[case::later_update_without_a_fill(
    ("ENTERED_BOOK", 0, 300),
    ("ENTERED_BOOK", 0, 400),
    Some((OrderStatus::Accepted, "0", 400)),
)]
#[case::earlier_request_updated_later(
    ("ENTERED_BOOK", 0, 400),
    ("ENTERED_BOOK", 0, 300),
    Some((OrderStatus::Accepted, "0", 400)),
)]
#[case::equal_updates_keep_the_later_larger_fill(
    ("ENTERED_BOOK", 0, 300),
    ("ENTERED_BOOK", 2, 300),
    Some((OrderStatus::PartiallyFilled, "2", 300)),
)]
#[case::equal_updates_keep_the_earlier_larger_fill(
    ("ENTERED_BOOK", 2, 300),
    ("ENTERED_BOOK", 0, 300),
    Some((OrderStatus::PartiallyFilled, "2", 300)),
)]
#[case::later_request_fully_executes(("ENTERED_BOOK", 0, 300), ("FULLY_EXECUTED", 5, 400), None)]
#[case::same_state(
    ("ENTERED_BOOK", 0, 300),
    ("ENTERED_BOOK", 0, 300),
    Some((OrderStatus::Accepted, "0", 300)),
)]
#[tokio::test]
async fn test_futures_orders_status_window_keeps_the_latest_state_both_requests_return(
    #[case] by_venue_order_id: (&str, u32, u64),
    #[case] by_client_order_id: (&str, u32, u64),
    #[case] expected: Option<(OrderStatus, &str, u64)>,
    #[values(false, true)] mass_status: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let held = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
    mark_cached_order_pending_cancel_while_held(&cache, &held);
    // Accepted with a venue order ID, its client order ID shortens as the held order's does
    let accepted =
        add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID_OTHER_DAY));
    set_venue_order_id_on_cached_order(&cache, &accepted, "V-HELD-012");

    // Each update stamp is a millisecond within the window entry's placement second
    let updated_at = |ms: u64| format!("2026-09-12T04:05:06.{ms:03}Z");
    let (status, filled, update_ms) = by_venue_order_id;
    let (later_status, later_filled, later_update_ms) = by_client_order_id;
    state.orders_status_sequence.lock().await.extend([
        orders_status_body_updated_at(
            &[(
                "V-HELD-012",
                SHORTENED_CLIENT_ORDER_ID,
                "PI_XBTUSD",
                "buy",
                status,
                filled,
            )],
            &updated_at(update_ms),
        ),
        orders_status_body_updated_at(
            &[
                (
                    "V-HELD-011",
                    SHORTENED_CLIENT_ORDER_ID,
                    "PI_XBTUSD",
                    "buy",
                    "ENTERED_BOOK",
                    0,
                ),
                (
                    "V-HELD-012",
                    SHORTENED_CLIENT_ORDER_ID,
                    "PI_XBTUSD",
                    "buy",
                    later_status,
                    later_filled,
                ),
            ],
            &updated_at(later_update_ms),
        ),
    ]);

    let kept: Vec<(OrderStatus, Quantity, UnixNanos)> = if mass_status {
        let mass_status = client
            .generate_mass_status(None)
            .await
            .unwrap()
            .expect("mass status available");
        assert_eq!(mass_status.reports_complete(), expected.is_some());
        mass_status
            .order_reports()
            .get(&VenueOrderId::from("V-HELD-012"))
            .map(|report| (report.order_status, report.filled_qty, report.ts_last))
            .into_iter()
            .collect()
    } else {
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
        client
            .generate_order_status_reports(&cmd)
            .await
            .unwrap()
            .iter()
            .filter(|report| report.venue_order_id == VenueOrderId::from("V-HELD-012"))
            .map(|report| (report.order_status, report.filled_qty, report.ts_last))
            .collect()
    };

    // The placement stamp is the window entry's 100th millisecond
    let expected: Vec<(OrderStatus, Quantity, UnixNanos)> = expected
        .map(|(status, filled, update_ms)| {
            (
                status,
                Quantity::from(filled),
                UnixNanos::from(WINDOW_ENTRY_PLACED_NS - 100_000_000 + update_ms * 1_000_000),
            )
        })
        .into_iter()
        .collect();
    assert_eq!(kept, expected);

    // The venue-ID request ran first, the client-order-ID request last
    let body = state
        .orders_status_request_body
        .lock()
        .await
        .clone()
        .expect("orders-status request recorded");
    assert_eq!(
        body,
        format!("cliOrdIds={LONG_CLIENT_ORDER_ID}&cliOrdIds={SHORTENED_CLIENT_ORDER_ID}")
    );
}

/// A window report the mass status withholds, a fully executed one or one it cannot attribute to
/// a queried order, leaves the set incomplete; a report it keeps does not.
#[rstest]
#[case::canceled_report_kept(true, "futures-held-020", "CANCELLED", 2, true)]
#[case::fully_executed_report_withheld(true, "futures-held-020", "FULLY_EXECUTED", 1, false)]
#[case::unattributed_report_withheld(false, "futures-held-013", "ENTERED_BOOK", 0, false)]
#[tokio::test]
async fn test_futures_mass_status_marks_a_withheld_window_report_incomplete(
    #[case] has_venue_order_id: bool,
    #[case] cli_ord_id: &str,
    #[case] status: &str,
    #[case] filled: u32,
    #[case] complete: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    if has_venue_order_id {
        let order = add_limit_order_to_cache(&cache, ClientOrderId::new("futures-held-020"));
        set_venue_order_id_on_cached_order(&cache, &order, "V-HELD-020");
    } else {
        let order = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
        mark_cached_order_pending_cancel_while_held(&cache, &order);
    }
    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
        "V-HELD-020",
        cli_ord_id,
        "PI_XBTUSD",
        "buy",
        status,
        filled,
    )]));

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status available");

    assert_eq!(mass_status.reports_complete(), complete);
    assert_eq!(
        mass_status
            .order_reports()
            .contains_key(&VenueOrderId::from("V-HELD-020")),
        complete,
    );
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

/// Kraken Futures takes a client order ID of up to 100 characters, so the order is sent with its
/// full ID, and one past the limit is denied before it reaches the venue.
#[rstest]
#[case::default_id(LONG_CLIENT_ORDER_ID.len(), true)]
#[case::at_the_limit(100, true)]
#[case::past_the_limit(101, false)]
#[tokio::test]
async fn test_futures_submit_sends_the_full_client_order_id(
    #[case] len: usize,
    #[case] sent: bool,
) {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let id = if len == LONG_CLIENT_ORDER_ID.len() {
        LONG_CLIENT_ORDER_ID.to_string()
    } else {
        format!("O-{}", "1".repeat(len - 2))
    };
    let client_order_id = ClientOrderId::new(&id);
    let order = add_limit_order_to_cache(&cache, client_order_id);

    client.submit_order(submit_order_command(&order)).unwrap();

    if sent {
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.submit_request_count.load(Ordering::Relaxed) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the order is sent");
        let requests = state.command_requests.lock().await.clone();
        assert!(
            requests.iter().any(|request| {
                request.starts_with("/derivatives/api/v3/sendorder")
                    && request.contains(&format!("cliOrdId={id}"))
            }),
            "{requests:?}"
        );
    } else {
        let events =
            order_events_until_denied(&mut rx, &[client_order_id], &[client_order_id]).await;
        assert_denied_alone(&events, &[client_order_id]);
        assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 0);
    }
}

/// A list goes out whole or not at all: an open member whose client order ID is past the 100
/// characters Kraken Futures takes denies every open member before any is submitted, so a bracket
/// cannot reach the venue without a leg. A closed member is not denied, and its ID does not
/// count.
#[rstest]
#[case::open_members(None, &[0, 1])]
#[case::closed_short_member(Some(0), &[1])]
#[case::closed_long_member(Some(1), &[])]
#[tokio::test]
async fn test_futures_submit_order_list_with_a_long_client_order_id_is_denied_whole(
    #[case] closed: Option<usize>,
    #[case] denied: &[usize],
) {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let long_id = format!("O-{}", "1".repeat(99));
    let members = [
        add_limit_order_to_cache(&cache, ClientOrderId::new("list-member-1")),
        add_limit_order_to_cache(&cache, ClientOrderId::new(&long_id)),
    ];

    if let Some(index) = closed {
        deny_cached_order(&cache, &members[index]);
    }

    client
        .submit_order_list(submit_order_list_command(
            "OL-LONG-1",
            test_instrument_id(),
            &members,
        ))
        .unwrap();

    let ids: Vec<ClientOrderId> = members.iter().map(Order::client_order_id).collect();
    let denied: Vec<ClientOrderId> = denied.iter().map(|index| ids[*index]).collect();

    if denied.is_empty() {
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.batch_submit_request_count.load(Ordering::Relaxed) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the open member is sent");
        let mut denials = Vec::new();

        while let Ok(event) = rx.try_recv() {
            if let ExecutionEvent::Order(OrderEventAny::Denied(event)) = event {
                denials.push(event.client_order_id);
            }
        }
        assert!(denials.is_empty(), "{denials:?}");
    } else {
        let events = order_events_until_denied(&mut rx, &ids, &denied).await;
        assert_denied_alone(&events, &denied);
        assert_eq!(state.batch_submit_request_count.load(Ordering::Relaxed), 0);
        assert_eq!(state.submit_request_count.load(Ordering::Relaxed), 0);
    }
}

/// The order events of `client_order_ids`, received until each of `denied` is denied.
async fn order_events_until_denied(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    client_order_ids: &[ClientOrderId],
    denied: &[ClientOrderId],
) -> Vec<OrderEventAny> {
    let mut events: Vec<OrderEventAny> = Vec::new();
    let all_denied = |events: &[OrderEventAny]| {
        denied.iter().all(|id| {
            events.iter().any(|event| {
                matches!(event, OrderEventAny::Denied(_)) && event.client_order_id() == *id
            })
        })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !all_denied(&events) {
            let event = rx.recv().await.expect("Execution event channel closed");
            if let ExecutionEvent::Order(event) = event
                && client_order_ids.contains(&event.client_order_id())
            {
                events.push(event);
            }
        }
    })
    .await
    .expect("Timed out waiting for the denials");
    events
}

/// Asserts `events` are one denial for the client order ID length per order in `denied`, in order,
/// and nothing else.
fn assert_denied_alone(events: &[OrderEventAny], denied: &[ClientOrderId]) {
    let received: Vec<ClientOrderId> = events
        .iter()
        .map(|event| match event {
            OrderEventAny::Denied(event) => {
                assert!(event.reason.contains("at most 100 characters"), "{event:?}");
                event.client_order_id
            }
            other => panic!("Expected only OrderDenied events, was {other:?}"),
        })
        .collect();
    assert_eq!(received, denied);
}

/// Denies the cached order, closing it.
fn deny_cached_order(cache: &Rc<RefCell<Cache>>, order: &OrderAny) {
    let denied = OrderDenied::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        "closed before the list is sent".into(),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Denied(denied))
        .unwrap();
}

/// `query_order` names the report by the cached order its venue order ID indexes, so a command
/// whose venue order ID belongs to another cached order cannot pass that order off as the queried
/// one.
#[rstest]
#[tokio::test]
async fn test_futures_query_order_names_the_report_by_the_cached_order() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let owner = add_limit_order_to_cache(&cache, ClientOrderId::new("query-owner-1"));
    set_venue_order_id_on_cached_order(&cache, &owner, "V-OWNED-1");
    add_limit_order_to_cache(&cache, ClientOrderId::new("query-other-1"));
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-OWNED-1", "PI_XBTUSD"));

    client
        .query_order(QueryOrder::new(
            test_trader_id(),
            None,
            test_strategy_id(),
            test_instrument_id(),
            ClientOrderId::new("query-other-1"),
            Some(VenueOrderId::from("V-OWNED-1")),
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
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("query-owner-1"))
    );
}

/// A cancel or edit names the order by one identifier: its venue order ID when known, else its
/// full client order ID. An order sent with a shortened client order ID is held under that form,
/// so its full ID beside the venue order ID could name no order.
#[rstest]
#[case::cancel_with_venue_order_id(false, true)]
#[case::cancel_without_venue_order_id(false, false)]
#[case::modify_with_venue_order_id(true, true)]
#[case::modify_without_venue_order_id(true, false)]
#[tokio::test]
async fn test_futures_cancel_and_modify_name_the_order_by_one_identifier(
    #[case] modify: bool,
    #[case] with_venue_order_id: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new(LONG_CLIENT_ORDER_ID);
    let order = add_limit_order_to_cache(&cache, client_order_id);
    let venue_order_id = with_venue_order_id.then(|| VenueOrderId::from("V-ONE-ID-1"));

    if modify {
        let mut cmd = modify_order_command(&order, VenueOrderId::from("V-ONE-ID-1"));
        cmd.venue_order_id = venue_order_id;
        client.modify_order(cmd).unwrap();
    } else {
        let mut cmd = cancel_order_command(client_order_id, VenueOrderId::from("V-ONE-ID-1"));
        cmd.venue_order_id = venue_order_id;
        client.cancel_order(cmd).unwrap();
    }

    let path = if modify {
        "/derivatives/api/v3/editorder"
    } else {
        "/derivatives/api/v3/cancelorder"
    };
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(request) = state
                .command_requests
                .lock()
                .await
                .iter()
                .find(|request| request.starts_with(path))
            {
                return request.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the command is sent");

    let names_venue_order_id = request.contains("V-ONE-ID-1");
    let names_client_order_id = request.contains(&format!("cliOrdId={LONG_CLIENT_ORDER_ID}"));
    assert_eq!(
        (names_venue_order_id, names_client_order_id),
        (with_venue_order_id, !with_venue_order_id),
        "{request}"
    );
}

/// The single-order query takes the row carrying the order's venue order ID when that is known.
/// Otherwise a row is the order's when it echoes the client order ID, in full or in the shortened
/// form the venue holds an order sent with one under, on the order's instrument and side and
/// placed no more than the clock skew allowance before its creation, a fill's execution standing
/// in for its placement; an older row sharing the ID, one on another instrument or side, or one
/// the cache indexes for another order, even one whose full ID is this order's shortened one, is
/// another order, while one it indexes for this order fits even before the order carries that
/// venue order ID. With the venue order ID unknown, one read's rows or fills of two venue orders
/// that can be the order leave the answer open, as does a row of a venue order the cache indexes
/// for no order, or with no row such a fill, that another cached order sent without a venue order
/// ID, holding its echo as its full ID, can be by its instrument, side and creation; with it known,
/// so does a row or fill of another venue order that can be the order when no row or fill carries
/// the ID. Either way the query fails and reconciliation defers.
#[rstest]
#[case::venue_order_id_row_wins(
    true,
    LookupRow::HistoryOfAnother(LONG_CLIENT_ORDER_ID),
    false,
    Ok(Some("V-OWN-1"))
)]
#[case::full_id_row_without_venue_order_id(
    false,
    LookupRow::HistoryOfAnother(LONG_CLIENT_ORDER_ID),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::shortened_history_row(
    false,
    LookupRow::HistoryOfAnother(SHORTENED_CLIENT_ORDER_ID),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::shortened_window_row(
    false,
    LookupRow::Window(SHORTENED_CLIENT_ORDER_ID),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::shortened_fill(
    false,
    LookupRow::Fill(SHORTENED_CLIENT_ORDER_ID),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::older_history_row(
    false,
    LookupRow::HistoryOfAnother(SHORTENED_CLIENT_ORDER_ID),
    true,
    Ok(None)
)]
#[case::older_window_row(false, LookupRow::Window(SHORTENED_CLIENT_ORDER_ID), true, Ok(None))]
#[case::older_fill(false, LookupRow::Fill(SHORTENED_CLIENT_ORDER_ID), true, Ok(None))]
#[case::window_row_on_another_instrument(
    false,
    LookupRow::WindowOnAnotherInstrument,
    false,
    Ok(None)
)]
#[case::history_row_of_the_other_side(false, LookupRow::HistoryOfTheOtherSide, false, Ok(None))]
#[case::rows_of_two_venue_orders(
    false,
    LookupRow::TwoHistoryRows,
    false,
    Err("several venue orders")
)]
#[case::full_id_row_of_another_venue_order(
    true,
    LookupRow::OnlyAnotherWithTheId,
    false,
    Err("carries its client order ID")
)]
#[case::full_id_window_row_of_another_venue_order(
    true,
    LookupRow::Window(LONG_CLIENT_ORDER_ID),
    false,
    Err("carries its client order ID")
)]
#[case::full_id_fill_of_another_venue_order(
    true,
    LookupRow::Fill(LONG_CLIENT_ORDER_ID),
    false,
    Err("carries its client order ID")
)]
#[case::window_rows_of_two_venue_orders(
    false,
    LookupRow::TwoWindowRows,
    false,
    Err("several venue orders")
)]
#[case::fills_of_two_venue_orders(
    false,
    LookupRow::FillsOfTwoOrders,
    false,
    Err("carries its client order ID")
)]
#[case::window_row_of_another_cached_order(
    false,
    LookupRow::WindowOfAnotherCachedOrder,
    false,
    Ok(None)
)]
#[case::fill_of_another_cached_order(false, LookupRow::FillOfAnotherCachedOrder, false, Ok(None))]
#[case::history_row_of_a_short_id_order(false, LookupRow::HistoryOfShortIdOrder, false, Ok(None))]
#[case::window_row_of_a_short_id_order(false, LookupRow::WindowOfShortIdOrder, false, Ok(None))]
#[case::fill_of_a_short_id_order(false, LookupRow::FillOfShortIdOrder, false, Ok(None))]
#[case::fill_of_a_short_id_order_adopted_after_the_fill(
    false,
    LookupRow::FillOfAdoptedShortIdOrder,
    false,
    Ok(None)
)]
#[case::own_row_beside_a_short_id_order(
    false,
    LookupRow::HistoryOfShortIdOrderAndTheOrder,
    false,
    Ok(Some("V-OWN-1"))
)]
#[case::history_row_of_a_short_id_order_with_venue_order_id(
    true,
    LookupRow::HistoryOfShortIdOrder,
    false,
    Ok(None)
)]
#[case::window_row_of_a_short_id_order_with_venue_order_id(
    true,
    LookupRow::WindowOfShortIdOrder,
    false,
    Ok(None)
)]
#[case::fill_of_a_short_id_order_with_venue_order_id(
    true,
    LookupRow::FillOfShortIdOrder,
    false,
    Ok(None)
)]
#[case::own_row_indexed_while_the_query_reads(
    false,
    LookupRow::HistoryIndexedForTheOrder,
    false,
    Ok(Some("V-OWN-1"))
)]
#[case::history_row_of_an_unindexed_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Fits),
    false,
    Err("the full ID of another cached order")
)]
#[case::window_row_of_an_unindexed_short_id_order(
    false,
    LookupRow::WindowOfUnindexedShortIdOrder,
    false,
    Err("the full ID of another cached order")
)]
#[case::fill_of_an_unindexed_short_id_order(
    false,
    LookupRow::FillOfUnindexedShortIdOrder,
    false,
    Err("carries its client order ID")
)]
#[case::history_row_of_an_unindexed_short_id_order_with_venue_order_id(
    true,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Fits),
    false,
    Err("carries its client order ID")
)]
#[case::row_placed_before_an_unindexed_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::CreatedAfterTheRow),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::row_on_the_other_side_of_an_unindexed_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::OnTheOtherSide),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::row_on_another_instrument_than_an_unindexed_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::OnAnotherInstrument),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::row_beside_a_short_id_order_holding_another_venue_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::HoldsAnotherVenueOrder),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::row_beside_a_denied_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Denied),
    false,
    Ok(Some("V-OTHER-1"))
)]
#[case::row_beside_a_rejected_short_id_order(
    false,
    LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Rejected),
    false,
    Err("the full ID of another cached order")
)]
#[case::no_row_carries_the_id(false, LookupRow::None, false, Ok(None))]
#[tokio::test]
async fn test_futures_order_lookup_takes_only_the_queried_order(
    #[case] with_venue_order_id: bool,
    #[case] row: LookupRow,
    #[case] created_after_the_rows: bool,
    #[case] expected: Result<Option<&str>, &str>,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new(LONG_CLIENT_ORDER_ID);
    // A day after now, after every row below, or at the epoch, before them
    let created = if created_after_the_rows {
        UnixNanos::from(get_atomic_clock_realtime().get_time_ns().as_u64() + 86_400_000_000_000)
    } else {
        UnixNanos::default()
    };
    let order = add_futures_limit_order_created_at(
        &cache,
        client_order_id,
        test_instrument_id(),
        OrderSide::Buy,
        test_strategy_id(),
        created,
    );

    if with_venue_order_id {
        set_venue_order_id_on_cached_order(&cache, &order, "V-OWN-1");
    } else {
        // A cached order without a venue order ID is in flight when reconciliation queries it
        mark_cached_order_submitted(&cache, &order);
    }

    let history_row = |order_uid: &str, client_id: &str| {
        futures_history_element(
            "OrderCancelled",
            &format!("e-{order_uid}"),
            order_uid,
            "PI_XBTUSD",
            "1000",
            "0",
            1_700_000_000_000,
        )
        .replace(r#""clientId":"""#, &format!(r#""clientId":"{client_id}""#))
    };

    match row {
        LookupRow::HistoryOfAnother(echoed) => {
            let mut elements = vec![history_row("V-OTHER-1", echoed)];
            if with_venue_order_id {
                elements.push(history_row("V-OWN-1", ""));
            }
            *state.futures_order_history_json.lock().await =
                Some(futures_order_history_json(&elements));
        }
        LookupRow::OnlyAnotherWithTheId => {
            *state.futures_order_history_json.lock().await =
                Some(futures_order_history_json(&[history_row(
                    "V-OTHER-1",
                    LONG_CLIENT_ORDER_ID,
                )]));
        }
        LookupRow::TwoHistoryRows => {
            *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
                history_row("V-OTHER-1", LONG_CLIENT_ORDER_ID),
                history_row("V-OTHER-2", SHORTENED_CLIENT_ORDER_ID),
            ]));
        }
        LookupRow::WindowOnAnotherInstrument => {
            *state.orders_status_response.lock().await = Some(orders_status_body(&[(
                "V-OTHER-1",
                LONG_CLIENT_ORDER_ID,
                "PF_ETHUSD",
                "buy",
                "ENTERED_BOOK",
                0,
            )]));
        }
        LookupRow::HistoryOfTheOtherSide => {
            let row = futures_history_element_sided(
                "OrderCancelled",
                "e-sell",
                "V-OTHER-1",
                "PI_XBTUSD",
                "Sell",
                "1000",
                "0",
                1_700_000_000_000,
            )
            .replace(
                r#""clientId":"""#,
                &format!(r#""clientId":"{LONG_CLIENT_ORDER_ID}""#),
            );
            *state.futures_order_history_json.lock().await =
                Some(futures_order_history_json(&[row]));
        }
        LookupRow::Window(echoed) => {
            *state.orders_status_response.lock().await = Some(orders_status_body(&[(
                "V-OTHER-1",
                echoed,
                "PI_XBTUSD",
                "buy",
                "ENTERED_BOOK",
                0,
            )]));
        }
        LookupRow::Fill(echoed) => {
            *state.fills_response.lock().await =
                Some(recent_fills_body(&[("F-ECHO-1", "V-OTHER-1", echoed)]));
        }
        LookupRow::TwoWindowRows => {
            *state.orders_status_response.lock().await = Some(orders_status_body(&[
                (
                    "V-OTHER-1",
                    LONG_CLIENT_ORDER_ID,
                    "PI_XBTUSD",
                    "buy",
                    "ENTERED_BOOK",
                    0,
                ),
                (
                    "V-OTHER-2",
                    SHORTENED_CLIENT_ORDER_ID,
                    "PI_XBTUSD",
                    "buy",
                    "ENTERED_BOOK",
                    0,
                ),
            ]));
        }
        LookupRow::FillsOfTwoOrders => {
            *state.fills_response.lock().await = Some(recent_fills_body(&[
                ("F-ECHO-1", "V-OTHER-1", LONG_CLIENT_ORDER_ID),
                ("F-ECHO-2", "V-OTHER-2", LONG_CLIENT_ORDER_ID),
            ]));
        }
        LookupRow::WindowOfAnotherCachedOrder | LookupRow::FillOfAnotherCachedOrder => {
            let other = add_limit_order_to_cache(
                &cache,
                ClientOrderId::new(LONG_CLIENT_ORDER_ID_OTHER_DAY),
            );
            set_venue_order_id_on_cached_order(&cache, &other, "V-OTHER-1");

            if matches!(row, LookupRow::WindowOfAnotherCachedOrder) {
                *state.orders_status_response.lock().await = Some(orders_status_body(&[(
                    "V-OTHER-1",
                    SHORTENED_CLIENT_ORDER_ID,
                    "PI_XBTUSD",
                    "buy",
                    "ENTERED_BOOK",
                    0,
                )]));
            } else {
                *state.fills_response.lock().await = Some(recent_fills_body(&[(
                    "F-OTHER-1",
                    "V-OTHER-1",
                    SHORTENED_CLIENT_ORDER_ID,
                )]));
            }
        }
        LookupRow::HistoryOfShortIdOrder
        | LookupRow::HistoryOfShortIdOrderAndTheOrder
        | LookupRow::WindowOfShortIdOrder
        | LookupRow::FillOfShortIdOrder
        | LookupRow::FillOfAdoptedShortIdOrder => {
            // An order adopted from a venue report is created after the venue placed and filled it
            let created = if matches!(row, LookupRow::FillOfAdoptedShortIdOrder) {
                UnixNanos::from(
                    get_atomic_clock_realtime().get_time_ns().as_u64() + 86_400_000_000_000,
                )
            } else {
                UnixNanos::default()
            };
            let other = add_futures_limit_order_created_at(
                &cache,
                ClientOrderId::new(SHORTENED_CLIENT_ORDER_ID),
                test_instrument_id(),
                OrderSide::Buy,
                test_strategy_id(),
                created,
            );
            set_venue_order_id_on_cached_order(&cache, &other, "V-OTHER-1");

            match row {
                LookupRow::WindowOfShortIdOrder => {
                    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
                        "V-OTHER-1",
                        SHORTENED_CLIENT_ORDER_ID,
                        "PI_XBTUSD",
                        "buy",
                        "ENTERED_BOOK",
                        0,
                    )]));
                }
                LookupRow::FillOfShortIdOrder | LookupRow::FillOfAdoptedShortIdOrder => {
                    *state.fills_response.lock().await = Some(recent_fills_body(&[(
                        "F-OTHER-1",
                        "V-OTHER-1",
                        SHORTENED_CLIENT_ORDER_ID,
                    )]));
                }
                _ => {
                    let mut elements = vec![history_row("V-OTHER-1", SHORTENED_CLIENT_ORDER_ID)];
                    if matches!(row, LookupRow::HistoryOfShortIdOrderAndTheOrder) {
                        elements.push(history_row("V-OWN-1", LONG_CLIENT_ORDER_ID));
                    }
                    *state.futures_order_history_json.lock().await =
                        Some(futures_order_history_json(&elements));
                }
            }
        }
        LookupRow::HistoryOfUnindexedShortIdOrder(_)
        | LookupRow::WindowOfUnindexedShortIdOrder
        | LookupRow::FillOfUnindexedShortIdOrder => {
            let (instrument_id, side, created) = match row {
                LookupRow::HistoryOfUnindexedShortIdOrder(
                    UnindexedShortIdOrder::CreatedAfterTheRow,
                ) => (
                    test_instrument_id(),
                    OrderSide::Buy,
                    UnixNanos::from(
                        get_atomic_clock_realtime().get_time_ns().as_u64() + 86_400_000_000_000,
                    ),
                ),
                LookupRow::HistoryOfUnindexedShortIdOrder(
                    UnindexedShortIdOrder::OnTheOtherSide,
                ) => (test_instrument_id(), OrderSide::Sell, UnixNanos::default()),
                LookupRow::HistoryOfUnindexedShortIdOrder(
                    UnindexedShortIdOrder::OnAnotherInstrument,
                ) => (
                    InstrumentId::from("PF_ETHUSD.KRAKEN"),
                    OrderSide::Buy,
                    UnixNanos::default(),
                ),
                _ => (test_instrument_id(), OrderSide::Buy, UnixNanos::default()),
            };
            let other = add_futures_limit_order_created_at(
                &cache,
                ClientOrderId::new(SHORTENED_CLIENT_ORDER_ID),
                instrument_id,
                side,
                test_strategy_id(),
                created,
            );

            match row {
                LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Denied) => {
                    deny_cached_order(&cache, &other);
                }
                LookupRow::HistoryOfUnindexedShortIdOrder(
                    UnindexedShortIdOrder::HoldsAnotherVenueOrder,
                ) => {
                    mark_cached_order_submitted(&cache, &other);
                    set_venue_order_id_on_cached_order(&cache, &other, "V-OTHER-2");
                }
                LookupRow::HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder::Rejected) => {
                    mark_cached_order_rejected(&cache, &other);
                }
                _ => mark_cached_order_submitted(&cache, &other),
            }

            match row {
                LookupRow::WindowOfUnindexedShortIdOrder => {
                    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
                        "V-OTHER-1",
                        SHORTENED_CLIENT_ORDER_ID,
                        "PI_XBTUSD",
                        "buy",
                        "ENTERED_BOOK",
                        0,
                    )]));
                }
                LookupRow::FillOfUnindexedShortIdOrder => {
                    *state.fills_response.lock().await = Some(recent_fills_body(&[(
                        "F-OTHER-1",
                        "V-OTHER-1",
                        SHORTENED_CLIENT_ORDER_ID,
                    )]));
                }
                _ => {
                    *state.futures_order_history_json.lock().await =
                        Some(futures_order_history_json(&[history_row(
                            "V-OTHER-1",
                            SHORTENED_CLIENT_ORDER_ID,
                        )]));
                }
            }
        }
        LookupRow::HistoryIndexedForTheOrder => {
            cache
                .borrow_mut()
                .add_venue_order_id(&client_order_id, &VenueOrderId::from("V-OWN-1"), false)
                .unwrap();
            *state.futures_order_history_json.lock().await =
                Some(futures_order_history_json(&[history_row(
                    "V-OWN-1",
                    LONG_CLIENT_ORDER_ID,
                )]));
        }
        LookupRow::None => {}
    }

    // As the engine's targeted query does, the command carries the order's venue order ID
    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        Some(client_order_id),
        with_venue_order_id.then(|| VenueOrderId::from("V-OWN-1")),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    assert_lookup_result(expected, result, client_order_id);

    if let LookupRow::Window(_) = row {
        let body = state
            .orders_status_request_body
            .lock()
            .await
            .clone()
            .expect("orders-status request recorded");
        // The window is asked about the order under both forms the venue can hold it under
        assert!(
            body.contains(&format!("cliOrdIds={LONG_CLIENT_ORDER_ID}"))
                && body.contains(&format!("cliOrdIds={SHORTENED_CLIENT_ORDER_ID}")),
            "{body}"
        );
    }
}

/// Without a cached order to check a row's instrument, side and placement against, the echo alone
/// ties a row to the queried order unless the cache indexes the row's venue order for another
/// order or, indexing it for none, another cached order sent without a venue order ID holds the
/// echo as its full ID and fits the row by its instrument, side and creation, and with the venue
/// order ID known a row of another venue order that can be the order leaves the answer open, so the
/// query fails and reconciliation defers.
#[rstest]
#[case::echoing_row(
    false,
    Some(LONG_CLIENT_ORDER_ID),
    ShortIdOrder::Absent,
    Ok(Some("V-OTHER-1"))
)]
#[case::shortened_echoing_row(
    false,
    Some(SHORTENED_CLIENT_ORDER_ID),
    ShortIdOrder::Absent,
    Ok(Some("V-OTHER-1"))
)]
#[case::echoing_row_of_another_venue_order(
    true,
    Some(LONG_CLIENT_ORDER_ID),
    ShortIdOrder::Absent,
    Err("carries its client order ID")
)]
#[case::no_row(true, None, ShortIdOrder::Absent, Ok(None))]
#[case::row_of_a_short_id_order(
    false,
    Some(SHORTENED_CLIENT_ORDER_ID),
    ShortIdOrder::Indexed,
    Ok(None)
)]
#[case::row_of_a_short_id_order_with_venue_order_id(
    true,
    Some(SHORTENED_CLIENT_ORDER_ID),
    ShortIdOrder::Indexed,
    Ok(None)
)]
#[case::row_of_an_unindexed_short_id_order(
    false,
    Some(SHORTENED_CLIENT_ORDER_ID),
    ShortIdOrder::Unindexed,
    Err("the full ID of another cached order")
)]
#[tokio::test]
async fn test_futures_order_lookup_without_a_cached_order_takes_the_echo(
    #[case] with_venue_order_id: bool,
    #[case] echoed: Option<&str>,
    #[case] short_id_order: ShortIdOrder,
    #[case] expected: Result<Option<&str>, &str>,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new(LONG_CLIENT_ORDER_ID);

    if !matches!(short_id_order, ShortIdOrder::Absent) {
        let other = add_limit_order_to_cache(&cache, ClientOrderId::new(SHORTENED_CLIENT_ORDER_ID));

        if matches!(short_id_order, ShortIdOrder::Indexed) {
            set_venue_order_id_on_cached_order(&cache, &other, "V-OTHER-1");
        } else {
            mark_cached_order_submitted(&cache, &other);
        }
    }

    if let Some(echoed) = echoed {
        let row = futures_history_element(
            "OrderCancelled",
            "e-other",
            "V-OTHER-1",
            "PI_XBTUSD",
            "1000",
            "0",
            1_700_000_000_000,
        )
        .replace(r#""clientId":"""#, &format!(r#""clientId":"{echoed}""#));
        *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[row]));
    }

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        Some(client_order_id),
        with_venue_order_id.then(|| VenueOrderId::from("V-OWN-1")),
        None,
        None,
    );
    let result = client.generate_order_status_report(&cmd).await;

    assert_lookup_result(expected, result, client_order_id);
}

/// Asserts a lookup's result: the venue order ID of the report it takes, named by the queried
/// client order ID, no report, or an error whose message holds the expected text.
fn assert_lookup_result(
    expected: Result<Option<&str>, &str>,
    result: anyhow::Result<Option<OrderStatusReport>>,
    client_order_id: ClientOrderId,
) {
    match (expected, result) {
        (Ok(None), Ok(None)) => {}
        (Ok(Some(venue_order_id)), Ok(Some(report))) => {
            assert_eq!(report.venue_order_id, VenueOrderId::from(venue_order_id));
            assert_eq!(report.client_order_id, Some(client_order_id));
        }
        (Err(message), Err(e)) => {
            assert!(
                format!("{e:#}").contains(message),
                "unexpected error: {e:#}"
            );
        }
        (expected, result) => panic!("expected {expected:?}, received {result:?}"),
    }
}

/// A recent `/fills` body with one fill per `(fill_id, order_id, cli_ord_id)` of one contract.
fn recent_fills_body(fills: &[(&str, &str, &str)]) -> String {
    let fill_time = jiff::Timestamp::now().to_string();
    let rows: Vec<String> = fills
        .iter()
        .map(|(fill_id, order_id, echoed)| {
            format!(
                r#"{{"fill_id":"{fill_id}","symbol":"PI_XBTUSD","side":"buy","order_id":"{order_id}","cli_ord_id":"{echoed}","fillTime":"{fill_time}","size":1,"price":50000.0,"fillType":"taker","fee_paid":0.0,"fee_currency":"USD"}}"#
            )
        })
        .collect();
    format!(r#"{{"result":"success","fills":[{}]}}"#, rows.join(","))
}

/// Where a row carrying the queried client order ID appears in a lookup test.
#[derive(Clone, Copy, Debug)]
enum LookupRow {
    /// A history row of `V-OTHER-1` echoing this ID, beside the order's own row when it has a
    /// venue order ID.
    HistoryOfAnother(&'static str),
    /// Only a history row of `V-OTHER-1` echoing the full ID, the order's own row absent.
    OnlyAnotherWithTheId,
    /// History rows of `V-OTHER-1` and `V-OTHER-2`, echoing the full and the shortened ID.
    TwoHistoryRows,
    /// A window entry of `V-OTHER-1` echoing the full ID on another instrument.
    WindowOnAnotherInstrument,
    /// A history row of `V-OTHER-1` echoing the full ID on the other side.
    HistoryOfTheOtherSide,
    /// A window entry of `V-OTHER-1` echoing this ID.
    Window(&'static str),
    /// A recent fill of `V-OTHER-1` echoing this ID.
    Fill(&'static str),
    /// Window entries of `V-OTHER-1` and `V-OTHER-2`, echoing the full and the shortened ID.
    TwoWindowRows,
    /// Recent fills of `V-OTHER-1` and `V-OTHER-2`, each echoing the full ID.
    FillsOfTwoOrders,
    /// A window entry echoing the shortened ID of `V-OTHER-1`, which the cache indexes for another
    /// order whose ID shortens alike.
    WindowOfAnotherCachedOrder,
    /// A recent fill echoing the shortened ID of `V-OTHER-1`, which the cache indexes for another
    /// order whose ID shortens alike.
    FillOfAnotherCachedOrder,
    /// A history row of `V-OTHER-1` echoing the shortened ID, which the cache indexes for another
    /// order holding that form as its full ID.
    HistoryOfShortIdOrder,
    /// As `HistoryOfShortIdOrder`, beside a history row of `V-OWN-1` echoing the full ID.
    HistoryOfShortIdOrderAndTheOrder,
    /// A window entry of `V-OTHER-1` echoing the shortened ID, which the cache indexes for another
    /// order holding that form as its full ID.
    WindowOfShortIdOrder,
    /// A recent fill of `V-OTHER-1` echoing the shortened ID, which the cache indexes for another
    /// order holding that form as its full ID.
    FillOfShortIdOrder,
    /// As `FillOfShortIdOrder`, with the other order created a day after the fill.
    FillOfAdoptedShortIdOrder,
    /// A history row of `V-OWN-1` echoing the full ID, which the cache indexes for the order while
    /// the order carries no venue order ID, as when it is accepted while the query reads.
    HistoryIndexedForTheOrder,
    /// A history row of `V-OTHER-1`, which the cache indexes for no order, echoing the shortened ID
    /// another cached order holds as its full ID.
    HistoryOfUnindexedShortIdOrder(UnindexedShortIdOrder),
    /// A window entry of `V-OTHER-1`, as `HistoryOfUnindexedShortIdOrder` with an order that fits.
    WindowOfUnindexedShortIdOrder,
    /// A recent fill of `V-OTHER-1`, as `HistoryOfUnindexedShortIdOrder` with an order that fits.
    FillOfUnindexedShortIdOrder,
    /// No row carries the ID.
    None,
}

/// The state of the cached order holding the shortened ID as its full ID, beside an unindexed row
/// echoing it. Unless stated, it is submitted without a venue order ID, on the row's instrument and
/// side, and created before the row's placement, as while its acknowledgement is in flight.
#[derive(Clone, Copy, Debug)]
enum UnindexedShortIdOrder {
    /// Fits the row.
    Fits,
    /// Created a day after the test starts, long after the row's placement.
    CreatedAfterTheRow,
    /// On the other side.
    OnTheOtherSide,
    /// On another instrument.
    OnAnotherInstrument,
    /// Submitted and accepted as `V-OTHER-2`.
    HoldsAnotherVenueOrder,
    /// Denied, so never sent.
    Denied,
    /// Submitted and rejected, so closed without a venue order ID.
    Rejected,
}

/// The cached order, if any, holding the shortened ID as its full ID in an uncached lookup.
#[derive(Clone, Copy, Debug)]
enum ShortIdOrder {
    /// No such order is cached.
    Absent,
    /// Submitted, with no venue order ID.
    Unindexed,
    /// Cached, with `V-OTHER-1` indexed for it.
    Indexed,
}

/// The open-orders read lists the held order's 1,000 contracts as last updated then; a window copy
/// lists 5.
const OPEN_ORDER_UPDATED: &str = "2023-04-07T14:15:30.250Z";

/// A window copy of an order the open-orders read already returned names that report by the cached
/// order the window attributed it to and is dropped, so the order reaches reconciliation once with
/// the open-orders fields. A copy updated later lends the report its state and its rules: a fully
/// executed one is withheld, leaving mass status incomplete, and a canceled one with executions
/// passes; an older copy lends nothing more. A copy the window cannot attribute is withheld,
/// leaving the report unnamed and mass status incomplete.
#[rstest]
#[case::later_update(
    &[("V-HELD-030", "ENTERED_BOOK", 0)],
    "2026-09-12T04:05:06.300Z",
    Some((OrderStatus::Accepted, "5", Some(LONG_CLIENT_ORDER_ID))),
    true
)]
#[case::older_update(
    &[("V-HELD-030", "ENTERED_BOOK", 0)],
    "2023-04-07T14:15:30.000Z",
    Some((OrderStatus::Accepted, "1000", Some(LONG_CLIENT_ORDER_ID))),
    true
)]
#[case::later_cancel(
    &[("V-HELD-030", "CANCELLED", 0)],
    OPEN_ORDER_UPDATED,
    Some((OrderStatus::Canceled, "5", Some(LONG_CLIENT_ORDER_ID))),
    true
)]
#[case::later_cancel_with_executions(
    &[("V-HELD-030", "CANCELLED", 2)],
    OPEN_ORDER_UPDATED,
    Some((OrderStatus::Canceled, "5", Some(LONG_CLIENT_ORDER_ID))),
    true
)]
#[case::fully_executed(&[("V-HELD-030", "FULLY_EXECUTED", 5)], OPEN_ORDER_UPDATED, None, false)]
#[case::ambiguous_copy(
    &[("V-HELD-030", "ENTERED_BOOK", 0), ("V-HELD-031", "ENTERED_BOOK", 0)],
    "2026-09-12T04:05:06.300Z",
    Some((OrderStatus::Accepted, "1000", None)),
    false
)]
#[tokio::test]
async fn test_futures_window_copy_of_a_venue_read_order_names_it_once(
    #[case] window: &[(&str, &str, u32)],
    #[case] window_updated: &str,
    #[case] expected: Option<(OrderStatus, &str, Option<&str>)>,
    #[case] complete: bool,
    #[values(false, true)] mass_status: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let held = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
    mark_cached_order_pending_cancel_while_held(&cache, &held);
    // The venue rests the held order, whose acknowledgement the node missed
    *state.futures_open_orders_json.lock().await =
        Some(futures_open_orders_json("V-HELD-030", "PI_XBTUSD"));
    let entries: Vec<(&str, &str, &str, &str, &str, u32)> = window
        .iter()
        .map(|(venue_order_id, status, filled)| {
            (
                *venue_order_id,
                LONG_CLIENT_ORDER_ID,
                "PI_XBTUSD",
                "buy",
                *status,
                *filled,
            )
        })
        .collect();
    *state.orders_status_response.lock().await =
        Some(orders_status_body_updated_at(&entries, window_updated));

    let reports: Vec<OrderStatusReport> = if mass_status {
        let mass_status = client
            .generate_mass_status(None)
            .await
            .unwrap()
            .expect("mass status available");
        assert_eq!(mass_status.reports_complete(), complete);
        mass_status.order_reports().values().cloned().collect()
    } else {
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
        client.generate_order_status_reports(&cmd).await.unwrap()
    };

    let held_reports: Vec<(OrderStatus, Quantity, Option<ClientOrderId>)> = reports
        .iter()
        .filter(|report| report.venue_order_id == VenueOrderId::from("V-HELD-030"))
        .map(|report| (report.order_status, report.quantity, report.client_order_id))
        .collect();
    let expected: Vec<(OrderStatus, Quantity, Option<ClientOrderId>)> = expected
        .map(|(status, quantity, named)| {
            (
                status,
                Quantity::from(quantity),
                named.map(ClientOrderId::from),
            )
        })
        .into_iter()
        .collect();
    assert_eq!(held_reports, expected);
}

/// A closed history report keeps its state however the window copy reads, so a later fully
/// executed copy of an order the history already closed neither displaces the report nor leaves
/// mass status incomplete.
#[rstest]
#[tokio::test]
async fn test_futures_window_copy_leaves_a_closed_history_report_alone() {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let held = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
    mark_cached_order_pending_cancel_while_held(&cache, &held);
    *state.futures_order_history_json.lock().await =
        Some(futures_order_history_json(&[futures_history_element(
            "OrderCancelled",
            "e-closed",
            "V-HELD-030",
            "PI_XBTUSD",
            "1000",
            "0",
            1_680_876_930_250,
        )]));
    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
        "V-HELD-030",
        LONG_CLIENT_ORDER_ID,
        "PI_XBTUSD",
        "buy",
        "FULLY_EXECUTED",
        5,
    )]));

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status available");

    assert!(mass_status.reports_complete());
    let report = mass_status
        .order_reports()
        .get(&VenueOrderId::from("V-HELD-030"))
        .cloned()
        .expect("the history report stands");
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from(LONG_CLIENT_ORDER_ID))
    );
}

/// A report path of the futures client, for the naming test below.
#[derive(Clone, Copy, Debug)]
enum ReportPath {
    BulkHistory,
    MassStatusOrders,
    MassStatusFills,
    FillReports,
    Lookup,
    QueryOrder,
}

/// An order sent with a shortened client order ID is echoed in that form, so every report of an
/// order the cache indexes by venue order ID takes the cached client order ID, while a report of an
/// order the cache does not hold keeps the echo.
#[rstest]
#[case::bulk_history(ReportPath::BulkHistory)]
#[case::mass_status_orders(ReportPath::MassStatusOrders)]
#[case::mass_status_fills(ReportPath::MassStatusFills)]
#[case::fill_reports(ReportPath::FillReports)]
#[case::lookup(ReportPath::Lookup)]
#[case::query_order(ReportPath::QueryOrder)]
#[tokio::test]
async fn test_futures_reports_of_a_cached_order_carry_its_client_order_id(
    #[case] path: ReportPath,
) {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let client_order_id = ClientOrderId::new(LONG_CLIENT_ORDER_ID);
    let order = add_limit_order_to_cache(&cache, client_order_id);
    set_venue_order_id_on_cached_order(&cache, &order, "V-LEGACY-1");

    let history_row = |order_uid: &str| {
        futures_history_element(
            "OrderCancelled",
            &format!("e-{order_uid}"),
            order_uid,
            "PI_XBTUSD",
            "1000",
            "0",
            1_700_000_000_000,
        )
        .replace(
            r#""clientId":"""#,
            &format!(r#""clientId":"{SHORTENED_CLIENT_ORDER_ID}""#),
        )
    };
    *state.futures_order_history_json.lock().await = Some(futures_order_history_json(&[
        history_row("V-LEGACY-1"),
        history_row("V-UNKNOWN-1"),
    ]));
    let fill = |fill_id: &str, order_id: &str| {
        format!(
            r#"{{"fill_id":"{fill_id}","symbol":"PI_XBTUSD","side":"buy","order_id":"{order_id}","cli_ord_id":"{SHORTENED_CLIENT_ORDER_ID}","fillTime":"2023-04-07T15:20:45.500Z","size":1,"price":50000.0,"fillType":"taker","fee_paid":0.0,"fee_currency":"USD"}}"#
        )
    };
    *state.fills_response.lock().await = Some(format!(
        r#"{{"result":"success","fills":[{},{}]}}"#,
        fill("F-LEGACY-1", "V-LEGACY-1"),
        fill("F-UNKNOWN-1", "V-UNKNOWN-1"),
    ));

    let named: Vec<(VenueOrderId, Option<ClientOrderId>)> = match path {
        ReportPath::BulkHistory => client
            .generate_order_status_reports(&history_orders_cmd())
            .await
            .unwrap()
            .into_iter()
            .map(|report| (report.venue_order_id, report.client_order_id))
            .collect(),
        ReportPath::MassStatusOrders => client
            .generate_mass_status(None)
            .await
            .unwrap()
            .expect("mass status available")
            .order_reports()
            .values()
            .map(|report| (report.venue_order_id, report.client_order_id))
            .collect(),
        ReportPath::MassStatusFills => client
            .generate_mass_status(None)
            .await
            .unwrap()
            .expect("mass status available")
            .fill_reports()
            .values()
            .flatten()
            .map(|fill| (fill.venue_order_id, fill.client_order_id))
            .collect(),
        ReportPath::FillReports => client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|fill| (fill.venue_order_id, fill.client_order_id))
            .collect(),
        ReportPath::Lookup => {
            let cmd = GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(test_instrument_id()),
                Some(client_order_id),
                Some(VenueOrderId::from("V-LEGACY-1")),
                None,
                None,
            );
            let report = client
                .generate_order_status_report(&cmd)
                .await
                .unwrap()
                .expect("the order is reported");
            vec![(report.venue_order_id, report.client_order_id)]
        }
        ReportPath::QueryOrder => {
            *state.futures_open_orders_json.lock().await =
                Some(futures_open_orders_json("V-LEGACY-1", "PI_XBTUSD"));
            client
                .query_order(QueryOrder::new(
                    test_trader_id(),
                    None,
                    test_strategy_id(),
                    test_instrument_id(),
                    client_order_id,
                    Some(VenueOrderId::from("V-LEGACY-1")),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .unwrap();
            let event =
                recv_until(&mut rx, |event| matches!(event, ExecutionEvent::Report(_))).await;
            let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
                panic!("expected an order status report, received {event:?}");
            };
            vec![(report.venue_order_id, report.client_order_id)]
        }
    };

    let name_of = |venue_order_id: &str| {
        named
            .iter()
            .filter(|(id, _)| *id == VenueOrderId::from(venue_order_id))
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
    };
    assert!(
        name_of("V-LEGACY-1")
            .iter()
            .all(|name| *name == Some(client_order_id)),
        "{named:?}"
    );
    assert!(!name_of("V-LEGACY-1").is_empty(), "{named:?}");

    // Mass status withholds the uncached control on its own rules, so the bulk paths carry it
    if matches!(path, ReportPath::BulkHistory | ReportPath::FillReports) {
        assert_eq!(
            name_of("V-UNKNOWN-1"),
            vec![Some(ClientOrderId::from(SHORTENED_CLIENT_ORDER_ID))],
            "an order the cache does not hold keeps the echo"
        );
    }
}

/// The client order ID request returns every order holding the ID, so a window report of another
/// instrument, here an order whose ID shortens alike, stays out of a run scoped to one instrument
/// and is kept in an unscoped one.
#[rstest]
#[case::scoped_to_the_held_order(Some("PI_XBTUSD.KRAKEN"), false)]
#[case::unscoped(None, true)]
#[tokio::test]
async fn test_futures_scoped_window_run_keeps_only_its_instrument(
    #[case] instrument_id: Option<&str>,
    #[case] reported: bool,
) {
    let (client, _rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;
    let held = add_limit_order_to_cache(&cache, ClientOrderId::new(LONG_CLIENT_ORDER_ID));
    mark_cached_order_pending_cancel_while_held(&cache, &held);
    let other = add_futures_limit_order_to_cache(
        &cache,
        ClientOrderId::new(LONG_CLIENT_ORDER_ID_OTHER_DAY),
        InstrumentId::from("PF_ETHUSD.KRAKEN"),
        OrderSide::Buy,
        test_strategy_id(),
    );
    set_venue_order_id_on_cached_order(&cache, &other, "V-ETH-1");
    *state.orders_status_response.lock().await = Some(orders_status_body(&[(
        "V-ETH-1",
        SHORTENED_CLIENT_ORDER_ID,
        "PF_ETHUSD",
        "buy",
        "ENTERED_BOOK",
        0,
    )]));

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        instrument_id.map(InstrumentId::from),
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert_eq!(
        reports
            .iter()
            .any(|report| report.venue_order_id == VenueOrderId::from("V-ETH-1")),
        reported,
        "{reports:?}"
    );
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

/// A cancel of an order without a venue order ID is sent, and rejected, by its full client order
/// ID, which the venue echoes on the rejected item, so the rejection reaches that order.
#[rstest]
#[tokio::test]
async fn test_batch_cancel_rejection_by_client_order_id_reaches_its_order() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::RejectedByClientOrderId,
            ..Default::default()
        })
        .await;
    let client_order_id = ClientOrderId::new(LONG_CLIENT_ORDER_ID);
    add_limit_order_to_cache(&cache, client_order_id);
    let mut cancel = cancel_order_command(client_order_id, VenueOrderId::from("V-UNUSED"));
    cancel.venue_order_id = None;

    client
        .batch_cancel_orders(batch_cancel_command(vec![cancel]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

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
            assert!(event.reason.contains("notFound"), "{event:?}");
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
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
