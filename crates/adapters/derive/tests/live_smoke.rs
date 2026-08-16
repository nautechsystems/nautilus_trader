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

//! Bounded live smoke tests against the Derive venue.
//!
//! These run only when explicitly selected:
//!
//! ```text
//! cargo test -p nautilus-derive --test live_smoke -- --include-ignored
//! ```
//!
//! The public REST leg needs no credentials. The authenticated legs read
//! `DERIVE_WALLET_ADDRESS`, `DERIVE_SESSION_PRIVATE_KEY`, and
//! `DERIVE_SUBACCOUNT_ID` from the environment. The matching-write leg
//! additionally requires `DERIVE_LIVE_ORDER_SMOKE=1`: it places one
//! far-from-market minimal limit order and cancels it, exercising the
//! fixed-window matching pacing (account-wide and per-instrument) end to end.

use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nautilus_core::UnixNanos;
use nautilus_derive::{
    common::{
        consts::{ACTION_TYPEHASH, DOMAIN_SEPARATOR_MAINNET, TRADE_MODULE_ADDRESS_MAINNET},
        enums::{DeriveEnvironment, DeriveOrderSide, DeriveOrderType, DeriveTimeInForce},
        urls,
    },
    http::{
        DeriveCredentials, DeriveHttpClient,
        models::DerivePublicTrade,
        parse::{
            parse_derive_order_to_report, parse_derive_position_to_report,
            parse_derive_subaccount_to_balances, parse_derive_trade_to_fill_report,
        },
        query::{
            DeriveCancelParams, DeriveGetOpenOrdersParams, DeriveGetPositionsParams,
            DeriveGetSubaccountParams, DeriveGetTradeHistoryParams, DeriveGetTriggerOrdersParams,
            order_to_derive_payload,
        },
    },
    signing::nonce::NonceManager,
    websocket::{
        DeriveWebSocketClient, DeriveWsChannel, DeriveWsCredentials, DeriveWsMessage,
        parse_trade_tick, parse_trade_tick_from_rest, parse_trades_msg,
    },
};
use nautilus_model::{
    enums::{AggressorSide, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, InstrumentId},
    orders::{OrderAny, OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

const INSTRUMENT_NAME: &str = "ETH-PERP";

struct LiveCredentials {
    wallet_address: String,
    subaccount_id: u64,
    session_key: String,
}

fn live_credentials() -> LiveCredentials {
    let missing = [
        "DERIVE_WALLET_ADDRESS",
        "DERIVE_SESSION_PRIVATE_KEY",
        "DERIVE_SUBACCOUNT_ID",
    ]
    .iter()
    .find(|name| std::env::var(name).unwrap_or_default().trim().is_empty())
    .unwrap_or(&"DERIVE_WALLET_ADDRESS");
    let wallet_address = std::env::var("DERIVE_WALLET_ADDRESS").unwrap_or_default();
    let session_key = std::env::var("DERIVE_SESSION_PRIVATE_KEY").unwrap_or_default();
    let subaccount_id = std::env::var("DERIVE_SUBACCOUNT_ID").unwrap_or_default();

    if wallet_address.trim().is_empty()
        || session_key.trim().is_empty()
        || subaccount_id.trim().is_empty()
    {
        panic!("live Derive smoke requires {missing} to be set");
    }
    LiveCredentials {
        wallet_address,
        subaccount_id: subaccount_id.trim().parse().expect("subaccount id is u64"),
        session_key,
    }
}

fn utc_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_secs() as i64
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_rest_public_read_reaches_venue() {
    let client =
        DeriveHttpClient::new(urls::rest_url(DeriveEnvironment::Mainnet), None, None, None)
            .expect("client builds");
    let instrument = client
        .get_instrument(INSTRUMENT_NAME)
        .await
        .expect("public/get_instrument succeeds");
    assert_eq!(instrument.instrument_name.as_str(), INSTRUMENT_NAME);
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_rest_public_trade_history_pairs_agree_on_aggressor_side() {
    let client =
        DeriveHttpClient::new(urls::rest_url(DeriveEnvironment::Mainnet), None, None, None)
            .expect("client builds");

    for instrument_name in [INSTRUMENT_NAME, "BTC-PERP"] {
        let (rows, unique) = live_public_pairing_snapshot(&client, instrument_name).await;
        log::info!(
            "live public trade history {instrument_name}: {rows} rows, {unique} unique trades, paired rows agree on the aggressor side",
        );
    }
}

/// Walks the first pages of public trade history and returns the row count and
/// unique trade count, asserting paired rows agree on the aggressor side.
async fn live_public_pairing_snapshot(
    client: &DeriveHttpClient,
    instrument_name: &str,
) -> (usize, usize) {
    let instrument = client
        .get_instrument(instrument_name)
        .await
        .unwrap_or_else(|e| panic!("public/get_instrument for {instrument_name}: {e}"));
    let price_precision = u8::try_from(instrument.tick_size.scale()).expect("price scale fits u8");
    let size_precision =
        u8::try_from(instrument.amount_step.scale()).expect("amount scale fits u8");

    let mut aggressor_by_trade_id: HashMap<String, AggressorSide> = HashMap::new();
    let mut rows = 0;

    for page in 1..=2 {
        let result = client
            .get_trade_history(instrument_name, None, None, page, 100)
            .await
            .unwrap_or_else(|e| panic!("public/get_trade_history page {page}: {e}"));
        if result.trades.is_empty() {
            break;
        }

        rows += result.trades.len();
        for trade in &result.trades {
            let tick = parse_trade_tick_from_rest(
                trade,
                price_precision,
                size_precision,
                UnixNanos::from(0),
            )
            .unwrap_or_else(|e| panic!("live public trade {} must parse: {e}", trade.trade_id));
            if let Some(previous) =
                aggressor_by_trade_id.insert(trade.trade_id.clone(), tick.aggressor_side)
            {
                assert_eq!(
                    previous, tick.aggressor_side,
                    "paired rows for trade {} disagree on the aggressor side",
                    trade.trade_id,
                );
            }
        }

        if i64::from(page) >= result.pagination.num_pages {
            break;
        }
    }

    assert!(
        !aggressor_by_trade_id.is_empty(),
        "{instrument_name} must return recent public trades",
    );
    assert!(
        rows > aggressor_by_trade_id.len(),
        "must observe paired maker/taker rows under shared trade ids for {instrument_name}",
    );
    (rows, aggressor_by_trade_id.len())
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_ws_public_trades_match_rest_aggressor_side() {
    // The public WS feed defines `direction` as the taker side while REST
    // derives the aggressor from the row role; a live overlap must produce
    // identical aggressor sides through both parsers. Derive public trade flow
    // is bursty, so this listens across several trades channels and fails
    // loudly when the venue is too quiet to prove the overlap.
    let mut ws =
        DeriveWebSocketClient::new(None, DeriveEnvironment::Mainnet, Default::default(), None);
    ws.connect().await.expect("public ws connects");
    let handle = ws.subscription_handle();

    for (instrument_type, currency) in [
        ("perp", "ETH"),
        ("perp", "BTC"),
        ("option", "ETH"),
        ("erc20", "ETH"),
    ] {
        handle
            .subscribe_trades(instrument_type, currency)
            .await
            .unwrap_or_else(|e| {
                panic!("trades.{instrument_type}.{currency} subscribes: {e}");
            });
    }

    // Rows are collected raw and parsed later with each instrument's real
    // precisions, since the subscribed channels deliver any instrument.
    let mut ws_trades: Vec<DerivePublicTrade> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while let Ok(event) = tokio::time::timeout_at(deadline, ws.next_event()).await {
        match event {
            Some(DeriveWsMessage::Subscription(payload)) => {
                if !payload.channel.starts_with("trades.") {
                    continue;
                }
                let msg = parse_trades_msg(&payload)
                    .unwrap_or_else(|e| panic!("live ws trades frame must parse: {e}"));
                ws_trades.extend(msg.trades);
            }
            Some(_) => {}
            None => break,
        }
    }
    ws.disconnect().await.expect("disconnect");

    assert!(
        !ws_trades.is_empty(),
        "no live public trades observed within the window; rerun when the venue is active",
    );

    let http = DeriveHttpClient::new(urls::rest_url(DeriveEnvironment::Mainnet), None, None, None)
        .expect("client builds");
    let mut distinct_instruments: Vec<String> = Vec::new();

    for trade in &ws_trades {
        let instrument_name = trade.instrument_name.to_string();
        if !distinct_instruments.contains(&instrument_name) {
            distinct_instruments.push(instrument_name);
        }
    }
    let mut matched = 0;

    for instrument_name in &distinct_instruments {
        let instrument = http
            .get_instrument(instrument_name)
            .await
            .unwrap_or_else(|e| panic!("public/get_instrument for {instrument_name}: {e}"));
        let price_precision =
            u8::try_from(instrument.tick_size.scale()).expect("price scale fits u8");
        let size_precision =
            u8::try_from(instrument.amount_step.scale()).expect("amount scale fits u8");
        let result = http
            .get_trade_history(instrument_name, None, None, 1, 100)
            .await
            .unwrap_or_else(|e| panic!("public/get_trade_history for {instrument_name}: {e}"));

        let mut rest_aggressor_by_trade_id = HashMap::new();

        for trade in &result.trades {
            let tick = parse_trade_tick_from_rest(
                trade,
                price_precision,
                size_precision,
                UnixNanos::from(0),
            )
            .unwrap_or_else(|e| panic!("live public trade {} must parse: {e}", trade.trade_id));
            rest_aggressor_by_trade_id.insert(trade.trade_id.clone(), tick.aggressor_side);
        }

        for trade in &ws_trades {
            if trade.instrument_name.as_str() != instrument_name {
                continue;
            }
            let ws_tick =
                parse_trade_tick(trade, price_precision, size_precision, UnixNanos::from(0))
                    .unwrap_or_else(|e| panic!("live ws trade {} must parse: {e}", trade.trade_id));
            if let Some(rest_side) = rest_aggressor_by_trade_id.get(&trade.trade_id) {
                assert_eq!(
                    ws_tick.aggressor_side, *rest_side,
                    "WS and REST disagree on the aggressor side for trade {}",
                    trade.trade_id,
                );
                matched += 1;
            }
        }
    }

    assert!(
        matched > 0,
        "must match at least one trade across the live WS and REST feeds",
    );
    log::info!(
        "live ws trades cross-check: {} ws trades across {} instruments, {matched} matched with REST, all agree on the aggressor side",
        ws_trades.len(),
        distinct_instruments.len(),
    );
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_ws_login_and_private_read() {
    let creds = live_credentials();
    let ws_creds = DeriveWsCredentials::new(creds.wallet_address.clone(), &creds.session_key)
        .expect("session key parses");
    let mut client = DeriveWebSocketClient::with_credentials(
        None,
        DeriveEnvironment::Mainnet,
        Default::default(),
        None,
        ws_creds,
        None,
        None,
    );
    client.connect().await.expect("login succeeds");
    assert!(client.is_authenticated());

    // A private read paces against the WebSocket non-matching window; the
    // discriminating check is that login and the read complete live.
    let exec = client.execution_handle();
    let result = exec
        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(creds.subaccount_id))
        .await
        .expect("private/get_trigger_orders succeeds");
    log::info!(
        "live get_trigger_orders returned {} orders",
        result.orders.len(),
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_rest_trade_history_commissions_construct_exactly() {
    let creds = live_credentials();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(DeriveEnvironment::Mainnet),
        DeriveCredentials::new(&creds.wallet_address, &creds.session_key).unwrap(),
        None,
        None,
        None,
    )
    .expect("http client builds");

    let result = http
        .get_private_trade_history(&DeriveGetTradeHistoryParams::new(
            creds.subaccount_id,
            1,
            100,
        ))
        .await
        .expect("private/get_trade_history succeeds");

    let account_id = AccountId::new("DERIVE-SMOKE");
    let mut commissions = 0;

    for trade in &result.trades {
        let report = parse_derive_trade_to_fill_report(
            trade,
            account_id,
            Currency::USDC(),
            UnixNanos::from(0),
        )
        .unwrap_or_else(|e| panic!("live trade_fee must construct exactly: {e}"));
        if let Some(report) = report {
            assert_eq!(report.commission.currency, Currency::USDC());
            commissions += 1;
        }
    }

    log::info!(
        "live trade history parsed {} trades, {} commissions",
        result.trades.len(),
        commissions,
    );
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_subaccount_maps_collateral_and_margin_account_state() {
    let creds = live_credentials();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(DeriveEnvironment::Mainnet),
        DeriveCredentials::new(&creds.wallet_address, &creds.session_key).unwrap(),
        None,
        None,
        None,
    )
    .expect("http client builds");

    let subaccount = http
        .get_subaccount(&DeriveGetSubaccountParams::new(creds.subaccount_id))
        .await
        .expect("private/get_subaccount succeeds");

    let (balances, margins, info) = parse_derive_subaccount_to_balances(&subaccount)
        .expect("live subaccount maps to account state");

    // Collateral credit must not surface as locked funds: every balance row
    // stays in its own units with nothing reserved per collateral.
    for balance in &balances {
        assert_eq!(balance.locked.as_decimal(), dec!(0));
        assert_eq!(balance.free.as_decimal(), balance.total.as_decimal());
    }

    // Requirements aggregate position and open-order initial margin; the
    // signed net health values travel in `info` instead of the margins.
    assert_eq!(margins.len(), 1);
    let expected_initial = subaccount.positions_initial_margin + subaccount.open_orders_margin;
    assert_eq!(margins[0].initial.as_decimal(), expected_initial);
    assert_eq!(
        margins[0].maintenance.as_decimal(),
        subaccount.positions_maintenance_margin,
    );
    assert_eq!(
        info.get("net_initial_margin").and_then(|v| v.as_str()),
        Some(subaccount.initial_margin.to_string().as_str()),
    );
    assert_eq!(
        info.get("net_maintenance_margin").and_then(|v| v.as_str()),
        Some(subaccount.maintenance_margin.to_string().as_str()),
    );

    log::info!(
        "live subaccount mapped {} collateral balances, initial requirement {} (raw: positions_initial_margin={}, open_orders_margin={}, positions_maintenance_margin={}, net health initial={} maintenance={})",
        balances.len(),
        margins[0].initial,
        subaccount.positions_initial_margin,
        subaccount.open_orders_margin,
        subaccount.positions_maintenance_margin,
        subaccount.initial_margin,
        subaccount.maintenance_margin,
    );
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_reconciliation_reads_parse_every_report_path() {
    // Exercises the same read set as reconnect reconciliation, parsing every
    // returned row through the adapter's report parsers.
    let creds = live_credentials();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(DeriveEnvironment::Mainnet),
        DeriveCredentials::new(&creds.wallet_address, &creds.session_key).unwrap(),
        None,
        None,
        None,
    )
    .expect("http client builds");
    let account_id = AccountId::new("DERIVE-SMOKE");
    let ts = UnixNanos::from(0);

    let subaccount = http
        .get_subaccount(&DeriveGetSubaccountParams::new(creds.subaccount_id))
        .await
        .expect("private/get_subaccount succeeds");
    let (balances, margins, info) =
        parse_derive_subaccount_to_balances(&subaccount).expect("subaccount maps to account state");

    for balance in &balances {
        assert_eq!(balance.locked.as_decimal(), dec!(0));
        assert_eq!(balance.free.as_decimal(), balance.total.as_decimal());
    }
    assert_eq!(margins.len(), 1);
    assert_eq!(
        margins[0].initial.as_decimal(),
        subaccount.positions_initial_margin + subaccount.open_orders_margin,
    );
    assert_eq!(
        info.get("net_initial_margin").and_then(|v| v.as_str()),
        Some(subaccount.initial_margin.to_string().as_str()),
    );

    let open_orders = http
        .get_open_orders(&DeriveGetOpenOrdersParams::new(creds.subaccount_id))
        .await
        .expect("private/get_open_orders succeeds");
    let trigger_orders = http
        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(creds.subaccount_id))
        .await
        .expect("private/get_trigger_orders succeeds");
    let mut order_reports = 0;

    for order in open_orders
        .orders
        .iter()
        .chain(trigger_orders.orders.iter())
    {
        parse_derive_order_to_report(order, account_id, ts)
            .unwrap_or_else(|e| panic!("live order must parse: {e}"));
        order_reports += 1;
    }

    let trades = http
        .get_private_trade_history(&DeriveGetTradeHistoryParams::new(
            creds.subaccount_id,
            1,
            100,
        ))
        .await
        .expect("private/get_trade_history succeeds");
    let mut fills = 0;

    for trade in &trades.trades {
        let report = parse_derive_trade_to_fill_report(trade, account_id, Currency::USDC(), ts)
            .unwrap_or_else(|e| panic!("live trade must parse: {e}"));
        if let Some(report) = report {
            assert_eq!(report.commission.currency, Currency::USDC());
            fills += 1;
        }
    }

    let positions = http
        .get_positions(&DeriveGetPositionsParams::new(creds.subaccount_id))
        .await
        .expect("private/get_positions succeeds");
    let mut position_reports = 0;

    for position in &positions.positions {
        parse_derive_position_to_report(position, account_id, ts)
            .unwrap_or_else(|e| panic!("live position must parse: {e}"));
        position_reports += 1;
    }

    log::info!(
        "live reconciliation parsed {} balances, margins initial {}, {} order reports, {} fills, {} position reports",
        balances.len(),
        margins[0].initial,
        order_reports,
        fills,
        position_reports,
    );
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_subaccount_mapping_is_stable_across_repeat_polls() {
    let creds = live_credentials();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(DeriveEnvironment::Mainnet),
        DeriveCredentials::new(&creds.wallet_address, &creds.session_key).unwrap(),
        None,
        None,
        None,
    )
    .expect("http client builds");
    let account_id = AccountId::new("DERIVE-SMOKE");

    let mut observed_totals: Vec<String> = Vec::new();

    for poll in 0..10 {
        let subaccount = http
            .get_subaccount(&DeriveGetSubaccountParams::new(creds.subaccount_id))
            .await
            .unwrap_or_else(|e| panic!("poll {poll} failed: {e}"));
        let (balances, margins, info) = parse_derive_subaccount_to_balances(&subaccount)
            .unwrap_or_else(|e| panic!("poll {poll} failed to map: {e}"));

        assert!(!balances.is_empty(), "poll {poll} lost collateral rows");
        for balance in &balances {
            assert_eq!(balance.locked.as_decimal(), dec!(0), "poll {poll}");
            assert_eq!(balance.free.as_decimal(), balance.total.as_decimal());
        }
        assert_eq!(margins.len(), 1, "poll {poll}");
        assert_eq!(
            margins[0].initial.as_decimal(),
            subaccount.positions_initial_margin + subaccount.open_orders_margin,
            "poll {poll}",
        );
        assert_eq!(
            margins[0].maintenance.as_decimal(),
            subaccount.positions_maintenance_margin,
            "poll {poll}",
        );
        assert_eq!(
            info.get("net_initial_margin").and_then(|v| v.as_str()),
            Some(subaccount.initial_margin.to_string().as_str()),
            "poll {poll}",
        );
        assert_eq!(
            info.get("net_maintenance_margin").and_then(|v| v.as_str()),
            Some(subaccount.maintenance_margin.to_string().as_str()),
            "poll {poll}",
        );
        assert_eq!(
            info.get("is_under_liquidation"),
            Some(&serde_json::json!(false)),
            "poll {poll}",
        );
        observed_totals.push(
            balances
                .iter()
                .map(|b| b.total.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    let distinct: std::collections::BTreeSet<&str> =
        observed_totals.iter().map(String::as_str).collect();
    log::info!(
        "live repeat polls observed {} distinct balance sets: {:?}",
        distinct.len(),
        distinct,
    );
    let _ = account_id;
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_ws_balances_channel_subscribes_and_stays_connected() {
    let creds = live_credentials();
    let ws_creds = DeriveWsCredentials::new(creds.wallet_address.clone(), &creds.session_key)
        .expect("session key parses");
    let mut client = DeriveWebSocketClient::with_credentials(
        None,
        DeriveEnvironment::Mainnet,
        Default::default(),
        None,
        ws_creds,
        None,
        None,
    );
    client.connect().await.expect("login succeeds");
    assert!(client.is_authenticated());

    let channel = DeriveWsChannel::balances(creds.subaccount_id);
    client
        .subscribe_channels(vec![channel])
        .await
        .expect("balances channel subscribes");
    assert_eq!(client.subscription_count(), 1);

    // The execution client refreshes authoritative state from REST when a
    // `.balances` notification arrives; on a dormant account none will fire,
    // so the check is that the subscription stays confirmed and the session
    // stays live for a sustained window.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(client.is_active(), "session must stay connected");
    assert_eq!(client.subscription_count(), 1);

    client
        .unsubscribe_channels(vec![DeriveWsChannel::balances(creds.subaccount_id)])
        .await
        .expect("balances channel unsubscribes");
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
#[ignore = "live order placement on api.lyra.finance; requires DERIVE_LIVE_ORDER_SMOKE=1 and --include-ignored"]
async fn test_live_matching_write_far_from_market_then_cancel() {
    if std::env::var("DERIVE_LIVE_ORDER_SMOKE")
        .unwrap_or_default()
        .trim()
        != "1"
    {
        panic!("set DERIVE_LIVE_ORDER_SMOKE=1 to place the bounded far-from-market order");
    }
    let creds = live_credentials();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(DeriveEnvironment::Mainnet),
        DeriveCredentials::new(&creds.wallet_address, &creds.session_key).unwrap(),
        None,
        None,
        None,
    )
    .expect("http client builds");
    let instrument = http
        .get_instrument(INSTRUMENT_NAME)
        .await
        .expect("instrument definition");

    let ws_creds =
        DeriveWsCredentials::new(creds.wallet_address.clone(), &creds.session_key).unwrap();
    let mut client = DeriveWebSocketClient::with_credentials(
        None,
        DeriveEnvironment::Mainnet,
        Default::default(),
        None,
        ws_creds,
        None,
        None,
    );
    client.connect().await.expect("login succeeds");
    let exec = client.execution_handle();

    let order: OrderAny = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(InstrumentId::from("ETH-PERP.DERIVE"))
        .side(OrderSide::Buy)
        .price(Price::from("10.00"))
        .quantity(Quantity::from("0.1"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    let nonce = NonceManager::new()
        .next_nonce(&creds.wallet_address, creds.subaccount_id)
        .expect("nonce");
    let expiry = utc_now_secs() + 600;
    let payload = order_to_derive_payload(
        &order,
        &instrument,
        creds.subaccount_id,
        creds.wallet_address.parse().expect("wallet parses"),
        &DeriveWsCredentials::new(&creds.wallet_address, &creds.session_key)
            .unwrap()
            .signer,
        nonce,
        expiry,
        TRADE_MODULE_ADDRESS_MAINNET
            .parse()
            .expect("module address"),
        DOMAIN_SEPARATOR_MAINNET.parse().expect("domain separator"),
        ACTION_TYPEHASH.parse().expect("action typehash"),
        dec!(2),
        None,
    )
    .expect("signed payload builds");
    assert_eq!(payload.instrument_name, Ustr::from(INSTRUMENT_NAME));
    assert_eq!(payload.direction, DeriveOrderSide::Buy);
    assert_eq!(payload.order_type, DeriveOrderType::Limit);
    assert_eq!(payload.time_in_force, DeriveTimeInForce::Gtc);

    let accepted = exec.submit_order(&payload).await.expect("order accepted");
    let venue_order_id = accepted.order_id.clone();

    let canceled = exec
        .cancel_order(&DeriveCancelParams::new(
            creds.subaccount_id,
            INSTRUMENT_NAME,
            venue_order_id.as_str(),
        ))
        .await;
    // Disconnect before asserting so a failed cancel does not strand the
    // connection while the order may still be resting on the account.
    client.disconnect().await.expect("disconnect");
    assert!(
        canceled.is_ok(),
        "far-from-market order must cancel cleanly: {:?}",
        canceled.err(),
    );
}
