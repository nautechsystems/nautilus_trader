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

//! Automatic repayment of SPOT margin borrows.
//!
//! When a BUY order fully fills on a SPOT instrument it closes (or reduces) a
//! short position that was opened by borrowing the base coin. Repaying that
//! borrow promptly avoids accruing interest. Fully-filled BUYs are enqueued from
//! the WebSocket dispatch path onto an unbounded channel; a single background
//! consumer task repays each one in turn.
//!
//! Bybit charges the trading fee for a SPOT BUY in the base coin (the coin just
//! bought), so the amount actually received is slightly less than the amount
//! borrowed. Repayment therefore uses the venue's manual (converting) repay
//! endpoint, which draws that small shortfall from other assets rather than
//! failing on an insufficient debt-coin balance.

use std::time::Duration;

use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::types::Quantity;
use ustr::Ustr;

use crate::http::client::BybitHttpClient;

/// Start of Bybit's hourly repayment blackout, in seconds past the hour (mm:04:00).
const BLACKOUT_START_SEC: u64 = 4 * 60;
/// End of Bybit's hourly repayment blackout, in seconds past the hour (mm:05:30).
const BLACKOUT_END_SEC: u64 = 5 * 60 + 30;

/// A request to repay an outstanding SPOT borrow for `coin`, capped at
/// `quantity` (the amount just bought).
#[derive(Clone, Debug)]
pub(crate) struct RepayRequest {
    pub(crate) coin: Ustr,
    pub(crate) quantity: Quantity,
}

/// Delay to wait out Bybit's hourly repayment blackout (mm:04:00-mm:05:30 UTC,
/// end inclusive so repayment resumes at :05:31), or `None` when repayment is allowed.
#[must_use]
fn repay_blackout_delay(now_ns: UnixNanos) -> Option<Duration> {
    let sec_of_hour = now_ns.as_seconds().rem_euclid(3600);
    if (BLACKOUT_START_SEC..=BLACKOUT_END_SEC).contains(&sec_of_hour) {
        Some(Duration::from_secs(BLACKOUT_END_SEC + 1 - sec_of_hour))
    } else {
        None
    }
}

/// Repays each queued SPOT borrow in turn (waiting out the blackout) until the channel closes.
pub(crate) async fn run_spot_repay_consumer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<RepayRequest>,
    http_client: BybitHttpClient,
    clock: &'static AtomicTime,
) {
    log::debug!("Spot repay consumer starting");

    while let Some(req) = rx.recv().await {
        // Repayment is blocked during the hourly blackout; wait it out.
        if let Some(wait) = repay_blackout_delay(clock.get_time_ns()) {
            log::info!(
                "In Bybit repay blackout window; deferring repayment of {} for {wait:?}",
                req.coin,
            );
            tokio::time::sleep(wait).await;
        }

        repay_coin(&http_client, req.coin, req.quantity).await;
    }

    log::debug!("Spot repay consumer stopped");
}

/// Repays the outstanding borrow for a single coin, capped at `bought`.
async fn repay_coin(http_client: &BybitHttpClient, coin: Ustr, bought: Quantity) {
    let coin_str = coin.as_str();

    let outstanding = match http_client.get_spot_borrow_amount(coin_str).await {
        Ok(amount) => amount,
        Err(e) => {
            log::error!("Failed to query borrow amount for {coin}: {e}");
            return;
        }
    };

    if outstanding.is_zero() {
        log::debug!("No outstanding borrow for {coin}");
        return;
    }

    let repay = outstanding.min(bought.as_decimal());
    let repay_qty = match Quantity::from_decimal_dp(repay, bought.precision) {
        Ok(qty) => qty,
        Err(e) => {
            log::error!("Failed to build repay quantity for {coin} ({repay}): {e}");
            return;
        }
    };

    if repay_qty.is_zero() {
        return;
    }

    // The BUY fee is charged in the base coin, so the received amount is a touch
    // below the borrow. Use the converting repay so that small shortfall is
    // covered from other assets instead of failing with an insufficient balance.
    match http_client
        .repay_spot_borrow_with_conversion(coin_str, Some(repay_qty))
        .await
    {
        Ok(_) => log::info!(
            "Repaid {repay} {coin} spot borrow (outstanding was {outstanding}, bought {bought})"
        ),
        Err(e) => log::error!("Failed to repay spot borrow for {coin}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use nautilus_core::UnixNanos;
    use nautilus_core::datetime::NANOSECONDS_IN_SECOND;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde_json::{Value, json};

    use super::*;

    type RepayBodies = Arc<Mutex<Vec<Value>>>;

    #[derive(Clone, Default)]
    struct MockVenue {
        repay_bodies: RepayBodies,
    }

    fn wallet_with_borrow(coin: &str, spot_borrow: &str) -> Value {
        let mut wallet: Value =
            serde_json::from_str(include_str!("../test_data/http_get_wallet_balance.json"))
                .expect("valid wallet balance fixture");

        let coins = wallet["result"]["list"][0]["coin"]
            .as_array_mut()
            .expect("fixture has a coin list");
        let entry = coins
            .iter_mut()
            .find(|entry| entry["coin"] == coin)
            .expect("fixture has the requested coin");
        entry["spotBorrow"] = json!(spot_borrow);

        wallet
    }

    async fn spawn_mock_venue(wallet: Option<Value>) -> (String, RepayBodies) {
        let state = MockVenue::default();
        let repay_bodies = state.repay_bodies.clone();

        let router = Router::new()
            .route(
                "/v5/account/wallet-balance",
                get(move || {
                    let wallet = wallet.clone();
                    async move {
                        match wallet {
                            Some(wallet) => Json(wallet).into_response(),
                            None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                        }
                    }
                }),
            )
            .route("/v5/account/repay", post(handle_repay))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        (format!("http://{addr}"), repay_bodies)
    }

    async fn handle_repay(State(state): State<MockVenue>, body: axum::body::Bytes) -> Response {
        let params: Value = serde_json::from_slice(&body).expect("valid repay body");
        state.repay_bodies.lock().unwrap().push(params);

        Json(json!({
            "retCode": 0,
            "retMsg": "OK",
            "result": {"resultStatus": "SU"},
            "retExtInfo": {},
            "time": 1704470400123i64
        }))
        .into_response()
    }

    fn test_client(base_url: String) -> BybitHttpClient {
        BybitHttpClient::with_credentials(
            "test_api_key".to_string(),
            "test_api_secret".to_string(),
            Some(base_url),
            60,
            0,
            1_000,
            10_000,
            5_000,
            None,
        )
        .unwrap()
    }

    fn single_repay_amount(bodies: &RepayBodies, coin: &str) -> Decimal {
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1, "Expected exactly one repay request");

        let body = &bodies[0];
        assert_eq!(body["coin"], coin);

        body["amount"]
            .as_str()
            .expect("repay request has an amount")
            .parse()
            .expect("repay amount parses as a decimal")
    }

    #[rstest]
    #[case::capped_at_outstanding("5.0", 10.0, dec!(5.0))]
    #[case::capped_at_bought("10.0", 0.25, dec!(0.25))]
    #[tokio::test]
    async fn test_repay_coin_repays_lesser_of_outstanding_and_bought(
        #[case] outstanding: &str,
        #[case] bought: f64,
        #[case] expected: Decimal,
    ) {
        let (base_url, repay_bodies) =
            spawn_mock_venue(Some(wallet_with_borrow("ETH", outstanding))).await;
        let client = test_client(base_url);

        repay_coin(&client, Ustr::from("ETH"), Quantity::new(bought, 3)).await;

        assert_eq!(single_repay_amount(&repay_bodies, "ETH"), expected);
    }

    #[rstest]
    #[case::no_outstanding_borrow("0")]
    #[case::outstanding_below_one_tick("0.000000012")]
    #[tokio::test]
    async fn test_repay_coin_skips_when_nothing_to_repay(#[case] outstanding: &str) {
        let (base_url, repay_bodies) =
            spawn_mock_venue(Some(wallet_with_borrow("ETH", outstanding))).await;
        let client = test_client(base_url);

        repay_coin(&client, Ustr::from("ETH"), Quantity::new(1.0, 3)).await;

        assert!(
            repay_bodies.lock().unwrap().is_empty(),
            "Should not repay an outstanding borrow of {outstanding}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_repay_coin_skips_when_borrow_query_fails() {
        let (base_url, repay_bodies) = spawn_mock_venue(None).await;
        let client = test_client(base_url);

        repay_coin(&client, Ustr::from("ETH"), Quantity::new(1.0, 3)).await;

        assert!(
            repay_bodies.lock().unwrap().is_empty(),
            "Should not repay when the borrow amount is unknown"
        );
    }

    fn ns_at(minute: u64, second: u64) -> UnixNanos {
        // Anchor at an arbitrary whole hour, then offset within it.
        UnixNanos::from((100 * 3600 + minute * 60 + second) * NANOSECONDS_IN_SECOND)
    }

    #[rstest]
    #[case(4, 0, Some(91))]
    #[case(4, 30, Some(61))]
    #[case(5, 0, Some(31))]
    #[case(5, 29, Some(2))]
    #[case(5, 30, Some(1))]
    fn test_blackout_wait_inside_window(
        #[case] minute: u64,
        #[case] second: u64,
        #[case] expected_secs: Option<u64>,
    ) {
        let wait = repay_blackout_delay(ns_at(minute, second));
        assert_eq!(wait.map(|d| d.as_secs()), expected_secs);
    }

    #[rstest]
    #[case(3, 59)]
    #[case(5, 31)]
    #[case(6, 0)]
    #[case(0, 0)]
    #[case(30, 0)]
    fn test_blackout_wait_outside_window(#[case] minute: u64, #[case] second: u64) {
        assert!(repay_blackout_delay(ns_at(minute, second)).is_none());
    }
}
