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
//! failing on an insufficient debt-coin balance. MNT uses no-convert repayment
//! because Bybit excludes it from converting repayment.

use std::time::Duration;

use anyhow::Context;
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::types::Quantity;
use rust_decimal::{Decimal, RoundingStrategy};
use ustr::Ustr;

use crate::{common::enums::BybitRepayStatus, http::client::BybitHttpClient};

const BLACKOUT_START_SEC: u64 = 4 * 60;
const BLACKOUT_END_SEC: u64 = 5 * 60 + 30;

// Bybit excludes MNT from convert-repay but permits no-convert repayment:
// https://bybit-exchange.github.io/docs/v5/account/repay
const CONVERT_REPAY_UNSUPPORTED_COIN: &str = "MNT";

#[derive(Clone, Debug)]
pub(crate) struct RepayRequest {
    pub(crate) coin: Ustr,
    pub(crate) quantity: Quantity,
    pub(crate) base_fee: Decimal,
    pub(crate) repayment_precision: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RepayOutcome {
    Processing,
    Repaid,
    Skipped,
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

        if let Err(e) = repay_coin(&http_client, req).await {
            log::error!("Failed to repay spot borrow: {e}");
        }
    }

    log::debug!("Spot repay consumer stopped");
}

/// Repays the outstanding borrow for a single coin, capped at `bought`.
async fn repay_coin(
    http_client: &BybitHttpClient,
    request: RepayRequest,
) -> anyhow::Result<RepayOutcome> {
    let RepayRequest {
        coin,
        quantity: bought,
        base_fee,
        repayment_precision,
    } = request;
    let coin_str = coin.as_str();

    let outstanding = http_client
        .get_spot_borrow_amount(coin_str)
        .await
        .with_context(|| format!("failed to query borrow amount for {coin}"))?;

    if outstanding.is_zero() {
        log::debug!("No outstanding borrow for {coin}");
        return Ok(RepayOutcome::Skipped);
    }

    let uses_conversion = coin_str != CONVERT_REPAY_UNSUPPORTED_COIN;
    let available = if uses_conversion {
        bought.as_decimal()
    } else {
        (bought.as_decimal() - base_fee).max(Decimal::ZERO)
    };
    let precision = if uses_conversion {
        bought.precision
    } else {
        repayment_precision
    };
    let repay = outstanding.min(available);
    let repay = repay.round_dp_with_strategy(u32::from(precision), RoundingStrategy::ToZero);
    let repay_qty = Quantity::from_decimal_dp(repay, precision)
        .with_context(|| format!("failed to build repay quantity for {coin} ({repay})"))?;

    if repay_qty.is_zero() {
        return Ok(RepayOutcome::Skipped);
    }

    let status = if uses_conversion {
        http_client
            .repay_spot_borrow_with_conversion(coin_str, Some(repay_qty))
            .await?
            .result
            .result_status
    } else {
        http_client
            .repay_spot_borrow(coin_str, Some(repay_qty))
            .await?
            .result
            .result_status
    };

    match status {
        BybitRepayStatus::Success => {
            log::info!(
                "Repaid {repay_qty} {coin} spot borrow \
                 (outstanding was {outstanding}, bought {bought})"
            );
            Ok(RepayOutcome::Repaid)
        }
        BybitRepayStatus::Processing => {
            log::info!(
                "Repayment of {repay_qty} {coin} spot borrow is processing \
                 (outstanding was {outstanding}, bought {bought})"
            );
            Ok(RepayOutcome::Processing)
        }
        BybitRepayStatus::Failed => {
            anyhow::bail!("Bybit repay for {coin} returned result status {status}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Json, Router,
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
    use parking_lot::Mutex;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde_json::{Value, json};

    use super::*;

    type RepayBodies = Arc<Mutex<Vec<Value>>>;

    #[derive(Clone)]
    struct MockVenue {
        repay_bodies: RepayBodies,
        no_convert_repay_bodies: RepayBodies,
        result_status: &'static str,
    }

    fn wallet_with_borrow(coin: &str, spot_borrow: &str) -> Value {
        let mut wallet: Value =
            serde_json::from_str(include_str!("../test_data/http_get_wallet_balance.json"))
                .expect("valid wallet balance fixture");

        let coins = wallet["result"]["list"][0]["coin"]
            .as_array_mut()
            .expect("fixture has a coin list");
        let entry = if let Some(index) = coins.iter().position(|entry| entry["coin"] == coin) {
            &mut coins[index]
        } else {
            let mut entry = coins.first().expect("fixture has a coin").clone();
            entry["coin"] = json!(coin);
            coins.push(entry);
            coins.last_mut().expect("inserted coin is present")
        };
        entry["spotBorrow"] = json!(spot_borrow);

        wallet
    }

    async fn spawn_mock_venue(
        wallet: Option<Value>,
        result_status: &'static str,
    ) -> (String, RepayBodies, RepayBodies) {
        let state = MockVenue {
            repay_bodies: RepayBodies::default(),
            no_convert_repay_bodies: RepayBodies::default(),
            result_status,
        };
        let repay_bodies = state.repay_bodies.clone();
        let no_convert_repay_bodies = state.no_convert_repay_bodies.clone();

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
            .route(
                "/v5/account/no-convert-repay",
                post(handle_no_convert_repay),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        (
            format!("http://{addr}"),
            repay_bodies,
            no_convert_repay_bodies,
        )
    }

    async fn handle_repay(State(state): State<MockVenue>, body: axum::body::Bytes) -> Response {
        let params: Value = serde_json::from_slice(&body).expect("valid repay body");
        state.repay_bodies.lock().push(params);
        repay_response(state.result_status)
    }

    async fn handle_no_convert_repay(
        State(state): State<MockVenue>,
        body: axum::body::Bytes,
    ) -> Response {
        let params: Value = serde_json::from_slice(&body).expect("valid repay body");
        state.no_convert_repay_bodies.lock().push(params);
        repay_response(state.result_status)
    }

    fn repay_response(result_status: &str) -> Response {
        Json(json!({
            "retCode": 0,
            "retMsg": "OK",
            "result": {"resultStatus": result_status},
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

    fn repay_request(coin: &str, bought: f64, precision: u8) -> RepayRequest {
        RepayRequest {
            coin: Ustr::from(coin),
            quantity: Quantity::new(bought, precision),
            base_fee: Decimal::ZERO,
            repayment_precision: precision,
        }
    }

    fn single_repay_amount(bodies: &RepayBodies, coin: &str) -> Decimal {
        let bodies = bodies.lock();
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
    #[case::truncates_at_precision("0.12356", 1.0, dec!(0.123))]
    #[tokio::test]
    async fn test_repay_coin_repays_lesser_of_outstanding_and_bought(
        #[case] outstanding: &str,
        #[case] bought: f64,
        #[case] expected: Decimal,
    ) {
        let (base_url, repay_bodies, _) =
            spawn_mock_venue(Some(wallet_with_borrow("ETH", outstanding)), "SU").await;
        let client = test_client(base_url);

        let outcome = repay_coin(&client, repay_request("ETH", bought, 3))
            .await
            .unwrap();

        assert_eq!(outcome, RepayOutcome::Repaid);
        assert_eq!(single_repay_amount(&repay_bodies, "ETH"), expected);
    }

    #[rstest]
    #[tokio::test]
    async fn test_repay_coin_uses_no_convert_for_mnt_and_excludes_base_fee() {
        let (base_url, repay_bodies, no_convert_repay_bodies) =
            spawn_mock_venue(Some(wallet_with_borrow("MNT", "1.0")), "SU").await;
        let client = test_client(base_url);
        let request = RepayRequest {
            coin: Ustr::from("MNT"),
            quantity: Quantity::from("1.000"),
            base_fee: dec!(0.0015),
            repayment_precision: 8,
        };

        let outcome = repay_coin(&client, request).await.unwrap();

        assert_eq!(outcome, RepayOutcome::Repaid);
        assert!(repay_bodies.lock().is_empty());
        assert_eq!(
            single_repay_amount(&no_convert_repay_bodies, "MNT"),
            dec!(0.9985)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_repay_coin_reports_processing_status() {
        let (base_url, repay_bodies, _) =
            spawn_mock_venue(Some(wallet_with_borrow("ETH", "1.0")), "P").await;
        let client = test_client(base_url);

        let outcome = repay_coin(&client, repay_request("ETH", 1.0, 3))
            .await
            .unwrap();

        assert_eq!(outcome, RepayOutcome::Processing);
        assert_eq!(single_repay_amount(&repay_bodies, "ETH"), dec!(1.0));
    }

    #[rstest]
    #[case::no_outstanding_borrow("0")]
    #[case::outstanding_below_one_tick("0.000000012")]
    #[tokio::test]
    async fn test_repay_coin_skips_when_nothing_to_repay(#[case] outstanding: &str) {
        let (base_url, repay_bodies, _) =
            spawn_mock_venue(Some(wallet_with_borrow("ETH", outstanding)), "SU").await;
        let client = test_client(base_url);

        let outcome = repay_coin(&client, repay_request("ETH", 1.0, 3))
            .await
            .unwrap();

        assert_eq!(outcome, RepayOutcome::Skipped);
        assert!(
            repay_bodies.lock().is_empty(),
            "Should not repay an outstanding borrow of {outstanding}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_repay_coin_skips_when_borrow_query_fails() {
        let (base_url, repay_bodies, _) = spawn_mock_venue(None, "SU").await;
        let client = test_client(base_url);

        let result = repay_coin(&client, repay_request("ETH", 1.0, 3)).await;

        assert!(result.is_err());
        assert!(
            repay_bodies.lock().is_empty(),
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
