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

//! Data client tests against a mock of the Kalshi market data API.
//!
//! Every test drives the client's own poll, which is the work its poll task performs on an interval,
//! so what a running data client publishes is what these tests assert: instrument discovery, quotes,
//! book deltas, trades, and the close and resolution of a settled market.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades},
    },
};
use nautilus_core::UUID4;
use nautilus_kalshi::{
    KalshiDataClient,
    common::consts::{KALSHI_DATA_CLIENT_ID, KALSHI_VENUE},
    http::client::KalshiHttpClient,
    providers::KalshiInstrumentProvider,
};
use nautilus_model::{
    data::Data as NautilusData,
    enums::{BookType, InstrumentCloseType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId},
    instruments::Instrument,
    prediction::ResolutionOutcome,
    types::Price,
};
use parking_lot::Mutex;

const TICKER: &str = "KXHIGHNY-25JAN01-T50";
const COUNTERPART_TICKER: &str = "KXHIGHNY-25JAN01-T60";
const EVENT_TICKER: &str = "KXHIGHNY-25JAN01";
const TS_INIT: u64 = 1_735_732_800_000_000_000;

fn instrument_id(ticker: &str) -> InstrumentId {
    InstrumentId::from(format!("{ticker}.{KALSHI_VENUE}").as_str())
}

/// A market payload shaped like the exchange's own response.
///
/// The status, result, and settlement fields are the ones that change over a market's life, so they
/// are the parameters.
fn market_json(ticker: &str, status: &str, result: &str, settlement: &str) -> String {
    format!(
        r#"{{
            "ticker": "{ticker}",
            "event_ticker": "{EVENT_TICKER}",
            "market_type": "binary",
            "yes_sub_title": "{ticker} yes",
            "no_sub_title": "{ticker} no",
            "created_time": "2024-12-30T15:00:00Z",
            "updated_time": "2025-01-01T06:00:00Z",
            "open_time": "2024-12-30T15:00:00Z",
            "close_time": "2025-01-02T05:00:00Z",
            "latest_expiration_time": "2025-01-05T05:00:00Z",
            "settlement_timer_seconds": 1800,
            "status": "{status}",
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
            "result": "{result}",
            "can_close_early": true,
            "expiration_value": "51",
            "rules_primary": "Resolves YES if the high is 50 or above.",
            "rules_secondary": "Source: NWS Central Park.",
            "price_level_structure": "linear_cent",
            "price_ranges": [
                {{"start": "0.0000", "end": "1.0000", "step": "0.0100"}}
            ]{settlement}
        }}"#
    )
}

/// The settled fields of a market that has taken effect, or nothing while it is live.
fn settled_fields(result: &str) -> String {
    let value = if result == "yes" { "1.0000" } else { "0.0000" };

    format!(r#", "settlement_value_dollars": "{value}", "settlement_ts": "2025-01-02T06:00:00Z""#)
}

/// An event payload whose markets are nested, as the venue returns them when asked for them.
fn event_json(event_ticker: &str, markets: &[String]) -> String {
    format!(
        r#"{{
            "event": {{
                "event_ticker": "{event_ticker}",
                "series_ticker": "KXHIGHNY",
                "sub_title": "High temp",
                "title": "Highest temperature in NYC",
                "collateral_return_type": "binary",
                "mutually_exclusive": true,
                "markets": [{}]
            }},
            "markets": [{}]
        }}"#,
        markets.join(", "),
        markets.join(", ")
    )
}

/// A trade payload for the given market.
fn trade_json(ticker: &str) -> String {
    format!(
        r#"{{
            "trade_id": "trade-1",
            "ticker": "{ticker}",
            "count_fp": "25.00",
            "yes_price_dollars": "0.3400",
            "no_price_dollars": "0.6600",
            "taker_outcome_side": "yes",
            "taker_book_side": "bid",
            "is_block_trade": false,
            "created_time": "2025-01-01T12:00:05Z"
        }}"#
    )
}

/// What the mock exchange holds, and how a test moves it through a market's life.
#[derive(Clone, Debug)]
struct MockExchange {
    market: Arc<Mutex<String>>,
    counterpart: Arc<Mutex<String>>,
}

impl MockExchange {
    /// A live market with a live counterpart in the same event.
    fn live() -> Self {
        Self {
            market: Arc::new(Mutex::new(market_json(TICKER, "active", "", ""))),
            counterpart: Arc::new(Mutex::new(market_json(
                COUNTERPART_TICKER,
                "active",
                "",
                "",
            ))),
        }
    }

    /// Teaches the exchange that the event resolved: one leg pays and the other does not.
    fn settle(&self, winner: &str) {
        let (first, second) = if winner == TICKER {
            (TICKER, COUNTERPART_TICKER)
        } else {
            (COUNTERPART_TICKER, TICKER)
        };

        *self.market.lock() = if winner == TICKER {
            market_json(first, "finalized", "yes", &settled_fields("yes"))
        } else {
            market_json(first, "finalized", "no", &settled_fields("no"))
        };
        *self.counterpart.lock() = if winner == TICKER {
            market_json(second, "finalized", "no", &settled_fields("no"))
        } else {
            market_json(second, "finalized", "yes", &settled_fields("yes"))
        };
    }

    /// The markets the exchange lists, in the order it lists them.
    fn markets(&self) -> Vec<String> {
        vec![self.market.lock().clone(), self.counterpart.lock().clone()]
    }
}

async fn markets(State(exchange): State<MockExchange>) -> Response {
    (
        StatusCode::OK,
        format!(
            r#"{{"markets": [{}], "cursor": ""}}"#,
            exchange.markets().join(", ")
        ),
    )
        .into_response()
}

async fn market(Path(ticker): Path<String>, State(exchange): State<MockExchange>) -> Response {
    let payload = match ticker.as_str() {
        TICKER => exchange.market.lock().clone(),
        COUNTERPART_TICKER => exchange.counterpart.lock().clone(),
        _ => return (StatusCode::NOT_FOUND, r#"{"code":"not_found"}"#).into_response(),
    };

    (StatusCode::OK, format!(r#"{{"market": {payload}}}"#)).into_response()
}

async fn orderbook(Path(_ticker): Path<String>) -> Response {
    (
        StatusCode::OK,
        r#"{
            "orderbook_fp": {
                "yes_dollars": [["0.3400", "120.00"], ["0.3300", "50.00"]],
                "no_dollars": [["0.6500", "80.00"]]
            }
        }"#,
    )
        .into_response()
}

async fn trades(Query(params): Query<HashMap<String, String>>) -> Response {
    let ticker = params.get("ticker").map_or(TICKER, String::as_str);

    (
        StatusCode::OK,
        format!(r#"{{"trades": [{}], "cursor": ""}}"#, trade_json(ticker)),
    )
        .into_response()
}

async fn event(Path(event_ticker): Path<String>, State(exchange): State<MockExchange>) -> Response {
    (
        StatusCode::OK,
        event_json(&event_ticker, &exchange.markets()),
    )
        .into_response()
}

/// Starts the mock exchange and returns its address.
async fn spawn_mock(exchange: MockExchange) -> SocketAddr {
    let router = Router::new()
        .route("/trade-api/v2/markets", get(markets))
        .route("/trade-api/v2/markets/trades", get(trades))
        .route("/trade-api/v2/markets/{ticker}", get(market))
        .route("/trade-api/v2/markets/{ticker}/orderbook", get(orderbook))
        .route("/trade-api/v2/events/{event_ticker}", get(event))
        .with_state(exchange);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    addr
}

/// Builds the data client against the mock exchange, with a poll interval no test waits on.
fn data_client(addr: SocketAddr) -> KalshiDataClient {
    let base_url = Some(format!("http://{addr}/trade-api/v2"));
    let http_client = KalshiHttpClient::new(base_url.clone(), Some(5), None, None).unwrap();
    let provider = KalshiInstrumentProvider::new(
        KalshiHttpClient::new(base_url, Some(5), None, None).unwrap(),
        Vec::new(),
        None,
    );

    KalshiDataClient::new(
        ClientId::from(KALSHI_DATA_CLIENT_ID),
        http_client,
        provider,
        Some(Duration::from_secs(3_600)),
    )
}

/// Starts a client against the mock exchange, capturing the data events it publishes.
///
/// The client takes its event sender when it is constructed, so the sender is installed first.
async fn started_client(
    addr: SocketAddr,
) -> (
    KalshiDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    replace_data_event_sender(tx);
    let mut client = data_client(addr);
    client.connect().await.unwrap();

    (client, rx)
}

/// Returns every event that arrives within the window, draining the channel.
async fn events_within(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    window: Duration,
) -> Vec<DataEvent> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + window;

    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        events.push(event);
    }

    events
}

fn subscribe_quotes(client: &mut KalshiDataClient, instrument_id: InstrumentId) {
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(ClientId::from(KALSHI_DATA_CLIENT_ID)),
            None,
            UUID4::new(),
            TS_INIT.into(),
            None,
            None,
        ))
        .unwrap();
}

fn subscribe_book(client: &mut KalshiDataClient, instrument_id: InstrumentId) {
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(ClientId::from(KALSHI_DATA_CLIENT_ID)),
            None,
            UUID4::new(),
            TS_INIT.into(),
            None,
            true,
            None,
            None,
        ))
        .unwrap();
}

fn subscribe_trades(client: &mut KalshiDataClient, instrument_id: InstrumentId) {
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(ClientId::from(KALSHI_DATA_CLIENT_ID)),
            None,
            UUID4::new(),
            TS_INIT.into(),
            None,
            None,
        ))
        .unwrap();
}

#[tokio::test]
async fn test_start_publishes_the_instruments_the_exchange_lists() {
    let exchange = MockExchange::live();
    let addr = spawn_mock(exchange).await;
    let (mut client, mut rx) = started_client(addr).await;

    // The exchange's open markets become the instruments the engine can trade, and the client holds
    // no subscription until the engine asks for one.
    let events = events_within(&mut rx, Duration::from_millis(200)).await;
    let published: Vec<InstrumentId> = events
        .iter()
        .filter_map(|event| match event {
            DataEvent::Instrument(instrument) => Some(instrument.id()),
            _ => None,
        })
        .collect();

    assert_eq!(
        published,
        vec![instrument_id(TICKER), instrument_id(COUNTERPART_TICKER)]
    );
    assert_eq!(client.subscribed_instruments(), 0);

    client.stop().unwrap();
}

#[tokio::test]
async fn test_poll_publishes_quotes_book_deltas_and_trades() {
    let exchange = MockExchange::live();
    let addr = spawn_mock(exchange).await;
    let (mut client, mut rx) = started_client(addr).await;
    let id = instrument_id(TICKER);

    subscribe_quotes(&mut client, id);
    subscribe_book(&mut client, id);
    subscribe_trades(&mut client, id);

    assert_eq!(client.subscribed_instruments(), 1);
    let _ = events_within(&mut rx, Duration::from_millis(150)).await;

    assert_eq!(client.poll_markets().await.unwrap(), 1);
    assert_eq!(client.poll_trades().await.unwrap(), 1);

    let events = events_within(&mut rx, Duration::from_millis(200)).await;
    let quote = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Data(NautilusData::Quote(quote)) => Some(quote),
            _ => None,
        })
        .expect("the subscribed market publishes a quote");

    assert_eq!(quote.instrument_id, id);
    assert_eq!(quote.bid_price, Price::from("0.34"));
    assert_eq!(quote.ask_price, Price::from("0.35"));
    let deltas = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Data(NautilusData::BookDeltas(deltas)) => Some(deltas),
            _ => None,
        })
        .expect("the subscribed market publishes book deltas");

    assert!(!deltas.deltas.is_empty());
    let trade = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Data(NautilusData::Trade(trade)) => Some(trade),
            _ => None,
        })
        .expect("the subscribed market publishes its trades");

    assert_eq!(trade.trade_id.to_string(), "trade-1");
    assert_eq!(trade.price, Price::from("0.34"));

    client.stop().unwrap();
}

#[tokio::test]
async fn test_settlement_publishes_the_close_and_the_resolution_once() {
    let exchange = MockExchange::live();
    let addr = spawn_mock(exchange.clone()).await;
    let (mut client, mut rx) = started_client(addr).await;
    let id = instrument_id(TICKER);

    subscribe_quotes(&mut client, id);
    assert_eq!(client.poll_markets().await.unwrap(), 1);
    let _ = events_within(&mut rx, Duration::from_millis(150)).await;

    // The event resolves: the leg that held the high temperature pays a dollar and the other pays
    // nothing.
    exchange.settle(TICKER);
    assert_eq!(client.poll_markets().await.unwrap(), 1);

    let events = events_within(&mut rx, Duration::from_millis(200)).await;
    let status = events
        .iter()
        .find_map(|event| match event {
            DataEvent::InstrumentStatus(status) => Some(status),
            _ => None,
        })
        .expect("a settled market publishes its status");

    assert_eq!(status.instrument_id, id);
    assert_eq!(status.action, MarketStatusAction::Close);
    let close = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Data(NautilusData::InstrumentClose(close)) => Some(close),
            _ => None,
        })
        .expect("a settled market publishes its close");

    assert_eq!(close.instrument_id, id);
    assert_eq!(close.close_price, Price::from("1.00"));
    assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
    let resolution = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Data(NautilusData::MarketResolution(resolution)) => Some(resolution),
            _ => None,
        })
        .expect("a settled event publishes its resolution");

    assert_eq!(
        resolution.group_id.to_string(),
        format!("{EVENT_TICKER}.{KALSHI_VENUE}")
    );
    assert_eq!(resolution.version, 1);
    assert_eq!(resolution.outcome.state(), "payouts");
    let ResolutionOutcome::Payouts(payouts) = &resolution.outcome else {
        panic!("a settled market pays out");
    };
    let paid: Vec<(String, String)> = payouts
        .iter()
        .map(|payout| {
            (
                payout.outcome_id.to_string(),
                payout.payout_per_unit.to_string(),
            )
        })
        .collect();

    assert_eq!(
        paid,
        vec![
            (format!("{TICKER} yes"), "1.00 USD".to_string()),
            (format!("{COUNTERPART_TICKER} yes"), "0.00 USD".to_string()),
        ]
    );

    // A settlement is published once: a second poll of the same version repeats nothing, so a
    // consumer that applies the outcome does not apply it twice.
    assert_eq!(client.poll_markets().await.unwrap(), 1);
    let repeated = events_within(&mut rx, Duration::from_millis(200)).await;

    assert!(
        !repeated.iter().any(|event| matches!(
            event,
            DataEvent::Data(NautilusData::InstrumentClose(_) | NautilusData::MarketResolution(_))
        )),
        "a settled market must not be closed or resolved twice: {repeated:?}"
    );

    client.stop().unwrap();
}
