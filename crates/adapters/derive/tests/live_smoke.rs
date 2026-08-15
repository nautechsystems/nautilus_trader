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

use std::time::{SystemTime, UNIX_EPOCH};

use nautilus_core::UnixNanos;
use nautilus_derive::{
    common::{
        consts::{ACTION_TYPEHASH, DOMAIN_SEPARATOR_MAINNET, TRADE_MODULE_ADDRESS_MAINNET},
        enums::{DeriveEnvironment, DeriveOrderSide, DeriveOrderType, DeriveTimeInForce},
        urls,
    },
    http::{
        DeriveCredentials, DeriveHttpClient,
        parse::parse_derive_trade_to_fill_report,
        query::{
            DeriveCancelParams, DeriveGetTradeHistoryParams, DeriveGetTriggerOrdersParams,
            order_to_derive_payload,
        },
    },
    signing::nonce::NonceManager,
    websocket::{DeriveWebSocketClient, DeriveWsCredentials},
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
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
