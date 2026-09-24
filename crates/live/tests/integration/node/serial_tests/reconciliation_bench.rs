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

use std::{future::Future, time::Instant};

use nautilus_common::{
    live::runner::get_data_event_sender, messages::DataEvent, msgbus::TypedHandler,
};
use nautilus_live::node::{RunnerMetricsDelta, RunnerMetricsSnapshot};
use nautilus_model::{
    data::{Data, TradeTick},
    enums::AggressorSide,
};

use super::*;

const PERIOD: Duration = Duration::from_micros(100);
const WARMUP: usize = 1_000;
const SAMPLES: usize = 30_000;

#[rstest]
#[case::baseline("baseline", 0, 0)]
#[case::stall_control("stall_control", 0, 0)]
#[case::steady("steady", 0, 0)]
#[case::baseline_many("baseline_many", 0, 0)]
#[case::steady_many("steady_many", 0, 0)]
#[case::cancel_many("cancel_many", 0, 0)]
#[case::targeted_1("targeted", 1, 0)]
#[case::targeted_10("targeted", 10, 0)]
#[case::targeted_64("targeted", 64, 0)]
#[case::targeted_256("targeted", 256, 0)]
#[case::targeted_1024("targeted", 1024, 0)]
#[case::position_1("position", 1, 0)]
#[case::position_10("position", 10, 0)]
#[case::position_64("position", 64, 0)]
#[case::position_256("position", 256, 0)]
#[case::position_1024("position", 1024, 0)]
#[case::position_history_1024("position", 1, 1024)]
#[case::baseline_warm("baseline", 0, 1)]
#[case::targeted_warm_1("targeted", 1, 1)]
#[case::targeted_warm_1024("targeted", 1024, 1)]
#[case::position_warm_1("position", 1, 1)]
#[case::position_warm_1024("position", 1024, 1)]
#[ignore = "Wall-clock benchmark; run optimized with process isolation"]
#[tokio::test(flavor = "current_thread")]
async fn reconciliation_latency(
    #[case] scenario: &str,
    #[case] missing: usize,
    #[case] history: usize,
) {
    let client_order_id = ClientOrderId::from("O-BENCH");
    let venue_order_id = VenueOrderId::from("V-BENCH");
    let client_id = ClientId::from("BLOCKING-REPORT");
    let state = BlockingReportClientState::default();
    let release = Arc::new(tokio::sync::Notify::new());
    let total_fills = missing + history;
    let expected_qty = Decimal::new(total_fills as i64, 3);

    let order_count = if scenario.ends_with("_many") { 1024 } else { 1 };

    let mut report = terminal_order_report(
        client_order_id,
        venue_order_id,
        if matches!(scenario, "targeted" | "cancel_many") {
            OrderStatus::Canceled
        } else {
            OrderStatus::Accepted
        },
        Quantity::from_decimal_dp(expected_qty, 3).unwrap(),
    );

    report.avg_px = (total_fills > 0).then_some(Decimal::from(100));
    let mut reports = vec![report.clone()];

    for index in 1..order_count {
        let mut extra = report.clone();
        extra.client_order_id = Some(ClientOrderId::from(format!("O-BENCH-{index}")));
        extra.venue_order_id = VenueOrderId::from(format!("V-BENCH-{index}"));
        reports.push(extra);
    }

    let fills = (0..total_fills)
        .map(|index| {
            let mut fill = reconciliation_fill_report(
                client_order_id,
                venue_order_id,
                TradeId::from(format!("T-BENCH-{index:06}")),
                Price::from("100.00"),
                Money::from("0.002 USDT"),
                UnixNanos::from(index as u64 + 1),
            );
            fill.last_qty = Quantity::from("0.001");
            fill
        })
        .collect::<Vec<_>>();

    let mut factory = BlockingReportExecutionClientFactory::configurable(
        client_id,
        AccountId::from("BLOCKING-REPORT-001"),
        state.clone(),
    )
    .with_order_reports(reports.clone())
    .with_targeted_order_report(report)
    .with_position_reports(vec![reconciliation_position_report(
        Quantity::from_decimal_dp(expected_qty, 3).unwrap(),
    )])
    .with_fill_report_responses([fills.clone()], None)
    .with_fill_reports_at_window_end();
    factory.report_release = Some(release.clone());
    factory.report_release_once = true;
    let mut config = reconciliation_node_config(1);
    config.exec_engine.open_check_interval_secs = matches!(
        scenario,
        "steady" | "steady_many" | "cancel_many" | "targeted"
    )
    .then_some(0.05);
    config.exec_engine.position_check_interval_secs = (scenario == "position").then_some(0.05);
    config.exec_engine.position_check_threshold_ms = 0;
    config.exec_engine.position_check_retries = 100;
    let mut node = reconciliation_node("ReconciliationBench", config, factory);
    add_accepted_test_order(&node, client_order_id, venue_order_id, client_id);

    for report in reports.iter().skip(1) {
        add_accepted_test_order(
            &node,
            report.client_order_id.unwrap(),
            report.venue_order_id,
            client_id,
        );
    }

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

    for fill in fills.iter().take(history) {
        let order = node
            .kernel()
            .cache()
            .borrow()
            .order_owned(&client_order_id)
            .unwrap();
        let event = OrderFilledTestBuilder::new(&order, &instrument)
            .trade_id(fill.trade_id)
            .last_qty(fill.last_qty)
            .last_px(fill.last_px)
            .commission(fill.commission)
            .without_position_id()
            .build();
        apply_reconciliation_events(&mut node, &[event]);
    }

    let origin = Instant::now();
    let latencies = Rc::new(RefCell::new(Vec::with_capacity(SAMPLES)));
    let received = Rc::new(Cell::new(0usize));
    let samples = latencies.clone();
    let count = received.clone();
    let handle = node.handle();
    let callback_handle = handle.clone();
    let maintenance_previous = Rc::new(Cell::new(0));
    let maintenance_max = Rc::new(Cell::new(0));
    let maximum = maintenance_max.clone();
    let previous = maintenance_previous.clone();
    let snapshots = Rc::new(Cell::new([None::<RunnerMetricsSnapshot>; 2]));
    let callback_snapshots = snapshots.clone();
    let last_scheduled = Cell::new(None);
    let stall_control = scenario == "stall_control";
    let canceled = matches!(scenario, "targeted" | "cancel_many");
    let cache = node.kernel().cache();
    let callback_state = state.clone();
    let stall = Rc::new(Cell::new(None));
    let callback_stall = stall.clone();
    let stall_observed = Rc::new(Cell::new(false));
    let callback_stall_observed = stall_observed.clone();

    let handler = TypedHandler::from(move |trade: &TradeTick| {
        let now = origin.elapsed().as_nanos() as u64;
        let index = count.get();
        count.set(index + 1);
        let metrics = callback_handle.metrics_snapshot();
        let maintenance = metrics.maintenance_busy_ns;
        let mut boundaries = callback_snapshots.get();

        if index == WARMUP - 1 {
            boundaries[0] = Some(metrics);
            assert!(
                !callback_state
                    .report_response_returned
                    .load(Ordering::Relaxed)
            );
            assert_eq!(
                cache
                    .borrow()
                    .order(&client_order_id)
                    .unwrap()
                    .filled_qty()
                    .as_decimal(),
                Decimal::new(history as i64, 3),
            );
            release.notify_one();
        }

        if index == WARMUP + SAMPLES - 1 {
            boundaries[1] = Some(metrics);
            let cache = cache.borrow();
            assert_eq!(
                cache.orders_total_count(None, None, None, None, None),
                order_count
            );
            assert_eq!(
                cache.positions_open(None, None, None, None, None).len(),
                usize::from(total_fills > 0)
            );
            let order = cache.order(&client_order_id).unwrap();
            assert_eq!(order.filled_qty().as_decimal(), expected_qty);
            assert_eq!(order.trade_ids().len(), total_fills);
            assert_eq!(
                order.status(),
                if canceled {
                    OrderStatus::Canceled
                } else if total_fills > 0 {
                    OrderStatus::PartiallyFilled
                } else {
                    OrderStatus::Accepted
                }
            );

            for report in reports.iter().skip(1) {
                let order = cache.order(&report.client_order_id.unwrap()).unwrap();
                assert_eq!(order.status(), report.order_status);
                assert_eq!(order.filled_qty().as_decimal(), Decimal::ZERO);
                assert_eq!(order.trade_ids().len(), 0);
            }

            for fill in &fills {
                assert!(order.trade_ids().contains(&&fill.trade_id));
            }

            if total_fills > 0 {
                assert_eq!(order.avg_px(), Some(Decimal::from(100)));
                assert_eq!(
                    order
                        .commissions()
                        .get(&Currency::USDT())
                        .unwrap()
                        .as_decimal(),
                    Decimal::new(total_fills as i64 * 2, 3)
                );
                let position = cache
                    .position(cache.position_id(&client_order_id).unwrap())
                    .unwrap();
                assert_eq!(position.quantity.as_decimal(), expected_qty);
            }
        }

        callback_snapshots.set(boundaries);

        if let Some(last) = last_scheduled.get() {
            assert_eq!(trade.ts_event.as_u64() - last, PERIOD.as_nanos() as u64);
        }

        last_scheduled.set(Some(trade.ts_event.as_u64()));

        if index >= WARMUP {
            samples.borrow_mut().push((
                now - trade.ts_init.as_u64(),
                now - trade.ts_event.as_u64(),
                trade.ts_init.as_u64() - trade.ts_event.as_u64(),
            ));
            maximum.set(maximum.get().max(maintenance - previous.get()));
        }

        previous.set(maintenance);

        if let Some((start, end)) = callback_stall.get()
            && (start..end).contains(&trade.ts_init.as_u64())
            && now - trade.ts_init.as_u64() >= 4_000_000
        {
            callback_stall_observed.set(true);
        }

        if stall_control && index == WARMUP + 100 {
            let start = origin.elapsed().as_nanos() as u64;
            std::thread::sleep(Duration::from_millis(5));
            callback_stall.set(Some((start, origin.elapsed().as_nanos() as u64)));
        }
    });

    msgbus::subscribe_trades("data.trades.*".into(), handler, None);
    let sender = get_data_event_sender();
    let producer_handle = handle.clone();

    let producer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);

        while !producer_handle.is_running() {
            assert!(Instant::now() < deadline, "Node did not start");
            std::thread::sleep(Duration::from_millis(1));
        }

        let start = Instant::now() + Duration::from_millis(10);

        let mut trade = TradeTick::new(
            instrument.id(),
            Price::from("100.00"),
            Quantity::from("0.001"),
            AggressorSide::Buy,
            TradeId::from("MARKET-BENCH"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        for index in 0..WARMUP + SAMPLES {
            let due = start + PERIOD * index as u32;

            // Fixed deadlines retain samples during core stalls; late producers catch up
            while Instant::now() < due {
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
            }

            trade.ts_event = UnixNanos::from(due.duration_since(origin).as_nanos() as u64);
            trade.ts_init = UnixNanos::from(origin.elapsed().as_nanos() as u64);
            sender.send(DataEvent::Data(Data::Trade(trade))).unwrap();
        }
    });

    let driver_handle = handle.clone();
    let polls = RefCell::new(Vec::with_capacity(SAMPLES * 2));

    let driver = async {
        wait_until_async(
            || async { received.get() == WARMUP + SAMPLES },
            Duration::from_secs(15),
        )
        .await;
        driver_handle.stop();
        let [before, after] = snapshots.get();
        RunnerMetricsDelta::from_snapshots(before.unwrap(), after.unwrap())
    };

    let (delta, result) = {
        let run = node.run();
        tokio::pin!(run);

        let measured = std::future::poll_fn(|cx| {
            let measuring = (WARMUP..WARMUP + SAMPLES).contains(&received.get());
            let start = Instant::now();
            let result = run.as_mut().poll(cx);

            if measuring {
                polls.borrow_mut().push(start.elapsed().as_nanos() as u64);
            }

            result
        });

        tokio::join!(driver, measured)
    };

    result.unwrap();
    producer.join().unwrap();

    assert_eq!(received.get(), WARMUP + SAMPLES);

    if missing > 0 {
        assert!(state.fill_report_count.load(Ordering::Relaxed) > 0);
    } else {
        assert_eq!(state.fill_report_count.load(Ordering::Relaxed), 0);
    }

    match scenario {
        "baseline" | "baseline_many" | "stall_control" => {
            assert_eq!(state.bulk_order_report_count.load(Ordering::Relaxed), 0);
            assert_eq!(state.position_report_count.load(Ordering::Relaxed), 0);
        }
        "steady" | "steady_many" | "cancel_many" | "targeted" => {
            assert!(state.bulk_order_report_count.load(Ordering::Relaxed) > 0);
        }
        "position" => assert!(state.position_report_count.load(Ordering::Relaxed) > 0),
        _ => unreachable!(),
    }

    let samples = latencies.borrow();
    assert_eq!(samples.len(), SAMPLES);

    if stall_control {
        assert!(
            stall_observed.get(),
            "Producer did not expose the deliberate stall"
        );
    }

    let poll_values = polls.into_inner();
    let poll_busy_ns: u64 = poll_values.iter().sum();
    let distributions = [
        (
            "delivery",
            samples.iter().map(|value| value.0).collect::<Vec<_>>(),
        ),
        ("scheduled", samples.iter().map(|value| value.1).collect()),
        ("producer", samples.iter().map(|value| value.2).collect()),
        ("core_poll", poll_values),
    ];

    for (metric, mut values) in distributions {
        values.sort_unstable();

        let percentile = |numerator: usize, denominator: usize| {
            values[(values.len() * numerator).div_ceil(denominator) - 1] as f64 / 1_000.0
        };

        println!(
            "scenario={scenario} missing={missing} history={history} metric={metric} samples={} \
             p50_us={:.3} p95_us={:.3} p99_us={:.3} p999_us={:.3} max_us={:.3}",
            values.len(),
            percentile(50, 100),
            percentile(95, 100),
            percentile(99, 100),
            percentile(999, 1000),
            values.last().unwrap().to_owned() as f64 / 1_000.0,
        );
    }

    println!(
        "scenario={scenario} missing={missing} history={history} core_poll_busy_us={:.3} maintenance_us={:.3} \
         maintenance_between_ticks_max_us={:.3} fill_queries={} order_queries={} position_queries={}",
        poll_busy_ns as f64 / 1_000.0,
        delta.maintenance_busy_ns as f64 / 1_000.0,
        maintenance_max.get() as f64 / 1_000.0,
        state.fill_report_count.load(Ordering::Relaxed),
        state.bulk_order_report_count.load(Ordering::Relaxed),
        state.position_report_count.load(Ordering::Relaxed),
    );
}
