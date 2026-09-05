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

//! Non-mutating Hyperliquid live smoke. Run with:
//! `cargo test -p nautilus-hyperliquid --test integration live_mainnet_smoke:: -- --ignored --nocapture`

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use nautilus_core::string::secret::REDACTED;
use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment,
    config::{HyperliquidDataClientConfig, HyperliquidExecutionClientConfig},
    http::{
        HyperliquidHttpClient,
        models::{HyperliquidOrderStatus, HyperliquidRecentTrade},
    },
    websocket::{client::HyperliquidWebSocketClient, messages::NautilusWsMessage},
};
use nautilus_model::{
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{SocketState, SocketStateSink, websocket::TransportBackend};
use rstest::rstest;

fn btc_perp_id(instruments: &[InstrumentAny]) -> InstrumentId {
    let btc = instruments
        .iter()
        .find(|instrument| instrument.raw_symbol().as_str() == "BTC")
        .expect("BTC-USD-PERP must exist");

    match btc {
        InstrumentAny::CryptoPerpetual(instrument) => instrument.id,
        _ => panic!("BTC must be a perpetual"),
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

async fn wait_for_event(
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
#[ignore = "live mainnet smoke"]
async fn live_mainnet_http_public_and_fill_paths() {
    let mut http = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("public HTTP client");

    let meta = http.info_meta().await.expect("info_meta");
    assert!(
        meta.universe.len() > 10,
        "mainnet meta must list many markets"
    );

    let book = http.info_l2_book("BTC").await.expect("BTC l2 book");
    assert_eq!(book.levels.len(), 2, "l2 book must have bid and ask sides");
    assert!(
        !book.levels[0].is_empty() && !book.levels[1].is_empty(),
        "BTC book must have bids and asks"
    );

    let instruments = http.request_instruments().await.expect("instruments");
    assert!(
        instruments
            .iter()
            .any(|instrument| instrument.raw_symbol().as_str() == "BTC"),
        "instrument catalog must include BTC"
    );

    for instrument in &instruments {
        http.cache_instrument(instrument);
    }

    let trades = http
        .info_recent_trades("BTC")
        .await
        .expect("recent BTC trades");
    assert!(!trades.is_empty());
    assert!(
        trades.iter().any(|trade| trade.tid != 0),
        "recent trades must carry venue tid"
    );

    let user = first_user(&trades);
    let fills = http
        .info_user_fills(&user)
        .await
        .expect("userFills must deserialize");
    assert!(
        fills.iter().any(|fill| fill.tid != 0),
        "live userFills must retain a non-zero venue tid"
    );
    let builder_fee_seen = fills.iter().any(|fill| fill.builder_fee.is_some());
    eprintln!("builder_fee_present_on_sample={builder_fee_seen}");

    http.set_account_id(AccountId::from("HYPERLIQUID-SMOKE"));
    let reports = http
        .request_fill_reports(&user, None)
        .await
        .expect("FillReport parse");
    assert!(
        !reports.is_empty(),
        "cached instruments must produce at least one FillReport"
    );
    assert!(
        reports
            .iter()
            .all(|report| !report.trade_id.to_string().is_empty()),
        "parsed fill reports must have a trade id"
    );

    let oid = fills
        .iter()
        .map(|fill| fill.oid)
        .find(|oid| *oid != 0)
        .expect("fill must carry oid");
    let status = http
        .info_order_status(&user, oid)
        .await
        .expect("orderStatus");

    match status {
        HyperliquidOrderStatus::Order { .. } | HyperliquidOrderStatus::UnknownOid => {}
    }

    let parsed_status = http
        .request_order_status_report(&user, oid)
        .await
        .expect("order status report query");
    if let Some(mut report) = parsed_status {
        let known = ClientOrderId::new("O-SMOKE-ATTACH");
        report.client_order_id = Some(known);
        assert_eq!(report.client_order_id, Some(known));
        assert_eq!(
            report.venue_order_id.as_str(),
            oid.to_string(),
            "oid query must keep the venue order id"
        );
    }

    let open_orders = http.info_open_orders(&user).await.expect("openOrders");
    assert!(open_orders.is_array() || open_orders.is_object());

    let state = http
        .info_clearinghouse_state(&user)
        .await
        .expect("clearinghouseState");
    assert!(state.is_object(), "clearinghouse state must be an object");
}

#[rstest]
#[tokio::test]
#[ignore = "live mainnet smoke"]
async fn live_mainnet_data_streams_allmids_book_and_reconnect() {
    let http = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("public HTTP client");
    let instruments = http.request_instruments().await.expect("instruments");
    let instrument_id = btc_perp_id(&instruments);

    let connected = Arc::new(AtomicUsize::new(0));
    let disconnected = Arc::new(AtomicUsize::new(0));
    let sink = {
        let connected = Arc::clone(&connected);
        let disconnected = Arc::clone(&disconnected);
        SocketStateSink::new(move |state| match state {
            SocketState::Connected => {
                connected.fetch_add(1, Ordering::SeqCst);
            }
            SocketState::Disconnected => {
                disconnected.fetch_add(1, Ordering::SeqCst);
            }
        })
    };

    let mut client = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    )
    .with_state_sink(sink);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe trades");
    client
        .subscribe_quotes(instrument_id)
        .await
        .expect("subscribe quotes");
    client
        .subscribe_book(instrument_id)
        .await
        .expect("subscribe book");
    client
        .subscribe_all_mids()
        .await
        .expect("subscribe allMids");
    client
        .subscribe_mark_prices(instrument_id)
        .await
        .expect("subscribe mark");

    let mut saw_trade = false;
    let mut saw_quote = false;
    let mut saw_book = false;
    let mut saw_mids = false;
    let mut saw_mark = false;
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            match client.next_event().await {
                Some(NautilusWsMessage::Trades(_)) => saw_trade = true,
                Some(NautilusWsMessage::Quote(_)) => saw_quote = true,
                Some(NautilusWsMessage::Deltas(_) | NautilusWsMessage::Depth10(_)) => {
                    saw_book = true;
                }
                Some(NautilusWsMessage::CustomData(_)) => saw_mids = true,
                Some(NautilusWsMessage::MarkPrice(_)) => saw_mark = true,
                Some(NautilusWsMessage::Reconnected) => {
                    panic!("Reconnected must not precede market data")
                }
                Some(_) => {}
                None => panic!("WS closed while collecting market data"),
            }

            if saw_trade && saw_quote && saw_book && saw_mids {
                return;
            }
        }
    })
    .await
    .expect("timeout collecting data-stream types");
    eprintln!("saw_mark={saw_mark}");

    assert!(
        client.request_reconnect(),
        "active data socket must accept reconnect"
    );
    wait_for_event(&mut client, Duration::from_secs(20), |event| {
        matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;

    wait_for_event(&mut client, Duration::from_secs(25), |event| {
        matches!(
            event,
            NautilusWsMessage::Trades(_)
                | NautilusWsMessage::Quote(_)
                | NautilusWsMessage::CustomData(_)
        )
    })
    .await;

    client.disconnect().await.expect("disconnect");
    assert!(connected.load(Ordering::SeqCst) >= 1);
}

#[rstest]
#[tokio::test]
#[ignore = "live mainnet smoke"]
async fn live_mainnet_user_stream_reconnect() {
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
        .expect("subscribe user channels");

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        client.request_reconnect(),
        "active user socket must accept reconnect"
    );
    wait_for_event(&mut client, Duration::from_secs(20), |event| {
        matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;

    let _maybe_exec = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match client.next_event().await {
                Some(NautilusWsMessage::ExecutionReports(_)) => return true,
                Some(NautilusWsMessage::Reconnected) => {}
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;
    eprintln!("user_stream_exec_after_reconnect={_maybe_exec:?}");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live mainnet smoke"]
async fn live_testnet_public_reconnect() {
    let http = HyperliquidHttpClient::new(HyperliquidEnvironment::Testnet, 60, None)
        .expect("public testnet HTTP client");
    let meta = http.info_meta().await.expect("testnet meta");
    assert!(!meta.universe.is_empty());
    let instruments = http
        .request_instruments()
        .await
        .expect("testnet instruments");
    let instrument_id = btc_perp_id(&instruments);

    let mut client = HyperliquidWebSocketClient::new(
        None,
        HyperliquidEnvironment::Testnet,
        None,
        TransportBackend::default(),
        None,
    );
    client.cache_instruments(instruments);
    client.connect().await.expect("testnet connect");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe testnet trades");
    wait_for_event(&mut client, Duration::from_secs(20), |event| {
        !matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;
    assert!(client.request_reconnect());
    wait_for_event(&mut client, Duration::from_secs(20), |event| {
        matches!(event, NautilusWsMessage::Reconnected)
    })
    .await;
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live mainnet smoke"]
async fn live_config_debug_redacts_and_optional_private_paths() {
    let data = HyperliquidDataClientConfig {
        private_key: Some(
            "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..HyperliquidDataClientConfig::default()
    };
    let exec = HyperliquidExecutionClientConfig {
        private_key: Some(
            "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..HyperliquidExecutionClientConfig::default()
    };
    let data_debug = format!("{data:?}");
    let exec_debug = format!("{exec:?}");
    assert!(data_debug.contains(REDACTED));
    assert!(exec_debug.contains(REDACTED));
    assert!(!data_debug.contains("0123456789abcdef"));
    assert!(!exec_debug.contains("0123456789abcdef"));

    match HyperliquidHttpClient::from_env(HyperliquidEnvironment::Mainnet) {
        Ok(client) => {
            let address = client.get_user_address().expect("signer address");
            assert!(address.starts_with("0x"));
            assert_eq!(address.len(), 42);
            let fills = client
                .info_user_fills(&address)
                .await
                .expect("own userFills");
            eprintln!("authenticated_own_fill_count={}", fills.len());
        }
        Err(_) => {
            eprintln!("HYPERLIQUID_PK unset; authenticated exec paths skipped");
        }
    }

    match HyperliquidHttpClient::from_env(HyperliquidEnvironment::Testnet) {
        Ok(client) => {
            let address = client.get_user_address().expect("testnet signer address");
            let _ = client.info_open_orders(&address).await;
            eprintln!("authenticated_testnet_open_orders_queried");
        }
        Err(_) => {
            eprintln!("HYPERLIQUID_TESTNET_PK unset; authenticated testnet exec skipped");
        }
    }
}
