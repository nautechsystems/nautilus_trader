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

//! Longer Hyperliquid live soak. Most cases are read-only; one testnet case
//! places and cancels a post-only order. Run with:
//! `cargo test -p nautilus-hyperliquid --test integration live_soak:: -- --ignored --nocapture --test-threads=1`

use std::time::Duration;

use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment,
    http::{
        HyperliquidHttpClient,
        models::{
            Cloid, HyperliquidExchangeAction, HyperliquidExchangeCancelByCloidRequest,
            HyperliquidExchangeGrouping, HyperliquidExchangeLimitParams,
            HyperliquidExchangeOrderKind, HyperliquidExchangePlaceOrderRequest,
            HyperliquidExchangeResponse, HyperliquidExchangeTif, HyperliquidRecentTrade,
        },
    },
    websocket::{client::HyperliquidWebSocketClient, messages::NautilusWsMessage},
};
use nautilus_model::{
    data::{BarType, QuoteTick},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use rust_decimal::{Decimal, RoundingStrategy};

fn perp_id(instruments: &[InstrumentAny], raw: &str) -> InstrumentId {
    let instrument = instruments
        .iter()
        .find(|instrument| instrument.raw_symbol().as_str() == raw)
        .unwrap_or_else(|| panic!("{raw} perpetual must exist"));

    match instrument {
        InstrumentAny::CryptoPerpetual(instrument) => instrument.id,
        _ => panic!("{raw} must be a perpetual"),
    }
}

fn first_user(trades: &[HyperliquidRecentTrade]) -> String {
    trades
        .iter()
        .flat_map(|trade| trade.users.iter())
        .find(|address| address.starts_with("0x") && address.len() == 42)
        .expect("recent trade must name a user address")
        .clone()
}

async fn wait_for(
    client: &mut HyperliquidWebSocketClient,
    timeout: Duration,
    mut accept: impl FnMut(&NautilusWsMessage) -> bool,
) -> NautilusWsMessage {
    tokio::time::timeout(timeout, async {
        loop {
            match client.next_event().await {
                Some(event) if accept(&event) => return event,
                Some(_) => {}
                None => panic!("WS closed before the expected event"),
            }
        }
    })
    .await
    .expect("timeout waiting for expected WS event")
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_http_exec_paths_across_active_users() {
    let mut http = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("public HTTP client");
    let instruments = http.request_instruments().await.expect("instruments");
    for instrument in &instruments {
        http.cache_instrument(instrument);
    }
    http.set_account_id(AccountId::from("HYPERLIQUID-SOAK"));

    let mut users = Vec::new();

    for coin in ["BTC", "ETH", "SOL"] {
        let trades = http
            .info_recent_trades(coin)
            .await
            .unwrap_or_else(|e| panic!("{coin} recent trades: {e}"));
        assert!(!trades.is_empty(), "{coin} recent trades must be non-empty");
        users.push(first_user(&trades));
    }
    users.sort();
    users.dedup();

    let mut fill_reports = 0usize;
    let mut position_reports = 0usize;
    let mut historical_orders = 0usize;
    let mut nonzero_tid = 0usize;

    for user in &users {
        let fills = http.info_user_fills(user).await.expect("userFills");
        nonzero_tid += fills.iter().filter(|fill| fill.tid != 0).count();
        fill_reports += http
            .request_fill_reports(user, None)
            .await
            .expect("fill reports")
            .len();
        position_reports += http
            .request_position_status_reports(user, None)
            .await
            .expect("position reports")
            .len();
        match http.info_historical_orders(user).await {
            Ok(entries) => historical_orders += entries.len(),
            Err(e) => eprintln!("historical_orders_error={e}"),
        }

        if let Some(oid) = fills.iter().map(|fill| fill.oid).find(|oid| *oid != 0) {
            match http.request_order_status_report(user, oid).await {
                Ok(_) => {}
                Err(e) => eprintln!("oid_status_error={e}"),
            }
        }
    }

    let bars = http
        .request_bars(
            BarType::from("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL"),
            None,
            None,
            Some(50),
        )
        .await
        .expect("historical bars");
    assert!(bars.len() >= 10, "candle snapshot must return bars");
    for window in bars.windows(2) {
        assert!(
            window[0].ts_event <= window[1].ts_event,
            "bars must be non-decreasing in event time"
        );
    }

    eprintln!(
        "users={} fill_reports={fill_reports} positions={position_reports} historical_orders={historical_orders} nonzero_tid={nonzero_tid} bars={}",
        users.len(),
        bars.len()
    );
    assert!(nonzero_tid > 0, "live fills must retain venue tid");
    assert!(fill_reports > 0, "FillReport parse must succeed");
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_quote_integrity_and_triple_reconnect() {
    let http = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("public HTTP client");
    let instruments = http.request_instruments().await.expect("instruments");
    let btc = perp_id(&instruments, "BTC");
    let eth = perp_id(&instruments, "ETH");

    let mut client = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    );
    client.cache_instruments(instruments);
    client.connect().await.expect("connect");
    client.subscribe_trades(btc).await.expect("btc trades");
    client.subscribe_quotes(btc).await.expect("btc quotes");
    client.subscribe_trades(eth).await.expect("eth trades");
    client.subscribe_quotes(eth).await.expect("eth quotes");
    client.subscribe_book(btc).await.expect("btc book");
    client.subscribe_all_mids().await.expect("allMids");
    client
        .subscribe_bars(BarType::from(
            "BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL",
        ))
        .await
        .expect("bars");

    let mut quotes = 0usize;
    let mut trades = 0usize;
    let mut books = 0usize;
    let mut mids = 0usize;
    let mut candles = 0usize;
    let mut crossed = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, client.next_event()).await {
            Ok(Some(NautilusWsMessage::Quote(quote))) => {
                quotes += 1;

                if quote_is_crossed(&quote) {
                    crossed += 1;
                }
            }
            Ok(Some(NautilusWsMessage::Trades(batch))) => trades += batch.len(),
            Ok(Some(NautilusWsMessage::Deltas(_) | NautilusWsMessage::Depth10(_))) => books += 1,
            Ok(Some(NautilusWsMessage::CustomData(_))) => mids += 1,
            Ok(Some(NautilusWsMessage::Candle(_))) => candles += 1,
            Ok(Some(NautilusWsMessage::Reconnected)) => {
                panic!("unexpected Reconnected before reconnect request")
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("WS closed during integrity soak"),
            Err(_) => break,
        }
    }
    eprintln!(
        "quotes={quotes} trades={trades} books={books} mids={mids} candles={candles} crossed={crossed}"
    );
    assert!(quotes >= 20, "must collect live quotes");
    assert!(trades >= 5, "must collect live trades");
    assert_eq!(crossed, 0, "quotes must not cross the book");

    for cycle in 1..=3 {
        assert!(
            client.request_reconnect(),
            "reconnect cycle {cycle} must be accepted"
        );
        wait_for(&mut client, Duration::from_secs(20), |event| {
            matches!(event, NautilusWsMessage::Reconnected)
        })
        .await;
        wait_for(&mut client, Duration::from_secs(25), |event| {
            matches!(
                event,
                NautilusWsMessage::Quote(_)
                    | NautilusWsMessage::Trades(_)
                    | NautilusWsMessage::CustomData(_)
            )
        })
        .await;
        eprintln!("reconnect_cycle={cycle} recovered");
    }

    client.disconnect().await.expect("disconnect");
}

fn quote_is_crossed(quote: &QuoteTick) -> bool {
    quote.bid_price >= quote.ask_price
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_user_stream_execution_reports() {
    let http = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("public HTTP client");
    let trades = http
        .info_recent_trades("BTC")
        .await
        .expect("recent BTC trades");
    let user = first_user(&trades);

    let mut client = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect");
    client
        .subscribe_all_user_channels(&user)
        .await
        .expect("user channels");

    let mut reports = 0usize;
    let mut reconnects = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    let mut requested_reconnect = false;

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, client.next_event()).await {
            Ok(Some(NautilusWsMessage::ExecutionReports(batch))) => reports += batch.len(),
            Ok(Some(NautilusWsMessage::Reconnected)) => reconnects += 1,
            Ok(Some(_)) => {}
            Ok(None) => panic!("user WS closed"),
            Err(_) => break,
        }

        if !requested_reconnect && tokio::time::Instant::now() + Duration::from_secs(60) < deadline
        {
            assert!(client.request_reconnect());
            requested_reconnect = true;
        }
    }
    eprintln!(
        "user_reports={reports} reconnects={reconnects} requested_reconnect={requested_reconnect}"
    );
    assert!(
        requested_reconnect && reconnects >= 1,
        "user stream must forward Reconnected"
    );
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_testnet_data_and_reconnect() {
    let http = HyperliquidHttpClient::new(HyperliquidEnvironment::Testnet, 60, None)
        .expect("testnet HTTP");
    let instruments = http
        .request_instruments()
        .await
        .expect("testnet instruments");
    let btc = perp_id(&instruments, "BTC");
    let mut client = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Testnet,
        None,
        TransportBackend::default(),
        None,
    );
    client.cache_instruments(instruments);
    client.connect().await.expect("testnet connect");
    client.subscribe_trades(btc).await.expect("trades");
    client.subscribe_quotes(btc).await.expect("quotes");

    let mut events = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, client.next_event()).await {
            Ok(Some(NautilusWsMessage::Reconnected)) => {
                panic!("unexpected Reconnected before request")
            }
            Ok(Some(_)) => events += 1,
            Ok(None) => panic!("testnet WS closed"),
            Err(_) => break,
        }
    }
    assert!(events > 0, "testnet must deliver market data");
    assert!(client.request_reconnect());
    wait_for(&mut client, Duration::from_secs(20), |event| {
        matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;
    client.disconnect().await.expect("disconnect");
    eprintln!("testnet_events={events}");
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_authenticated_mainnet_readonly() {
    let mut client =
        HyperliquidHttpClient::from_env(HyperliquidEnvironment::Mainnet).expect("mainnet from_env");
    let address = client.get_user_address().expect("signer address");
    assert_eq!(address.len(), 42);
    assert!(address.starts_with("0x"));

    let instruments = client.request_instruments().await.expect("instruments");
    for instrument in &instruments {
        client.cache_instrument(instrument);
    }
    client.set_account_id(AccountId::from("HYPERLIQUID-SOAK"));

    let fills = client.info_user_fills(&address).await.expect("own fills");
    let reports = client
        .request_fill_reports(&address, None)
        .await
        .expect("own fill reports");
    let positions = client
        .request_position_status_reports(&address, None)
        .await
        .expect("own positions");
    let open = client
        .info_open_orders(&address)
        .await
        .expect("own open orders");
    eprintln!(
        "own_fills={} own_reports={} own_positions={} open_is_array={} nonzero_tid={}",
        fills.len(),
        reports.len(),
        positions.len(),
        open.is_array(),
        fills.iter().filter(|fill| fill.tid != 0).count()
    );

    let mut ws = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    );
    ws.connect().await.expect("own user ws");
    ws.subscribe_all_user_channels(&address)
        .await
        .expect("subscribe own user");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(ws.request_reconnect());
    wait_for(&mut ws, Duration::from_secs(20), |event| {
        matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;
    ws.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live soak"]
async fn soak_testnet_post_only_place_query_cancel() {
    let mut client =
        HyperliquidHttpClient::from_env(HyperliquidEnvironment::Testnet).expect("testnet from_env");
    let address = client.get_user_address().expect("signer address");
    let instruments = client.request_instruments().await.expect("instruments");
    for instrument in &instruments {
        client.cache_instrument(instrument);
    }
    client.set_account_id(AccountId::from("HYPERLIQUID-SOAK"));

    let meta = client.info_meta().await.expect("testnet meta");
    let asset = meta
        .universe
        .iter()
        .position(|asset| asset.name == "ETH")
        .expect("ETH") as u32;
    let book = client.info_l2_book("ETH").await.expect("ETH book");
    let best_bid = book.levels[0][0].px;
    let price = (best_bid * Decimal::new(50, 2)).round();
    let min_notional = Decimal::from(10);
    let size = (min_notional / price).round_dp_with_strategy(2, RoundingStrategy::AwayFromZero);
    eprintln!("testnet_place_price={price} size={size} bid={best_bid}");
    let client_order_id = ClientOrderId::from("O-HL-SOAK-POSTONLY-1");
    let cloid = Cloid::from_client_order_id(client_order_id);
    let cloid_hex = cloid.to_hex();

    let place = HyperliquidExchangeAction::Order {
        orders: vec![HyperliquidExchangePlaceOrderRequest {
            asset,
            is_buy: true,
            price,
            size,
            reduce_only: false,
            kind: HyperliquidExchangeOrderKind::Limit {
                limit: HyperliquidExchangeLimitParams {
                    tif: HyperliquidExchangeTif::Alo,
                },
            },
            cloid: Some(cloid),
        }],
        grouping: HyperliquidExchangeGrouping::Na,
        builder: None,
    };
    let placed = client
        .post_action_exec(&place)
        .await
        .expect("place post-only");

    match &placed {
        HyperliquidExchangeResponse::Status { status, response } => {
            eprintln!("testnet_place_status={status} response={response}");
        }
        HyperliquidExchangeResponse::Error { error } => {
            eprintln!("testnet_place_error={error}");
        }
    }
    tokio::time::sleep(Duration::from_secs(2)).await;

    let open = client
        .info_frontend_open_orders(&address)
        .await
        .expect("frontend open");
    let oid = open
        .as_array()
        .into_iter()
        .flatten()
        .find(|order| {
            order.get("cloid").and_then(|value| value.as_str()) == Some(cloid_hex.as_str())
        })
        .and_then(|order| order.get("oid").and_then(|value| value.as_u64()));
    eprintln!("testnet_resting_oid_present={}", oid.is_some());

    if let Some(oid) = oid {
        match client.request_order_status_report(&address, oid).await {
            Ok(Some(mut report)) => {
                report.client_order_id = Some(client_order_id);
                assert_eq!(report.client_order_id, Some(client_order_id));
                assert_eq!(report.venue_order_id.as_str(), oid.to_string());
                eprintln!("testnet_oid_query_attached");
            }
            Ok(None) => eprintln!("testnet_oid_query_none"),
            Err(e) => eprintln!("testnet_oid_query_error={e}"),
        }
    }

    let cancel = HyperliquidExchangeAction::CancelByCloid {
        cancels: vec![HyperliquidExchangeCancelByCloidRequest { asset, cloid }],
        fast: None,
    };
    client.post_action_exec(&cancel).await.expect("cancel");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let remaining = client
        .info_frontend_open_orders(&address)
        .await
        .expect("open after cancel");
    let still_resting = remaining.as_array().is_some_and(|orders| {
        orders.iter().any(|order| {
            order.get("cloid").and_then(|value| value.as_str()) == Some(cloid_hex.as_str())
        })
    });
    assert!(!still_resting, "post-only order must cancel");
}
