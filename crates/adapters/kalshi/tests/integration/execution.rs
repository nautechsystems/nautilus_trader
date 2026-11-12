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

use std::{cell::RefCell, rc::Rc, time::Duration};

use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    messages::execution::{
        GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GeneratePositionStatusReports, QueryOrder,
    },
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_kalshi::{
    common::consts::KALSHI_VENUE,
    config::KalshiExecutionClientConfig,
    execution::{KalshiExecutionClient, collect_snapshot},
    http::{
        models::{Event, Field, Fill, Market, MarketPosition, Order as VenueOrder},
        parse::parse_instrument,
        query::AccountQuery,
    },
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderAccepted, OrderCanceled, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, Symbol, TradeId, VenueOrderId},
    instruments::{BinaryOption, InstrumentAny},
    orders::{Order, builder::OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use serde_json::Value;

use super::{http, keys, rest};

#[rstest]
#[case(0)]
#[case(7)]
#[case(63)]
#[tokio::test]
async fn snapshot_retains_exact_records_and_original_timestamp(#[case] subaccount: u32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();
        for key in ["orders", "fills"] {
            for record in f[key][key].as_array_mut().unwrap() {
                record["subaccount_number"] = subaccount.into();
            }
        }

        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let collection_observed_at = get_atomic_clock_realtime().get_time_ns();
            let mut requests = Vec::new();
            for body in replies {
                requests.push(rest::reply(&listener, 200, body).await);
            }

            (requests, collection_observed_at)
        });

        let before = get_atomic_clock_realtime().get_time_ns();

        let snapshot = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount,
                exchange_index: 3,
            },
            None,
            2,
        )
        .await
        .unwrap();

        let after = get_atomic_clock_realtime().get_time_ns();
        assert!(before <= snapshot.ts_init && snapshot.ts_init <= after);
        assert_eq!(snapshot.lookback_start, None);
        assert_eq!(
            snapshot.orders,
            serde_json::from_value::<Vec<VenueOrder>>(f["orders"]["orders"].clone()).unwrap()
        );
        assert_eq!(
            snapshot.fills,
            serde_json::from_value::<Vec<Fill>>(f["fills"]["fills"].clone()).unwrap()
        );
        assert_eq!(
            snapshot.positions,
            serde_json::from_value::<Vec<MarketPosition>>(
                f["positions"]["market_positions"].clone()
            )
            .unwrap()
        );
        assert_eq!(snapshot.positions_settled, snapshot.positions);
        assert_eq!(
            snapshot.cutoff,
            serde_json::from_value(f["cutoff"].clone()).unwrap()
        );
        let (requests, collection_observed_at) = peer.await.unwrap();
        assert!(snapshot.ts_init < collection_observed_at);
        assert_eq!(requests.len(), 8);
        let targets: Vec<_> = requests
            .iter()
            .map(|r| r.lines().next().unwrap().split_whitespace().nth(1).unwrap())
            .collect();

        for target in &targets[1..] {
            assert!(
                target.contains(&format!("subaccount={subaccount}")),
                "wrong scope {target}"
            );
        }

        assert!(targets[1].contains("/portfolio/orders?") && !targets[1].contains("min_ts"));
        assert!(targets[2].contains("/historical/orders?") && !targets[2].contains("min_ts"));
        assert!(
            targets[5].contains("exchange_index=3") && !targets[5].contains("settlement_status")
        );
        assert!(targets[6].contains("settlement_status=settled"));
        assert!(
            targets[7].contains("/historical/positions?") && !targets[7].contains("exchange_index")
        );
    })
    .await
    .unwrap();
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(2)]
#[case(3)]
#[case(4)]
#[case(5)]
#[case(6)]
#[case(7)]
#[tokio::test]
async fn every_snapshot_source_failure_is_an_error(#[case] source: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for (index, body) in replies.into_iter().enumerate() {
                rest::reply(&listener, if index == source { 403 } else { 200 }, body).await;
                if index == source {
                    break;
                }
            }
        });

        let e = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            None,
            2,
        )
        .await
        .unwrap_err();

        assert_eq!(e.to_string(), "HTTP 403");
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("orphan", "fill has no authoritative order evidence")]
#[case("market", "fill and order market identities differ")]
#[case("order_shard", "live order shard mismatch")]
#[case("fill_shard", "live fill shard mismatch")]
#[case("fill_identity", "missing fill identity")]
#[case("position_shard", "current position shard mismatch")]
#[case("position_coverage", "missing position history coverage")]
#[case("conflict", "conflicting execution record")]
#[case("budget", "execution snapshot page budget exhausted")]
#[tokio::test]
async fn missing_or_conflicting_authority_fails_closed(
    #[case] scenario: &str,
    #[case] expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();

        let stop = match scenario {
            "orphan" => {
                f["fills"]["fills"][0]["order_id"] = "missing".into();
                4
            }
            "market" => {
                f["fills"]["fills"][0]["market_ticker"] = "wrong".into();
                4
            }
            "order_shard" => {
                f["orders"]["orders"][0]["exchange_index"] = 1.into();
                1
            }
            "fill_shard" => {
                f["fills"]["fills"][0]["exchange_index"] = 1.into();
                3
            }
            "fill_identity" => {
                f["fills"]["fills"][0]["fill_id"] = "".into();
                3
            }
            "position_shard" => {
                f["positions"]["market_positions"][0]["exchange_index"] = 1.into();
                5
            }
            "position_coverage" => {
                f["cutoff"]
                    .as_object_mut()
                    .unwrap()
                    .remove("market_positions_last_updated_ts");
                0
            }
            "conflict" => 2,
            "budget" => {
                f["orders"]["cursor"] = "next".into();
                1
            }
            _ => unreachable!(),
        };

        let mut replies = snapshot_replies(&f);
        if scenario == "conflict" {
            replies[2]["orders"][0]["remaining_count_fp"] = "3.49".into();
        }

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies.into_iter().take(stop + 1) {
                rest::reply(&listener, 200, body).await;
            }
        });

        let e = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            None,
            1,
        )
        .await
        .unwrap_err();

        assert!(e.to_string().ends_with(expected), "{e}");
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn old_open_order_ignores_history_window_and_current_flat_is_retained() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();
        f["fills"] = f["empty_fills"].clone();
        f["positions"]["market_positions"][0]["position_fp"] = "0.00".into();
        let start = UnixNanos::from(1792200000000000000u64);
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let snapshot = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            Some(start),
            1,
        )
        .await
        .unwrap();

        assert_eq!(snapshot.orders.len(), 1);
        assert_eq!(snapshot.orders[0].order_id, "order-a");
        assert_eq!(snapshot.lookback_start, Some(start));
        assert_eq!(snapshot.fills, Vec::new());
        assert_eq!(
            snapshot.positions[0].position_fp.0,
            rust_decimal::Decimal::ZERO
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn archive_filter_preserves_selected_shard_and_scoped_fill_identity() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut replies = snapshot_replies(&f);
        let mut order = f["orders"]["orders"][0].clone();
        order["order_id"] = "other-shard-order".into();
        order["exchange_index"] = 4.into();
        replies[2]["orders"].as_array_mut().unwrap().push(order);
        let mut fill = f["fills"]["fills"][0].clone();
        fill["exchange_index"] = 4.into();
        replies[4]["fills"].as_array_mut().unwrap().push(fill);
        let mut position = f["positions"]["market_positions"][0].clone();
        position["exchange_index"] = 4.into();
        replies[7]["market_positions"]
            .as_array_mut()
            .unwrap()
            .push(position);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let snapshot = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            None,
            1,
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(snapshot.orders).unwrap(),
            f["orders"]["orders"]
        );
        assert_eq!(
            serde_json::to_value(snapshot.fills).unwrap(),
            f["fills"]["fills"]
        );
        assert_eq!(
            serde_json::to_value(snapshot.positions_settled).unwrap(),
            f["positions"]["market_positions"]
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn historical_fill_keeps_its_closed_order_outside_the_order_window() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();
        f["orders"]["orders"][0]["status"] = "canceled".into();
        f["orders"]["orders"][0]["last_update_time"] = "2026-10-15T00:00:00Z".into();
        let expected_order = f["orders"]["orders"][0].clone();
        let mut unrelated = expected_order.clone();
        unrelated["order_id"] = "old-unreferenced".into();
        f["orders"]["orders"]
            .as_array_mut()
            .unwrap()
            .push(unrelated);
        let start = nautilus_core::datetime::try_datetime_to_unix_nanos(
            "2026-10-16T00:00:00Z".parse().unwrap(),
        )
        .unwrap();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let snapshot = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            Some(start),
            1,
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(snapshot.orders).unwrap(),
            Value::Array(vec![expected_order])
        );
        assert_eq!(
            serde_json::to_value(snapshot.fills).unwrap(),
            f["fills"]["fills"]
        );
        assert_eq!(snapshot.lookback_start, Some(start));
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(0, None)]
#[case(1, Some(UnixNanos::from(u64::MAX)))]
#[tokio::test]
async fn invalid_snapshot_window_fails_before_any_request(
    #[case] max_pages: u32,
    #[case] start: Option<UnixNanos>,
) {
    let (_listener, address) = http::bind_peer(0).await;

    let e = collect_snapshot(
        &rest::test_client(address),
        &AccountQuery {
            subaccount: 0,
            exchange_index: 3,
        },
        start,
        max_pages,
    )
    .await
    .unwrap_err();

    assert_eq!(
        e.to_string(),
        "invalid request: invalid execution snapshot window or page budget"
    );
}

#[rstest]
#[case("client_only")]
#[case("venue_only")]
#[case("both")]
#[case("missing")]
#[case("mismatch")]
#[case("unknown_metadata")]
#[case("external_unfilled")]
#[case("ambiguous")]
#[case("archive_only")]
#[case("cached_client_only")]
#[case("cached_both")]
#[tokio::test]
async fn targeted_report_distinguishes_absence_mismatch_and_unknown_authority(
    #[case] mode: &'static str,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        if mode == "external_unfilled" {
            f["orders"]["orders"][0]["fill_count_fp"] = "0.00".into();
        }

        if matches!(mode, "cached_client_only" | "cached_both") {
            f["orders"]["orders"][0]["client_order_id"] = "".into();
        }

        if mode == "ambiguous" {
            let mut other = f["orders"]["orders"][0].clone();
            other["order_id"] = "order-b".into();
            f["orders"]["orders"].as_array_mut().unwrap().push(other);
        }

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            rest::reply(
                &listener,
                200,
                f[if mode == "archive_only" {
                    "empty_orders"
                } else {
                    "orders"
                }]
                .clone(),
            )
            .await;

            rest::reply(&listener, 200, f["orders"].clone()).await;
        });

        let (client, instrument, _) = report_client(
            address,
            !matches!(mode, "unknown_metadata" | "external_unfilled"),
            true,
        );

        let client_order_id = match mode {
            "venue_only" => None,
            "missing" => Some(ClientOrderId::from("not-found")),
            "mismatch" => Some(ClientOrderId::from("wrong-client")),
            _ => Some(ClientOrderId::from("client-a")),
        };

        let venue_order_id = match mode {
            "client_only" | "cached_client_only" | "missing" => None,
            _ => Some(VenueOrderId::from("order-a")),
        };

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            client_order_id,
            venue_order_id,
            None,
            None,
        );

        let result = client.generate_order_status_report(&command).await;

        match mode {
            "missing" => assert_eq!(result.unwrap(), None),
            "ambiguous" => assert!(result.unwrap_err().to_string().contains("ambiguous")),
            "mismatch" => assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("identity mismatch")
            ),
            "unknown_metadata" | "external_unfilled" => assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("not authoritative")
            ),
            _ => {
                let report = result.unwrap().unwrap();
                assert_eq!(report.venue_order_id, VenueOrderId::from("order-a"));
                assert_eq!(
                    report.client_order_id,
                    Some(ClientOrderId::from("client-a"))
                );
                assert_eq!(
                    report.quantity.as_decimal(),
                    "5.75".parse::<rust_decimal::Decimal>().unwrap()
                );
                assert!(report.post_only);
            }
        }

        assert!(!client.is_connected());
        assert!(!client.provides_bulk_position_coverage(instrument.id));
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn report_collection_does_not_enable_execution_or_synthetic_fees() {
    let (_listener, address) = http::bind_peer(0).await;
    let (mut client, instrument, _) = report_client(address, false, true);
    assert!(
        client
            .connect()
            .await
            .unwrap_err()
            .to_string()
            .contains("bootstrap")
    );
    assert!(!client.is_connected());
    assert!(!client.settles_contract_expirations());
    let error = client
        .calculate_commission(
            &InstrumentAny::BinaryOption(instrument),
            Quantity::from_decimal_dp("2.75".parse().unwrap(), 2).unwrap(),
            Price::from_decimal_dp("0.123401".parse().unwrap(), 6).unwrap(),
            LiquiditySide::Taker,
        )
        .unwrap_err();
    assert!(error.to_string().contains("authoritative fill fee"));
    client.disconnect().await.unwrap();
    client.stop().unwrap();
    client.reset().unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn mass_report_keeps_one_window_and_rejects_missing_context(#[case] known: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["fills"] = f["empty_fills"].clone();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let observed = get_atomic_clock_realtime().get_time_ns();

            for body in replies {
                rest::reply(&listener, 200, body).await;
            }

            observed
        });

        let (client, instrument, _) = report_client(address, known, true);
        let result = client.generate_mass_status(Some(60)).await;
        let observed = peer.await.unwrap();

        if known {
            let mass = result.unwrap().unwrap();
            assert_eq!(
                mass.lookback_start(),
                Some(
                    mass.ts_init
                        .saturating_sub(nautilus_core::DurationNanos::try_from_mins(60).unwrap())
                )
            );
            assert!(mass.reports_complete());
            assert_eq!(mass.order_reports().len(), 1);
            assert_eq!(mass.fill_reports().len(), 0);
            assert_eq!(
                mass.position_reports()[&instrument.id][0].signed_decimal_qty,
                "-2.75".parse::<rust_decimal::Decimal>().unwrap()
            );
            assert!(mass.ts_init < observed);
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("lacks command metadata")
            );
        }

        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unbounded_mass_report_cannot_claim_complete_external_recovery() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["fills"] = f["empty_fills"].clone();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, _, _) = report_client(address, false, true);
        assert!(
            client
                .generate_mass_status(None)
                .await
                .unwrap_err()
                .to_string()
                .contains("lacks command metadata")
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case::bulk(false, false, false, false)]
#[case::bulk_mixed(false, false, true, false)]
#[case::bulk_canceled(false, false, false, true)]
#[case::mass_bounded(true, true, false, false)]
#[case::mass_unbounded(true, false, false, false)]
#[case::mass_mixed(true, false, true, false)]
#[case::mass_canceled(true, false, false, true)]
#[tokio::test]
async fn aggregate_reports_omit_unfilled_external_orders(
    #[case] mass: bool,
    #[case] bounded: bool,
    #[case] mixed: bool,
    #[case] canceled: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let mut external = f["orders"]["orders"][0].clone();
        external["order_id"] = "external-order".into();
        external["client_order_id"] = "external-client".into();
        external["fill_count_fp"] = "0.00".into();
        if canceled {
            external["status"] = "canceled".into();
            external["remaining_count_fp"] = "0.00".into();
        }

        let mut sources = Vec::new();
        if mixed {
            sources.push(f["orders"]["orders"][0].clone());
        }

        sources.push(external);
        f["orders"]["orders"] = sources.into();
        f["positions"] = f["empty_positions"].clone();
        if !mixed {
            f["fills"] = f["empty_fills"].clone();
        }

        let mut replies = if mass {
            snapshot_replies(&f)
        } else {
            vec![
                f["orders"].clone(),
                f["orders"].clone(),
                f["cutoff"].clone(),
            ]
        };

        replies.extend([f["empty_fills"].clone(), f["empty_fills"].clone()]);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let mut requests = Vec::new();
            for body in replies {
                requests.push(rest::reply(&listener, 200, body).await);
            }

            requests
        });

        let (client, _, cache) = report_client(address, mixed, true);
        let reports = if mass {
            let result = client
                .generate_mass_status(bounded.then_some(60))
                .await
                .unwrap()
                .unwrap();
            assert!(!result.reports_complete());
            assert_eq!(result.lookback_start().is_some(), bounded);
            assert_eq!(result.fill_reports().len(), usize::from(mixed));
            assert!(result.position_reports().is_empty());
            result.order_reports().into_values().collect::<Vec<_>>()
        } else {
            client
                .generate_order_status_reports(&GenerateOrderStatusReports::new(
                    UUID4::new(),
                    UnixNanos::from(1),
                    !canceled,
                    None,
                    None,
                    None,
                    None,
                    None,
                ))
                .await
                .unwrap()
        };

        let requests = peer.await.unwrap();
        let fills = &requests[requests.len() - 2..];
        assert!(fills[0].contains("order_id=external-order"));
        assert!(fills[0].contains("exchange_index=3") && fills[0].contains("subaccount=0"));
        assert!(fills[1].contains("/historical/fills?") && !fills[1].contains("min_ts"));
        assert_eq!(reports.len(), usize::from(mixed));

        if mixed {
            assert_eq!(reports[0].venue_order_id, VenueOrderId::from("order-a"));
            assert_eq!(
                reports[0].client_order_id,
                Some(ClientOrderId::from("client-a"))
            );
            assert_eq!(reports[0].filled_qty.as_decimal(), "2.75".parse().unwrap());
            assert_eq!(reports[0].quantity.as_decimal(), "5.75".parse().unwrap());
            assert_eq!(
                reports[0].price.unwrap().as_decimal(),
                "0.123401".parse().unwrap()
            );
            assert_eq!(reports[0].time_in_force, TimeInForce::Gtc);
            assert!(reports[0].post_only);
            assert!(!reports[0].reduce_only);
        }

        assert_eq!(
            cache.borrow().orders(None, None, None, None, None).len(),
            usize::from(mixed)
        );
        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[rstest]
#[case::filled("filled", "filled quantity")]
#[case::zero_leaves("zero_leaves", "remaining quantity")]
#[case::unknown_status("status", "unknown Kalshi order status")]
#[case::negative_count("negative", "negative")]
#[case::bad_prices("prices", "complementary")]
#[case::bad_expiration("expiration", "timestamp")]
#[case::cached_terminal("cached", "canceled total")]
#[case::cached_filled("cached_filled", "canceled total")]
#[case::binding_filled(
    "binding_filled",
    "order command metadata or canceled total is not authoritative"
)]
#[case::cached_binding("binding", "command metadata")]
#[tokio::test]
async fn aggregate_reports_reject_unproven_external_omissions(
    #[case] kind: &str,
    #[case] expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let source = &mut f["orders"]["orders"][0];
        source["fill_count_fp"] = "0.00".into();
        match kind {
            "filled" => source["fill_count_fp"] = "1.00".into(),
            "zero_leaves" => source["remaining_count_fp"] = "0.00".into(),
            "status" => source["status"] = "unknown".into(),
            "negative" => source["remaining_count_fp"] = "-1.00".into(),
            "prices" => source["no_price_dollars"] = "0.100000".into(),
            "expiration" => source["expiration_time"] = "1960-01-01T00:00:00Z".into(),
            "cached" => source["status"] = "canceled".into(),
            "cached_filled" | "binding_filled" => {
                source["status"] = "canceled".into();
                source["fill_count_fp"] = "0.25".into();
                source["remaining_count_fp"] = "0.00".into();
            }
            "binding" => {}
            _ => unreachable!(),
        }

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for _ in 0..2 {
                rest::reply(&listener, 200, f["orders"].clone()).await;
            }
        });

        let (client, _, cache) =
            report_client(address, matches!(kind, "cached" | "cached_filled"), true);

        if matches!(kind, "binding" | "binding_filled") {
            cache
                .borrow_mut()
                .add_venue_order_id(
                    &ClientOrderId::from("client-a"),
                    &VenueOrderId::from("order-a"),
                    false,
                )
                .unwrap();
        }

        let error = client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err();
        peer.await.unwrap();
        assert!(error.to_string().contains(expected), "{error:#}");
        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[rstest]
#[case::cached_client_only(false, true)]
#[case::purged_private_client(true, true)]
#[case::purged_private_venue(true, false)]
#[tokio::test]
async fn aggregate_reports_keep_authority_without_cache_bindings(
    #[case] purged: bool,
    #[case] wire_client: bool,
    #[values(false, true)] mass: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let source = &mut f["orders"]["orders"][0];
        source["status"] = "canceled".into();
        source["fill_count_fp"] = "0.00".into();
        source["remaining_count_fp"] = "0.00".into();
        if !wire_client {
            source["client_order_id"] = "".into();
        }

        f["fills"] = f["empty_fills"].clone();
        f["positions"] = f["empty_positions"].clone();
        let replies = if mass {
            snapshot_replies(&f)
        } else {
            vec![f["orders"].clone(), f["orders"].clone()]
        };

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, cache) = report_client(address, true, true);
        let client_order_id = ClientOrderId::from("client-a");
        let venue_order_id = VenueOrderId::from("order-a");
        let mut order = cache.borrow().order_owned(&client_order_id).unwrap();
        cache
            .borrow_mut()
            .purge_order(client_order_id, false)
            .unwrap();

        if purged {
            order
                .apply(OrderEventAny::Accepted(OrderAccepted::new(
                    order.trader_id(),
                    order.strategy_id(),
                    instrument.id,
                    client_order_id,
                    venue_order_id,
                    client.account_id(),
                    UUID4::new(),
                    UnixNanos::from(2),
                    UnixNanos::from(3),
                    false,
                )))
                .unwrap();
            order
                .apply(OrderEventAny::Canceled(OrderCanceled::new(
                    order.trader_id(),
                    order.strategy_id(),
                    instrument.id,
                    client_order_id,
                    UUID4::new(),
                    UnixNanos::from(4),
                    UnixNanos::from(5),
                    false,
                    Some(venue_order_id),
                    Some(client.account_id()),
                    None,
                )))
                .unwrap();
        }

        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client.client_id()), false)
            .unwrap();

        if purged {
            cache
                .borrow_mut()
                .add_venue_order_id(&client_order_id, &venue_order_id, false)
                .unwrap();

            let query = QueryOrder::new(
                order.trader_id(),
                Some(client.client_id()),
                order.strategy_id(),
                instrument.id,
                client_order_id,
                Some(venue_order_id),
                UUID4::new(),
                UnixNanos::from(6),
                None,
                None,
            );

            let error = client.query_order(query).unwrap_err();
            assert_eq!(error.to_string(), "Kalshi account bootstrap is not active");
            cache
                .borrow_mut()
                .purge_order(client_order_id, false)
                .unwrap();
        }

        assert_eq!(
            cache.borrow().order_owned(&client_order_id).is_none(),
            purged
        );
        assert_eq!(cache.borrow().venue_order_id(&client_order_id), None);
        assert_eq!(cache.borrow().client_order_id(&venue_order_id), None);

        let command = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::from(7),
            false,
            None,
            None,
            None,
            None,
            None,
        );

        let error = if mass {
            client.generate_mass_status(None).await.unwrap_err()
        } else {
            client
                .generate_order_status_reports(&command)
                .await
                .unwrap_err()
        };

        peer.await.unwrap();
        assert_eq!(
            error.to_string(),
            "order command metadata or canceled total is not authoritative"
        );
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn external_omissions_check_archived_fills_outside_report_window(#[case] mass: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["orders"]["orders"][0]["fill_count_fp"] = "0.00".into();
        let archived = f["fills"].clone();
        f["fills"] = f["empty_fills"].clone();
        f["positions"] = f["empty_positions"].clone();
        let mut replies = if mass {
            snapshot_replies(&f)
        } else {
            vec![
                f["orders"].clone(),
                f["orders"].clone(),
                f["cutoff"].clone(),
            ]
        };

        replies.extend([f["empty_fills"].clone(), archived]);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let mut last = String::new();
            for body in replies {
                last = rest::reply(&listener, 200, body).await;
            }

            last
        });

        let (client, _, _) = report_client(address, false, true);
        let error = if mass {
            client.generate_mass_status(Some(60)).await.unwrap_err()
        } else {
            client
                .generate_order_status_reports(&GenerateOrderStatusReports::new(
                    UUID4::new(),
                    UnixNanos::from(1),
                    true,
                    None,
                    Some(get_atomic_clock_realtime().get_time_ns()),
                    None,
                    None,
                    None,
                ))
                .await
                .unwrap_err()
        };

        let request = peer.await.unwrap();
        assert!(!request.contains("min_ts"));
        assert!(error.to_string().contains("collected fills"), "{error:#}");
        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[rstest]
#[case::bulk_live(false, false)]
#[case::bulk_archive(false, true)]
#[case::mass_live(true, false)]
#[case::mass_archive(true, true)]
#[tokio::test]
async fn external_omissions_require_complete_fill_evidence(
    #[case] mass: bool,
    #[case] archive: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["orders"]["orders"][0]["fill_count_fp"] = "0.00".into();
        f["fills"] = f["empty_fills"].clone();
        f["positions"] = f["empty_positions"].clone();
        let mut replies = if mass {
            snapshot_replies(&f)
        } else {
            vec![
                f["orders"].clone(),
                f["orders"].clone(),
                f["cutoff"].clone(),
            ]
        };

        if archive {
            replies.push(f["empty_fills"].clone());
        }

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }

            rest::reply(&listener, 403, f["empty_fills"].clone()).await
        });

        let (client, _, cache) = report_client(address, false, true);
        let error = if mass {
            client.generate_mass_status(Some(60)).await.unwrap_err()
        } else {
            client
                .generate_order_status_reports(&GenerateOrderStatusReports::new(
                    UUID4::new(),
                    UnixNanos::from(1),
                    true,
                    None,
                    None,
                    None,
                    None,
                    None,
                ))
                .await
                .unwrap_err()
        };

        let request = peer.await.unwrap();
        assert!(request.contains(if archive {
            "/historical/fills?"
        } else {
            "/portfolio/fills?"
        }));
        assert!(error.to_string().contains("403"), "{error:#}");
        assert!(
            cache
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        assert!(!client.is_connected());
    })
    .await
    .unwrap();
}

#[rstest]
#[case("order")]
#[case("position")]
#[tokio::test]
async fn mass_report_rejects_changes_after_its_fixed_fill_window(#[case] kind: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["fills"] = f["empty_fills"].clone();
        let future = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(60))
            .unwrap()
            .to_string();

        if kind == "order" {
            f["orders"]["orders"][0]["last_update_time"] = future.into();
        } else {
            f["positions"]["market_positions"][0]["last_updated_ts"] = future.into();
        }

        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, _, _) = report_client(address, true, true);
        let error = client.generate_mass_status(Some(60)).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("{kind} changed after mass collection began"))
        );
        assert!(!client.is_connected());
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn bulk_order_filters_keep_open_orders_without_history_bounds() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = report_fixtures();
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for _ in 0..3 {
                rest::reply(&listener, 200, f["orders"].clone()).await;
                rest::reply(&listener, 200, f["orders"].clone()).await;
            }
        });

        let (client, instrument, _) = report_client(address, true, true);
        let start = Some(get_atomic_clock_realtime().get_time_ns());
        let report = client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                true,
                Some(instrument.id),
                start,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].venue_order_id, VenueOrderId::from("order-a"));
        let mut foreign = instrument.id;
        foreign.symbol = Symbol::from("ANOTHER-MARKET");
        let empty = client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                Some(foreign),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(empty, Vec::new());
        let report = client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                None,
                None,
                Some(UnixNanos::from(2)),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(report.len(), 1);
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn fill_command_filters_and_exact_fee_survive_http_boundary() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = report_fixtures();
        let replies = snapshot_replies(&f).into_iter().take(5).collect::<Vec<_>>();
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;

            for _ in 0..3 {
                for body in &replies {
                    rest::reply(&listener, 200, body.clone()).await;
                }
            }
        });

        let (client, instrument, _) = report_client(address, true, true);

        let mut command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            Some(VenueOrderId::from("order-a")),
            None,
            None,
            None,
            None,
        );

        let fills = client.generate_fill_reports(command.clone()).await.unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, TradeId::from("fill-a"));
        assert_eq!(
            fills[0].commission.as_decimal(),
            "0.000001".parse::<rust_decimal::Decimal>().unwrap()
        );
        command.venue_order_id = Some(VenueOrderId::from("other-order"));
        assert_eq!(
            client.generate_fill_reports(command.clone()).await.unwrap(),
            Vec::new()
        );
        command.venue_order_id = None;
        command.end = Some(UnixNanos::from(1));
        assert_eq!(
            client.generate_fill_reports(command).await.unwrap(),
            Vec::new()
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn position_filters_do_not_claim_omissions_are_flat() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = report_fixtures();
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for _ in 0..2 {
                rest::reply(&listener, 200, f["positions"].clone()).await;
            }
        });

        let (client, instrument, _) = report_client(address, false, true);

        let mut command = GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            None,
            None,
        );

        let positions = client
            .generate_position_status_reports(&command)
            .await
            .unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions[0].signed_decimal_qty,
            "-2.75".parse::<rust_decimal::Decimal>().unwrap()
        );
        command.start = Some(get_atomic_clock_realtime().get_time_ns());
        assert_eq!(
            client
                .generate_position_status_reports(&command)
                .await
                .unwrap(),
            Vec::new()
        );
        assert!(!client.provides_bulk_position_coverage(instrument.id));
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("live")]
#[case("archive")]
#[case("market_failure")]
#[case("archive_failure")]
#[case("event_failure")]
#[case("wrong_shard")]
#[tokio::test]
async fn unloaded_report_definitions_resolve_or_fail_closed(#[case] mode: &'static str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let definitions: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        f["market"]["market"] = definitions["definitions"]["live"]["market"].clone();
        f["event"]["event"] = definitions["definitions"]["live"]["event"].clone();

        let shard = if mode == "wrong_shard" { 4 } else { 3 };
        f["market"]["market"]["exchange_index"] = shard.into();
        f["event"]["event"]["exchange_index"] = shard.into();
        f["series"]["series"]["ticker"] = f["event"]["event"]["series_ticker"].clone();
        let replies = snapshot_replies(&f).into_iter().take(5);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }

            let archived = matches!(mode, "archive" | "archive_failure");

            let status = match mode {
                "archive" | "archive_failure" => 404,
                "market_failure" => 403,
                _ => 200,
            };

            let request = rest::reply(&listener, status, f["market"].clone()).await;
            assert!(request.starts_with("GET /custom/v3/markets/"));

            if mode == "market_failure" {
                return;
            }

            if archived {
                let status = if mode == "archive_failure" { 403 } else { 200 };
                let request = rest::reply(&listener, status, f["market"].clone()).await;
                assert!(request.starts_with("GET /custom/v3/historical/markets/"));
            }

            if mode == "archive_failure" {
                return;
            }

            let status = if mode == "event_failure" { 403 } else { 200 };
            let request = rest::reply(&listener, status, f["event"].clone()).await;
            assert!(request.starts_with("GET /custom/v3/events/"));

            if mode == "event_failure" {
                return;
            }

            let request = rest::reply(&listener, 200, f["series"].clone()).await;
            assert!(request.starts_with("GET /custom/v3/series/"));
        });

        let (client, instrument, _) = report_client(address, false, false);

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            None,
            None,
            None,
        );

        let result = client.generate_fill_reports(command).await;
        if matches!(mode, "live" | "archive") {
            let reports = result.unwrap();
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].instrument_id, instrument.id);
            assert_eq!(
                reports[0].commission.as_decimal(),
                "0.000001".parse::<rust_decimal::Decimal>().unwrap()
            );
        } else {
            assert!(result.is_err());
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("commission")]
#[case("position")]
#[tokio::test]
async fn malformed_report_economics_fail_the_request(#[case] kind: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let replies = if kind == "commission" {
            f["fills"]["fills"][0]["fee_cost"] = rust_decimal::Decimal::MAX.to_string().into();
            snapshot_replies(&f).into_iter().take(5).collect::<Vec<_>>()
        } else {
            f["positions"]["market_positions"][0]["position_fp"] = "-2.751".into();
            vec![f["positions"].clone()]
        };

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, _) = report_client(address, false, true);

        if kind == "commission" {
            let command = GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                Some(instrument.id),
                None,
                None,
                None,
                None,
                None,
            );

            assert!(client.generate_fill_reports(command).await.is_err());
        } else {
            let command = GeneratePositionStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                Some(instrument.id),
                None,
                None,
                None,
                None,
            );

            assert!(
                client
                    .generate_position_status_reports(&command)
                    .await
                    .is_err()
            );
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn fill_query_widens_seconds_and_filters_exact_nanoseconds() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();
        let source = f["fills"]["fills"][0].clone();
        let time: jiff::Timestamp = source["created_time"].as_str().unwrap().parse().unwrap();
        let start = nautilus_core::datetime::try_datetime_to_unix_nanos(time).unwrap();
        let mut before = source.clone();
        before["fill_id"] = "before-window".into();
        before["created_time"] = time
            .checked_sub(jiff::SignedDuration::from_nanos(1))
            .unwrap()
            .to_string()
            .into();
        let mut after = source.clone();
        after["fill_id"] = "after-window".into();
        after["created_time"] = "2099-01-01T00:00:00Z".into();
        f["fills"]["fills"] = vec![before, source, after].into();
        let cutoff: jiff::Timestamp = f["cutoff"]["trades_created_ts"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let mut requests = Vec::new();
            for body in replies {
                requests.push(rest::reply(&listener, 200, body).await);
            }

            requests
        });

        let snapshot = collect_snapshot(
            &rest::test_client(address),
            &AccountQuery {
                subaccount: 0,
                exchange_index: 3,
            },
            Some(start),
            2,
        )
        .await
        .unwrap();

        assert_eq!(snapshot.fills.len(), 1);
        assert_eq!(snapshot.fills[0].fill_id, "fill-a");
        let requests = peer.await.unwrap();
        let query = |index: usize| {
            let target = requests[index]
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            url::Url::parse(&format!("http://localhost{target}"))
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect::<std::collections::HashMap<_, _>>()
        };

        let live = query(3);
        let archive = query(4);
        assert_eq!(
            live["min_ts"],
            (time.as_second().max(cutoff.as_second()) - 1).to_string()
        );
        assert_eq!(archive["min_ts"], (time.as_second() - 1).to_string());
        assert_eq!(
            live["max_ts"],
            (snapshot.ts_init.as_u64() / 1_000_000_000 + 1).to_string()
        );
        assert_eq!(archive["max_ts"], live["max_ts"]);
    })
    .await
    .unwrap();
}

#[rstest]
#[case(None, false)]
#[case(None, true)]
#[case(Some(60), false)]
#[case(Some(60), true)]
#[tokio::test]
async fn mass_report_checks_fill_coverage_and_ignores_unused_settled_definitions(
    #[case] lookback: Option<u64>,
    #[case] delayed: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();

        if lookback.is_some() {
            let time = jiff::Timestamp::now()
                .checked_sub(jiff::SignedDuration::from_secs(60))
                .unwrap()
                .to_string();
            f["orders"]["orders"][0]["created_time"] = time.clone().into();
            f["orders"]["orders"][0]["last_update_time"] = time.clone().into();
            f["fills"]["fills"][0]["created_time"] = time.into();
        }

        if delayed {
            f["fills"] = f["empty_fills"].clone();
        }

        let mut replies = snapshot_replies(&f);
        for body in &mut replies[6..] {
            body["market_positions"][0]["ticker"] = "UNSUPPORTED-OLD-MARKET".into();
        }

        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, _) = report_client(address, true, true);
        let result = client.generate_mass_status(lookback).await;

        if delayed {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("fills are not yet completely visible")
            );
        } else {
            let mass = result.unwrap().unwrap();
            assert!(mass.reports_complete());
            assert_eq!(mass.order_reports().len(), 1);
            let fills = &mass.fill_reports()[&VenueOrderId::from("order-a")];
            assert_eq!(fills.len(), 1);
            assert_eq!(fills[0].trade_id, TradeId::from("fill-a"));
            assert_eq!(
                fills[0].commission.as_decimal(),
                "0.000001".parse::<rust_decimal::Decimal>().unwrap()
            );
            assert_eq!(mass.position_reports().len(), 1);
            assert_eq!(
                mass.position_reports()[&instrument.id][0].signed_decimal_qty,
                "-2.75".parse::<rust_decimal::Decimal>().unwrap()
            );
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn current_positions_accept_event_only_pages_without_reading_history() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = report_fixtures();
        let mut first = f["positions"].clone();
        first["market_positions"] = Vec::<Value>::new().into();
        first["cursor"] = "next-position-page".into();
        let (listener, address) = http::bind_peer(0).await;
        let (completed, completion) = tokio::sync::oneshot::channel();

        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            let a = rest::reply(&listener, 200, first).await;
            let b = rest::reply(&listener, 200, f["positions"].clone()).await;
            assert!(a.starts_with("GET /custom/v3/portfolio/positions?"));
            assert!(!a.contains("settlement_status"));
            assert!(b.contains("cursor=next-position-page"));
            http::assert_no_connection(&listener, address, async {
                completion.await.unwrap();
            })
            .await;
        });

        let (client, instrument, _) = report_client(address, false, true);

        let command = GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            None,
            None,
        );

        let reports = client
            .generate_position_status_reports(&command)
            .await
            .unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].signed_decimal_qty,
            "-2.75".parse::<rust_decimal::Decimal>().unwrap()
        );
        completed.send(()).unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn mass_report_rejects_individually_valid_fills_exceeding_the_order_total() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let mut first = f["fills"]["fills"][0].clone();
        first["count_fp"] = "1.75".into();
        let mut second = first.clone();
        second["fill_id"] = "fill-b".into();
        second["trade_id"] = "trade-b".into();
        f["fills"]["fills"] = vec![first, second].into();
        let replies = snapshot_replies(&f);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, _, _) = report_client(address, true, true);
        let error = client.generate_mass_status(None).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "collected fills exceed order filled count"
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn fill_filters_precede_definition_loading_and_cached_identity_is_retained(
    #[case] excluded: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["orders"]["orders"][0]["client_order_id"] = "".into();
        let replies = snapshot_replies(&f).into_iter().take(5);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, _) = report_client(address, true, !excluded);
        let end = excluded.then_some(UnixNanos::from(1));

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            end,
            None,
            None,
        );

        let reports = client.generate_fill_reports(command).await.unwrap();

        if excluded {
            assert_eq!(reports, Vec::new());
        } else {
            assert_eq!(reports.len(), 1);
            assert_eq!(
                reports[0].client_order_id,
                Some(ClientOrderId::from("client-a"))
            );
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn closed_order_reports_apply_both_time_bounds() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        f["orders"]["orders"][0]["status"] = "executed".into();
        f["orders"]["orders"][0]["remaining_count_fp"] = "0.00".into();
        let last: jiff::Timestamp = f["orders"]["orders"][0]["last_update_time"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let last = nautilus_core::datetime::try_datetime_to_unix_nanos(last).unwrap();
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;

            for _ in 0..3 {
                for _ in 0..2 {
                    rest::reply(&listener, 200, f["orders"].clone()).await;
                }
            }
        });

        let (client, instrument, _) = report_client(address, true, true);

        for (start, end, count) in [
            (Some(last), Some(last), 1),
            (Some(UnixNanos::from(last.as_u64() + 1)), None, 0),
            (None, Some(UnixNanos::from(last.as_u64() - 1)), 0),
        ] {
            let command = GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::from(1),
                false,
                Some(instrument.id),
                start,
                end,
                None,
                None,
            );

            let reports = client
                .generate_order_status_reports(&command)
                .await
                .unwrap();
            assert_eq!(reports.len(), count);

            if count == 1 {
                assert_eq!(reports[0].order_status, OrderStatus::Filled);
            }
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("instrument")]
#[case("side")]
#[case("account")]
#[case("venue")]
#[case("mapping")]
#[tokio::test]
async fn supplied_fill_client_identity_must_agree_with_retained_evidence(#[case] kind: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = report_fixtures();
        let replies = snapshot_replies(&f).into_iter().take(5);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, cache) = report_client(address, false, true);

        let id = if kind == "instrument" {
            "ANOTHER-MARKET.KALSHI".into()
        } else {
            instrument.id
        };

        let side = if kind == "side" {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let client_id = if kind == "mapping" {
            ClientOrderId::from("client-b")
        } else {
            ClientOrderId::from("client-a")
        };

        let account = if kind == "account" {
            AccountId::from("KALSHI-OTHER")
        } else {
            client.account_id()
        };

        let venue = if kind == "venue" {
            VenueOrderId::from("another-order")
        } else {
            VenueOrderId::from("order-a")
        };

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(id)
            .client_order_id(client_id)
            .side(side)
            .quantity(Quantity::from_decimal_dp("5.75".parse().unwrap(), 2).unwrap())
            .price(Price::from_decimal_dp("0.123401".parse().unwrap(), 6).unwrap())
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                id,
                client_id,
                venue,
                account,
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(3),
                false,
            )))
            .unwrap();
        cache
            .borrow_mut()
            .add_order(order, None, Some(client.client_id()), false)
            .unwrap();
        cache
            .borrow_mut()
            .add_venue_order_id(&client_id, &venue, false)
            .unwrap();

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            None,
            None,
            None,
        );

        let error = client.generate_fill_reports(command).await.unwrap_err();

        let expected = match kind {
            "mapping" => "cached client order identity mismatch",
            "venue" => "cached venue order identity mismatch",
            _ => "cached fill identity mismatch",
        };

        assert_eq!(error.to_string(), expected);
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("excess")]
#[case("missing")]
#[case("historical_end")]
#[case("late_update")]
#[tokio::test]
async fn standalone_fills_enforce_coverage_only_for_the_requested_window(#[case] kind: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = report_fixtures();
        let event: jiff::Timestamp = f["fills"]["fills"][0]["created_time"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();

        let end = if kind == "historical_end" {
            Some(nautilus_core::datetime::try_datetime_to_unix_nanos(event).unwrap())
        } else {
            None
        };

        if matches!(kind, "missing" | "late_update") {
            f["fills"] = f["empty_fills"].clone();
        }

        if kind == "late_update" {
            f["orders"]["orders"][0]["last_update_time"] = jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_secs(60))
                .unwrap()
                .to_string()
                .into();
        }

        if kind == "excess" {
            let mut first = f["fills"]["fills"][0].clone();
            first["count_fp"] = "1.75".into();
            let mut second = first.clone();
            second["fill_id"] = "fill-b".into();
            second["trade_id"] = "trade-b".into();
            f["fills"]["fills"] = vec![first, second].into();
        }

        if kind == "historical_end" {
            f["fills"]["fills"][0]["count_fp"] = "1.25".into();
        }

        let replies = snapshot_replies(&f).into_iter().take(5);
        let (listener, address) = http::bind_peer(0).await;
        let peer = tokio::spawn(async move {
            rest::quota_discovery(&listener).await;
            for body in replies {
                rest::reply(&listener, 200, body).await;
            }
        });

        let (client, instrument, _) = report_client(address, true, true);

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(1),
            Some(instrument.id),
            None,
            None,
            end,
            None,
            None,
        );

        let result = client.generate_fill_reports(command).await;

        match kind {
            "excess" => assert_eq!(
                result.unwrap_err().to_string(),
                "collected fills exceed order filled count"
            ),
            "missing" | "late_update" => assert_eq!(
                result.unwrap_err().to_string(),
                "order fills are not yet completely visible"
            ),
            _ => {
                let fills = result.unwrap();
                assert_eq!(fills.len(), 1);
                assert_eq!(
                    fills[0].last_qty.as_decimal(),
                    "1.25".parse::<rust_decimal::Decimal>().unwrap()
                );
            }
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

fn report_fixtures() -> Value {
    let mut f = fixtures();
    let instrument = report_instrument();
    let ticker = instrument.id.symbol.as_str();

    for name in ["order", "orders"] {
        let raw = if name == "order" {
            &mut f[name]["order"]
        } else {
            &mut f[name]["orders"][0]
        };

        raw["ticker"] = ticker.into();
        raw["outcome_side"] = "no".into();
        raw["fill_count_fp"] = "2.75".into();
        raw["remaining_count_fp"] = "3.00".into();
    }

    f["fills"]["fills"][0]["ticker"] = ticker.into();
    f["fills"]["fills"][0]["market_ticker"] = ticker.into();
    f["fills"]["fills"][0]["book_side"] = "ask".into();
    f["positions"]["market_positions"][0]["ticker"] = ticker.into();
    f
}

fn report_instrument() -> BinaryOption {
    let f: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
    let mut market: Market =
        serde_json::from_value(f["definitions"]["live"]["market"].clone()).unwrap();
    let mut event: Event =
        serde_json::from_value(f["definitions"]["live"]["event"].clone()).unwrap();
    market.exchange_index = Field::Value(3);
    event.exchange_index = Field::Value(3);
    parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
}

fn report_client(
    address: std::net::SocketAddr,
    known: bool,
    cached_definition: bool,
) -> (KalshiExecutionClient, BinaryOption, Rc<RefCell<Cache>>) {
    let instrument = report_instrument();
    let cache = Rc::new(RefCell::new(Cache::default()));
    if cached_definition {
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::BinaryOption(instrument.clone()))
            .unwrap();
    }

    if known {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id(ClientOrderId::from("client-a"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from_decimal_dp("5.75".parse().unwrap(), 2).unwrap())
            .price(Price::from_decimal_dp("0.123401".parse().unwrap(), 6).unwrap())
            .time_in_force(TimeInForce::Gtc)
            .post_only(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, Some(ClientId::from("TEST")), false)
            .unwrap();
        cache
            .borrow_mut()
            .add_venue_order_id(
                &ClientOrderId::from("client-a"),
                &VenueOrderId::from("order-a"),
                false,
            )
            .unwrap();
    }

    let config = KalshiExecutionClientConfig {
        api_key_id: Some("auth-test-id".into()),
        private_key: Some(keys::ed25519_pkcs8().into()),
        base_url_http: Some(format!("http://{address}/custom/v3").into()),
        http_timeout_secs: 1,
        exchange_index: 3,
        ..Default::default()
    };

    let core = ExecutionClientCore::new(
        "TRADER-001".into(),
        "TEST".into(),
        *KALSHI_VENUE,
        OmsType::Netting,
        config.account_id,
        AccountType::Cash,
        Some(Currency::USD()),
        Rc::clone(&cache),
    );

    (
        KalshiExecutionClient::new(core, config).unwrap(),
        instrument,
        cache,
    )
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap()
}

fn snapshot_replies(f: &Value) -> Vec<Value> {
    vec![
        f["cutoff"].clone(),
        f["orders"].clone(),
        f["orders"].clone(),
        f["fills"].clone(),
        f["fills"].clone(),
        f["positions"].clone(),
        f["positions"].clone(),
        f["positions"].clone(),
    ]
}
