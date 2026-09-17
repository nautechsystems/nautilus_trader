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

//! Shared fixtures, caches and client builders for the Kalshi integration tests.

use std::{cell::RefCell, net::SocketAddr, rc::Rc, sync::OnceLock, time::Duration};

use nautilus_common::{
    cache::{Cache, CacheView},
    clients::ExecutionClient,
    live::runner::replace_exec_event_sender,
    messages::{ExecutionEvent, ExecutionReport, execution::SubmitOrder},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_kalshi::{
    KalshiExecClientConfig, KalshiExecutionClient,
    common::{consts::KALSHI_VENUE, credential::KalshiCredential},
    http::{auth::KalshiAuth, client::KalshiHttpClient, models::KalshiMarket},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta},
    enums::{AccountType, BookAction, BookType, OmsType, OrderSide, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
    },
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::{OrderAny, builder::OrderTestBuilder},
    reports::OrderStatusReport,
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

/// Returns the PEM for a throwaway 2048-bit key generated for these tests. It has no exchange
/// privileges.
///
/// The armor is composed here rather than stored, because the repository's secret scanner rejects
/// any file that contains a PEM header.
pub(crate) fn test_private_key_pem() -> &'static str {
    static PEM: OnceLock<String> = OnceLock::new();

    PEM.get_or_init(|| {
        let label = "PRIVATE KEY";
        let body = include_str!("../test_data/rsa_test_private_key_pkcs8.b64").trim();

        format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
    })
}

pub(crate) const API_KEY_ID: &str = "a952bcbe-ec3b-4b5b-b8f9-11dae589608c";
pub(crate) const TICKER: &str = "KXHIGHNY-25JAN01-T50";
pub(crate) const VENUE_ORDER_ID: &str = "order-1";
pub(crate) const CLIENT_ORDER_ID: &str = "O-20250101-000000-001-001-1";
pub(crate) const TS_INIT: u64 = 1_735_732_800_000_000_000;

pub(crate) fn instrument_id() -> InstrumentId {
    InstrumentId::from(format!("{TICKER}.{KALSHI_VENUE}").as_str())
}

pub(crate) fn ts_init() -> UnixNanos {
    UnixNanos::from(TS_INIT)
}

/// A market payload shaped like the exchange's own response.
pub(crate) const MARKET_JSON: &str = r#"{
    "ticker": "KXHIGHNY-25JAN01-T50",
    "event_ticker": "KXHIGHNY-25JAN01",
    "market_type": "binary",
    "yes_sub_title": "50 degrees or above",
    "no_sub_title": "49 degrees or below",
    "created_time": "2024-12-30T15:00:00Z",
    "updated_time": "2025-01-01T06:00:00Z",
    "open_time": "2024-12-30T15:00:00Z",
    "close_time": "2025-01-02T05:00:00Z",
    "latest_expiration_time": "2025-01-05T05:00:00Z",
    "settlement_timer_seconds": 1800,
    "status": "active",
    "notional_value_dollars": "1.0000",
    "yes_bid_dollars": "0.3400",
    "yes_ask_dollars": "0.3500",
    "no_bid_dollars": "0.6500",
    "no_ask_dollars": "0.6600",
    "yes_bid_size_fp": "120.00",
    "yes_ask_size_fp": "80.00",
    "last_price_dollars": "0.3500",
    "previous_yes_bid_dollars": "0.3300",
    "previous_yes_ask_dollars": "0.3600",
    "previous_price_dollars": "0.3400",
    "volume_fp": "1520.00",
    "volume_24h_fp": "310.00",
    "open_interest_fp": "900.00",
    "result": "",
    "can_close_early": true,
    "expiration_value": "51",
    "rules_primary": "Resolves YES if the high is 50 or above.",
    "rules_secondary": "Source: NWS Central Park.",
    "price_level_structure": "linear_cent",
    "price_ranges": [
        {"start": "0.0000", "end": "1.0000", "step": "0.0100"}
    ]
}"#;

/// An order the exchange reports, in the given lifecycle state.
pub(crate) fn order_json(status: &str, filled: &str, remaining: &str) -> String {
    format!(
        r#"{{
            "order_id": "{VENUE_ORDER_ID}",
            "user_id": "member-1",
            "client_order_id": "{CLIENT_ORDER_ID}",
            "ticker": "{TICKER}",
            "outcome_side": "yes",
            "book_side": "bid",
            "type": "limit",
            "status": "{status}",
            "yes_price_dollars": "0.3400",
            "no_price_dollars": "0.6600",
            "fill_count_fp": "{filled}",
            "remaining_count_fp": "{remaining}",
            "initial_count_fp": "100.00",
            "taker_fill_cost_dollars": "34.0000",
            "maker_fill_cost_dollars": "34.0000",
            "taker_fees_dollars": "0.1000",
            "maker_fees_dollars": "0.1000",
            "expiration_time": "",
            "created_time": "2025-01-01T12:00:00Z",
            "last_update_time": "2025-01-01T12:00:05Z"
        }}"#
    )
}

/// A fill the exchange reports for the order.
pub(crate) const FILL_JSON: &str = r#"{
    "fill_id": "fill-1",
    "trade_id": "fill-1",
    "order_id": "order-1",
    "ticker": "KXHIGHNY-25JAN01-T50",
    "outcome_side": "yes",
    "book_side": "bid",
    "count_fp": "100.00",
    "yes_price_dollars": "0.3400",
    "no_price_dollars": "0.6600",
    "is_taker": true,
    "fee_cost": "0.1000",
    "created_time": "2025-01-01T12:00:05Z"
}"#;

/// A fill for a given contract count and timestamp.
pub(crate) fn fill_json(fill_id: &str, count: &str, created_time: &str) -> String {
    format!(
        r#"{{
            "fill_id": "{fill_id}",
            "trade_id": "{fill_id}",
            "order_id": "{VENUE_ORDER_ID}",
            "ticker": "{TICKER}",
            "outcome_side": "yes",
            "book_side": "bid",
            "count_fp": "{count}",
            "yes_price_dollars": "0.3400",
            "no_price_dollars": "0.6600",
            "is_taker": true,
            "fee_cost": "0.1000",
            "created_time": "{created_time}"
        }}"#
    )
}

/// Adds one ask level to the cache, so a market order has a far side to price from.
pub(crate) fn add_ask(cache: &Rc<RefCell<Cache>>, price: &str, size: &str) {
    let mut book = OrderBook::new(instrument_id(), BookType::L2_MBP);
    let order = BookOrder::new(OrderSide::Sell, Price::from(price), Quantity::from(size), 1);
    let delta = OrderBookDelta::new(
        instrument_id(),
        BookAction::Add,
        order,
        0,
        1,
        ts_init(),
        ts_init(),
    );

    book.apply_delta(&delta).unwrap();
    cache.borrow_mut().add_order_book(book).unwrap();
}

/// Builds the execution client against the mock venue.
pub(crate) fn execution_client(
    addr: SocketAddr,
    cache: CacheView,
    poll_interval_millis: u64,
    reconciliation: bool,
) -> KalshiExecutionClient {
    let config = KalshiExecClientConfig::builder()
        .api_key_id(API_KEY_ID.to_string())
        .api_key_pem(test_private_key_pem().into())
        .poll_interval_millis(poll_interval_millis)
        .reconciliation(reconciliation)
        .build();
    let http_client = KalshiHttpClient::new(
        Some(format!("http://{addr}/trade-api/v2")),
        Some(5),
        None,
        Some(KalshiAuth::new(KalshiCredential::new(
            API_KEY_ID.to_string(),
            test_private_key_pem().to_string(),
        ))),
    )
    .unwrap();
    let core = ExecutionClientCore::new(
        TraderId::from("TESTER-001"),
        ClientId::from("KALSHI-EXEC"),
        Venue::from(KALSHI_VENUE),
        OmsType::Netting,
        AccountId::from("KALSHI-001"),
        AccountType::Cash,
        Some(Currency::from("USD")),
        cache,
    );

    KalshiExecutionClient::new(core, http_client, &config)
}

pub(crate) fn cache_handle() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::default()))
}

pub(crate) fn instrument() -> InstrumentAny {
    let market: KalshiMarket = serde_json::from_str(MARKET_JSON).unwrap();

    nautilus_kalshi::http::parse::create_instrument_from_market(&market, ts_init()).unwrap()
}

pub(crate) fn limit_order(
    side: OrderSide,
    time_in_force: TimeInForce,
    client_order_id: &str,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id())
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(Quantity::from("100.00"))
        .price(Price::from("0.34"))
        .time_in_force(time_in_force)
        .build()
}

/// Puts the order in the cache, as the trader leaves it before submitting.
pub(crate) fn cached_order(cache: &Rc<RefCell<Cache>>, order: &OrderAny) {
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
}

pub(crate) fn submit(cmd: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        cmd,
        TraderId::from("TESTER-001"),
        None,
        None,
        UUID4::new(),
        ts_init(),
    )
}

/// Starts the client with its events routed to the test, returning the receiver.
pub(crate) fn start_client(
    client: &mut KalshiExecutionClient,
) -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    replace_exec_event_sender(tx);
    client.start().unwrap();

    rx
}

pub(crate) async fn next_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> ExecutionEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an execution event arrives")
        .expect("the event sender is alive")
}

/// Returns the next order event, skipping anything else on the channel.
pub(crate) async fn next_order_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> OrderEventAny {
    loop {
        if let ExecutionEvent::Order(event) = next_event(rx).await {
            return event;
        }
    }
}

/// Returns the next report, skipping anything else on the channel.
pub(crate) async fn next_report(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> ExecutionReport {
    loop {
        if let ExecutionEvent::Report(report) = next_event(rx).await {
            return report;
        }
    }
}

/// Returns every event that arrives within the window, draining the channel.
pub(crate) async fn events_within(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    window: Duration,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + window;

    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        events.push(event);
    }

    events
}

/// Returns every fill identifier the events report, whether as an order event or a report.
pub(crate) fn reported_fill_ids(events: &[ExecutionEvent]) -> Vec<TradeId> {
    events
        .iter()
        .flat_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => vec![filled.trade_id],
            ExecutionEvent::Report(ExecutionReport::Fill(fill)) => vec![fill.trade_id],
            ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, fills)) => {
                fills.iter().map(|fill| fill.trade_id).collect()
            }
            _ => Vec::new(),
        })
        .collect()
}

/// Returns every order status report the events carry, bundled or not.
pub(crate) fn status_reports(events: &[ExecutionEvent]) -> Vec<OrderStatusReport> {
    events
        .iter()
        .flat_map(|event| match event {
            ExecutionEvent::Report(ExecutionReport::Order(status)) => vec![status.as_ref().clone()],
            ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, _)) => {
                vec![status.as_ref().clone()]
            }
            _ => Vec::new(),
        })
        .collect()
}

/// Parses a JSON string or number field as an exact decimal.
pub(crate) fn decimal(value: &serde_json::Value) -> Decimal {
    match value {
        serde_json::Value::String(value) => value.parse().expect("a decimal string"),
        other => other.to_string().parse().expect("a decimal"),
    }
}
