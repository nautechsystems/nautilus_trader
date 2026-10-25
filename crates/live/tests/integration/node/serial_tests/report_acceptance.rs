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

use nautilus_live::config::RoutingConfig;
#[cfg(not(madsim))]
use nautilus_live::node::RunnerMetricsDelta;

use super::*;

const RECONCILIATION_TIMEOUT: Duration = Duration::from_millis(100);

#[rstest]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_bulk_report_acceptance_applies_after_finalization_crosses_deadline() {
    let states = [
        BlockingReportClientState::default(),
        BlockingReportClientState::default(),
    ];
    states[0]
        .order_report_finish_ns
        .store(200_000_000, Ordering::Relaxed);
    let mut node = bulk_acceptance_node(&states, None, 1.0, 1, RECONCILIATION_TIMEOUT);
    let cache = node.kernel().cache();
    let handle = node.handle();

    let driver = async {
        loop {
            if cache
                .borrow()
                .order(&ClientOrderId::from("O-BULK-1"))
                .unwrap()
                .status()
                == OrderStatus::Canceled
            {
                break;
            }

            dst::time::sleep(Duration::from_millis(1)).await;
        }

        handle.stop();
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(5), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .expect("accepted reports reach the engine after slow finalization");

    result.unwrap();

    for (index, state) in states.iter().enumerate() {
        let client_order_id = ClientOrderId::from(format!("O-BULK-{index}"));
        assert_eq!(*state.order_reports_finalized.lock(), vec![client_order_id]);
        assert_eq!(state.bulk_order_report_count.load(Ordering::Relaxed), 1);
        assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
        let cache = cache.borrow();
        let order = cache.order(&client_order_id).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(
            order.venue_order_id(),
            Some(VenueOrderId::from(format!("V-BULK-{index}")))
        );
        assert_eq!(order.filled_qty(), Quantity::from("0.0"));
        assert_eq!(order.events().len(), 4);
        assert!(matches!(order.last_event(), OrderEventAny::Canceled(event)
            if event.client_order_id == client_order_id));
    }

    assert_eq!(handle.state(), NodeState::Stopped);
}

#[rstest]
#[case::expired(false)]
#[case::shutdown(true)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_bulk_report_acceptance_discards_ready_client_with_pending_batch(
    #[case] shutdown: bool,
) {
    let states = [
        BlockingReportClientState::default(),
        BlockingReportClientState::default(),
    ];
    let releases = [
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(tokio::sync::Notify::new()),
    ];

    let timeout = if shutdown {
        Duration::from_secs(10)
    } else {
        RECONCILIATION_TIMEOUT
    };

    let mut node = bulk_acceptance_node(&states, Some(&releases), 1.0, 1, timeout);
    let cache = node.kernel().cache();
    let handle = node.handle();

    let driver = async {
        let first = loop {
            if let Some(index) = states
                .iter()
                .position(|state| state.bulk_order_report_requested.load(Ordering::Relaxed))
            {
                break index;
            }

            dst::time::sleep(Duration::from_millis(1)).await;
        };

        releases[first].notify_one();
        while !states[1 - first]
            .bulk_order_report_requested
            .load(Ordering::Relaxed)
        {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        if !shutdown {
            dst::time::sleep(Duration::from_millis(150)).await;
        }

        handle.stop();
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(5), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .expect("pending batch is cancelled and workers are drained");

    result.unwrap();

    for (index, state) in states.iter().enumerate() {
        assert_eq!(
            *state.order_reports_finalized.lock(),
            Vec::<ClientOrderId>::new()
        );
        assert_eq!(state.bulk_order_report_count.load(Ordering::Relaxed), 1);
        assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
        let client_order_id = ClientOrderId::from(format!("O-BULK-{index}"));
        let cache = cache.borrow();
        let order = cache.order(&client_order_id).unwrap();
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.filled_qty(), Quantity::from("0.0"));
        assert_eq!(order.events().len(), 3);
    }

    assert_eq!(handle.state(), NodeState::Stopped);
}

#[cfg(not(madsim))]
#[rstest]
#[ignore = "Wall-clock comparison; run optimized with process isolation"]
#[tokio::test]
async fn bulk_report_maintenance_latency() {
    const BATCHES: usize = 20;
    const REPORTS: usize = 64;

    let states = [
        BlockingReportClientState::default(),
        BlockingReportClientState::default(),
    ];

    for state in &states {
        state.report_cpu_ns.store(500_000, Ordering::Relaxed);
        state.report_cpu_repeated.store(true, Ordering::Relaxed);
    }

    let mut node = bulk_acceptance_node(&states, None, 0.01, REPORTS, RECONCILIATION_TIMEOUT);
    let handle = node.handle();
    let snapshot = handle.metrics_snapshot();

    let driver = async {
        while states
            .iter()
            .any(|state| state.bulk_order_report_count.load(Ordering::Relaxed) < BATCHES)
        {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        while states
            .iter()
            .any(|state| state.order_reports_finalized.lock().len() < BATCHES * REPORTS)
        {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        handle.stop();
    };

    let core_poll_ns = Cell::new(0_u64);
    let run = node.run();
    tokio::pin!(run);

    let measured = std::future::poll_fn(|cx| {
        let started = std::time::Instant::now();
        let result = std::future::Future::poll(run.as_mut(), cx);
        core_poll_ns.set(core_poll_ns.get() + started.elapsed().as_nanos() as u64);
        result
    });

    let (result, ()) = dst::time::timeout(Duration::from_secs(10), async {
        tokio::join!(measured, driver)
    })
    .await
    .expect("benchmark completes the measured batches");

    result.unwrap();
    let metrics = RunnerMetricsDelta::from_snapshots(snapshot, handle.metrics_snapshot());
    let continuations: u64 = states
        .iter()
        .map(|state| state.order_report_finish_busy_ns.load(Ordering::Relaxed))
        .sum();
    let collections: usize = states
        .iter()
        .map(|state| state.bulk_order_report_count.load(Ordering::Relaxed))
        .sum();
    let finalized: usize = states
        .iter()
        .map(|state| state.order_reports_finalized.lock().len())
        .sum();
    assert_eq!(collections, states.len() * BATCHES);
    assert_eq!(finalized, states.len() * BATCHES * REPORTS);

    for state in &states {
        assert_eq!(state.report_collections_max.load(Ordering::Relaxed), 1);
        assert_ne!(
            *state.report_thread.lock(),
            Some(std::thread::current().id())
        );
    }

    println!(
        "BULK_BENCH clients=2 reports_per_client={REPORTS} batches={BATCHES} core_poll_ns={} maintenance_ns={} continuation_ns={continuations}",
        core_poll_ns.get(),
        metrics.maintenance_busy_ns
    );
}

fn bulk_acceptance_node(
    states: &[BlockingReportClientState; 2],
    releases: Option<&[Arc<tokio::sync::Notify>; 2]>,
    interval_secs: f64,
    reports_per_client: usize,
    timeout: Duration,
) -> LiveNode {
    let mut config = reconciliation_node_config(1);
    config.timeout_reconciliation = timeout;
    config.exec_engine.open_check_interval_secs = Some(interval_secs);
    let mut builder = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("BulkAcceptance");

    for (index, state) in states.iter().enumerate() {
        state.report_worker.store(true, Ordering::Relaxed);
        let client_id = ClientId::from(format!("BULK-{index}"));
        let mut factory = BlockingReportExecutionClientFactory::configurable(
            client_id,
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        )
        .with_order_reports(bulk_order_reports(index, reports_per_client));
        factory.report_release = releases.map(|releases| releases[index].clone());
        builder = builder
            .add_exec_client_with_routing(
                Some(format!("bulk-{index}")),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
                RoutingConfig {
                    default: index == 0,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let node = builder.build().unwrap();
    add_reconciliation_test_state(&node);

    for index in 0..2 {
        let client_id = ClientId::from(format!("BULK-{index}"));

        for report in bulk_order_reports(index, reports_per_client) {
            add_accepted_test_order(
                &node,
                report.client_order_id.unwrap(),
                report.venue_order_id,
                client_id,
            );
        }
    }

    node
}

fn bulk_order_reports(client: usize, count: usize) -> Vec<OrderStatusReport> {
    (0..count)
        .map(|index| {
            let suffix = if index == 0 {
                client.to_string()
            } else {
                format!("{client}-{index}")
            };

            terminal_order_report(
                ClientOrderId::from(format!("O-BULK-{suffix}")),
                VenueOrderId::from(format!("V-BULK-{suffix}")),
                OrderStatus::Canceled,
                Quantity::from("0.0"),
            )
        })
        .collect()
}

#[rstest]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_report_acceptance_delivers_fill_after_deadline() {
    let state = BlockingReportClientState::default();
    state.report_worker.store(true, Ordering::Relaxed);
    state
        .fill_report_finish_ns
        .store(200_000_000, Ordering::Relaxed);
    let client_order_id = ClientOrderId::from("O-ACCEPTED-FILL");
    let venue_order_id = VenueOrderId::from("V-ACCEPTED-FILL");
    let trade_id = TradeId::from("T-ACCEPTED-FILL");
    let fill = reconciliation_fill_report(
        client_order_id,
        venue_order_id,
        trade_id,
        Price::from("100.0"),
        Money::from("0.25 USDT"),
        UnixNanos::from(1_000_000),
    );
    let factory = BlockingReportExecutionClientFactory::configurable(
        ClientId::from("BLOCKING-REPORT"),
        AccountId::from("BLOCKING-REPORT-001"),
        state.clone(),
    )
    .with_position_reports(vec![reconciliation_position_report(Quantity::from("1.0"))])
    .with_fill_report_responses([vec![fill]], None)
    .with_fill_reports_at_window_end();
    let mut config = shutdown_report_test_config(None, Some(0.1));
    config.timeout_reconciliation = RECONCILIATION_TIMEOUT;
    config.delay_post_stop = Duration::ZERO;
    let replay_client = BlockingReportExecutionClient::new(&factory);
    let mut node = reconciliation_node("AcceptedPositionFill", config, factory);
    add_accepted_test_order(
        &node,
        client_order_id,
        venue_order_id,
        ClientId::from("BLOCKING-REPORT"),
    );
    let cache = node.kernel().cache();
    let handle = node.handle();

    let driver = async {
        loop {
            if !state.fill_reports_finalized.lock().is_empty() {
                break;
            }

            dst::time::sleep(Duration::from_millis(1)).await;
        }

        dst::time::sleep(Duration::from_millis(1)).await;
        handle.stop();
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(2), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .expect("accepted fill reaches engine despite slow continuation");

    result.unwrap();
    assert_eq!(*state.fill_reports_finalized.lock(), vec![trade_id]);

    let command = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::from(2_000_000),
        Some(crypto_perpetual_ethusdt().id()),
        None,
        None,
        None,
        None,
        None,
    );
    let replay = replay_client.fill_report_task(&command);
    replay.collection.await;
    assert_eq!((replay.finish)().unwrap(), Vec::<FillReport>::new());
    assert_eq!(state.fill_report_count.load(Ordering::Relaxed), 2);
    assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
    let cache = cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(order.filled_qty(), Quantity::from("1.0"));
    assert!(matches!(order.last_event(), OrderEventAny::Filled(event)
        if event.trade_id == trade_id && event.last_qty == Quantity::from("1.0")));
}

#[cfg(not(madsim))]
#[rstest]
#[ignore = "Wall-clock comparison; run optimized with process isolation"]
#[tokio::test]
async fn position_report_maintenance_latency() {
    const KEYS: usize = 16;
    const BATCHES: usize = 20;
    let state = BlockingReportClientState::default();
    state.report_worker.store(true, Ordering::Relaxed);
    state.report_cpu_ns.store(100_000, Ordering::Relaxed);
    state.report_cpu_repeated.store(true, Ordering::Relaxed);

    let instruments = (0..KEYS)
        .map(|index| {
            let mut instrument = crypto_perpetual_ethusdt();
            instrument.id = InstrumentId::from(format!("BENCH-{index}.BINANCE"));
            InstrumentAny::CryptoPerpetual(instrument)
        })
        .collect::<Vec<_>>();

    let positions = instruments
        .iter()
        .map(|instrument| {
            let mut report = reconciliation_position_report(Quantity::from("1.0"));
            report.instrument_id = instrument.id();
            report
        })
        .collect();

    let factory = BlockingReportExecutionClientFactory::configurable(
        ClientId::from("BLOCKING-REPORT"),
        AccountId::from("BLOCKING-REPORT-001"),
        state.clone(),
    )
    .with_position_reports(positions)
    .with_fill_report_responses([vec![]], None);
    let mut config = shutdown_report_test_config(None, Some(0.01));
    config.exec_engine.generate_missing_orders = false;
    config.timeout_reconciliation = Duration::from_secs(5);
    config.delay_post_stop = Duration::ZERO;
    let mut node = reconciliation_node("PositionMaintenanceLatency", config, factory);
    for instrument in instruments {
        node.kernel()
            .cache()
            .borrow_mut()
            .add_instrument(instrument)
            .unwrap();
    }

    let handle = node.handle();
    let snapshot = handle.metrics_snapshot();

    let driver = async {
        while state.fill_report_finalizations.load(Ordering::Relaxed) < KEYS * BATCHES {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        handle.stop();
    };

    let core_poll_ns = Cell::new(0_u64);
    let run = node.run();
    tokio::pin!(run);

    let measured = std::future::poll_fn(|cx| {
        let started = std::time::Instant::now();
        let result = std::future::Future::poll(run.as_mut(), cx);
        core_poll_ns.set(core_poll_ns.get() + u64::try_from(started.elapsed().as_nanos()).unwrap());
        result
    });

    let (result, ()) = dst::time::timeout(Duration::from_secs(10), async {
        tokio::join!(measured, driver)
    })
    .await
    .expect("multi-key batches complete");

    result.unwrap();
    assert_eq!(state.position_report_count.load(Ordering::Relaxed), BATCHES);
    assert_eq!(
        state.fill_report_count.load(Ordering::Relaxed),
        KEYS * BATCHES
    );
    assert_eq!(
        state.fill_report_finalizations.load(Ordering::Relaxed),
        KEYS * BATCHES
    );
    assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
    assert_eq!(state.report_collections_max.load(Ordering::Relaxed), 1);
    assert_ne!(
        *state.report_thread.lock(),
        Some(std::thread::current().id())
    );
    let metrics = RunnerMetricsDelta::from_snapshots(snapshot, handle.metrics_snapshot());
    println!(
        "POSITION_BENCH keys={KEYS} batches={BATCHES} fills_per_query=0 core_poll_ns={} maintenance_ns={} continuation_ns={}",
        core_poll_ns.get(),
        metrics.maintenance_busy_ns,
        state.fill_report_finish_busy_ns.load(Ordering::Relaxed)
    );
}

#[rstest]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_targeted_report_acceptance_delivers_fill_after_deadline() {
    let state = BlockingReportClientState::default();
    state.report_worker.store(true, Ordering::Relaxed);
    state
        .fill_report_finish_ns
        .store(200_000_000, Ordering::Relaxed);
    let client_order_id = ClientOrderId::from("O-TARGETED-ACCEPTED");
    let venue_order_id = VenueOrderId::from("V-TARGETED-ACCEPTED");
    let trade_id = TradeId::from("T-TARGETED-ACCEPTED");
    let fill = reconciliation_fill_report(
        client_order_id,
        venue_order_id,
        trade_id,
        Price::from("101.0"),
        Money::from("0.25 USDT"),
        UnixNanos::from(1_000_000),
    );
    let factory = BlockingReportExecutionClientFactory::configurable(
        ClientId::from("BLOCKING-REPORT"),
        AccountId::from("BLOCKING-REPORT-001"),
        state.clone(),
    )
    .with_order_reports(Vec::new())
    .with_targeted_order_report(terminal_order_report(
        client_order_id,
        venue_order_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
    ))
    .with_fill_report_responses([vec![fill]], None);
    let replay_client = BlockingReportExecutionClient::new(&factory);
    let mut config = reconciliation_node_config(1);
    config.timeout_reconciliation = RECONCILIATION_TIMEOUT;
    config.exec_engine.open_check_interval_secs = Some(1.0);
    config.delay_post_stop = Duration::ZERO;
    let mut node = targeted_acceptance_node(config, vec![factory]);
    let cache = node.kernel().cache();
    let handle = node.handle();

    let driver = async {
        while state.fill_reports_finalized.lock().is_empty() {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        dst::time::sleep(Duration::from_millis(1)).await;
        handle.stop();
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(5), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .expect("targeted accepted fills reach engine");

    result.unwrap();

    let command = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::from(2_000_000),
        Some(crypto_perpetual_ethusdt().id()),
        Some(venue_order_id),
        None,
        None,
        None,
        None,
    );
    let replay = replay_client.fill_report_task(&command);
    replay.collection.await;
    assert_eq!((replay.finish)().unwrap(), Vec::<FillReport>::new());
    assert_eq!(
        *state.targeted_order_reports_finalized.lock(),
        vec![client_order_id]
    );
    assert_eq!(*state.fill_reports_finalized.lock(), vec![trade_id]);
    assert_eq!(state.fill_report_count.load(Ordering::Relaxed), 2);
    assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
    let cache = cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("1.0"));

    let fills = order
        .events()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].trade_id, trade_id);
    assert_eq!(fills[0].last_qty, Quantity::from("1.0"));
    assert_eq!(fills[0].last_px, Price::from("101.0"));
    assert_eq!(fills[0].commission, Some(Money::from("0.25 USDT")));
}

#[rstest]
#[case::fill_timeout(false, false)]
#[case::shutdown(true, false)]
#[case::fill_error(false, true)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_targeted_report_acceptance_carries_status_across_fill_failure(
    #[case] shutdown: bool,
    #[case] error: bool,
) {
    let states = [
        BlockingReportClientState::default(),
        BlockingReportClientState::default(),
    ];
    let release = Arc::new(tokio::sync::Notify::new());
    states[1].fill_report_error.store(error, Ordering::Relaxed);
    let mut config = reconciliation_node_config(2);
    config.timeout_reconciliation = Duration::from_millis(500);
    config.exec_engine.inflight_check_threshold_ms = 0;
    config.exec_engine.open_check_interval_secs = Some(1.0);
    config.delay_post_stop = Duration::ZERO;
    let mut factories = Vec::new();

    for (index, state) in states.iter().enumerate() {
        state.report_worker.store(false, Ordering::Relaxed);
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from(format!("TARGETED-{index}")),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        )
        .with_order_reports(Vec::new())
        .with_targeted_order_report(terminal_order_report(
            ClientOrderId::from(format!("O-CARRIED-{index}")),
            VenueOrderId::from(format!("V-CARRIED-{index}")),
            OrderStatus::Canceled,
            Quantity::from(if index == 0 { "0.0" } else { "1.0" }),
        ))
        .with_fill_report_responses([Vec::new()], (index == 1).then(|| release.clone()));
        factories.push(factory);
    }

    let mut node = targeted_acceptance_node(config, factories);

    let cache = node.kernel().cache();
    let order = cache
        .borrow()
        .order_owned(&ClientOrderId::from("O-CARRIED-0"))
        .unwrap();
    let event_topic = switchboard::get_event_order_topic(order.strategy_id());
    let observed_updates = Rc::new(RefCell::new(Vec::new()));
    let callback_updates = observed_updates.clone();
    let updated = states[1].instrument_received.clone();

    let event_handler = msgbus::TypedHandler::from(move |event: &OrderEventAny| {
        if matches!(event, OrderEventAny::Canceled(_)) {
            callback_updates
                .borrow_mut()
                .push(updated.load(Ordering::Relaxed));
        }
    });

    msgbus::subscribe_order_events(event_topic.into(), event_handler.clone(), None);
    let handle = node.handle();

    let driver = async {
        while states[1].fill_report_count.load(Ordering::Relaxed) == 0 {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let topic = switchboard::get_instrument_topic(instrument.id());
        msgbus::publish_instrument(topic, &instrument);
        assert!(!states[1].instrument_received.load(Ordering::Relaxed));

        if error {
            release.notify_one();
        }

        if shutdown {
            handle.stop();
        } else {
            while cache
                .borrow()
                .order(&ClientOrderId::from("O-CARRIED-0"))
                .unwrap()
                .status()
                != OrderStatus::Canceled
            {
                dst::time::sleep(Duration::from_millis(1)).await;
            }

            handle.stop();
        }
    };

    let (result, ()) = dst::time::timeout(Duration::from_secs(5), async {
        tokio::join!(node.run(), driver)
    })
    .await
    .expect("carried statuses survive interrupted fill stage");

    msgbus::unsubscribe_order_events(event_topic.into(), &event_handler);
    result.unwrap();
    assert_eq!(*observed_updates.borrow(), vec![true]);

    for (index, state) in states.iter().enumerate() {
        assert_eq!(
            *state.targeted_order_reports_finalized.lock(),
            vec![ClientOrderId::from(format!("O-CARRIED-{index}"))]
        );
        assert_eq!(state.report_collections_active.load(Ordering::Relaxed), 0);
        assert_eq!(state.fill_report_finalizations.load(Ordering::Relaxed), 0);
    }

    assert_eq!(
        cache
            .borrow()
            .order(&ClientOrderId::from("O-CARRIED-0"))
            .unwrap()
            .status(),
        OrderStatus::Canceled
    );
    assert_eq!(
        cache
            .borrow()
            .order(&ClientOrderId::from("O-CARRIED-1"))
            .unwrap()
            .status(),
        OrderStatus::Accepted
    );
    assert_eq!(
        cache
            .borrow()
            .order(&ClientOrderId::from("O-CARRIED-1"))
            .unwrap()
            .filled_qty(),
        Quantity::from("0.0")
    );

    for index in 0..2 {
        let id = ClientOrderId::from(format!("O-CARRIED-{index}"));
        node.exec_manager_mut().register_inflight(id);
    }

    dst::time::sleep(Duration::from_millis(1)).await;
    let inflight = node.exec_manager_mut().check_inflight_orders();
    assert!(inflight.queries.iter().any(|cmd| matches!(cmd,TradingCommand::QueryOrder(query) if query.client_order_id==ClientOrderId::from("O-CARRIED-1"))),"unresolved carried status clears its targeted marker");
}

#[cfg(not(madsim))]
#[rstest]
#[ignore = "Wall-clock comparison; run optimized with process isolation"]
#[tokio::test]
async fn targeted_report_maintenance_latency() {
    const CLIENTS: usize = 16;
    const BATCHES: usize = 20;
    let states = (0..CLIENTS)
        .map(|_| BlockingReportClientState::default())
        .collect::<Vec<_>>();
    let mut config = reconciliation_node_config(CLIENTS as u32);
    config.exec_engine.open_check_interval_secs = Some(0.01);
    let mut factories = Vec::new();

    for (index, state) in states.iter().enumerate() {
        state.report_worker.store(true, Ordering::Relaxed);
        state.report_cpu_ns.store(100_000, Ordering::Relaxed);
        state.report_cpu_repeated.store(true, Ordering::Relaxed);
        let mut report = terminal_order_report(
            ClientOrderId::from(format!("O-TARGETED-BENCH-{index}")),
            VenueOrderId::from(format!("V-TARGETED-BENCH-{index}")),
            OrderStatus::Accepted,
            Quantity::from("0.0"),
        );
        report.price = Some(Price::from("100.0"));
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from(format!("TARGETED-BENCH-{index}")),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        )
        .with_order_reports(Vec::new())
        .with_targeted_order_report(report);
        factories.push(factory);
    }

    let mut node = targeted_acceptance_node(config, factories);

    let handle = node.handle();
    let snapshot = handle.metrics_snapshot();

    let driver = async {
        while states
            .iter()
            .any(|state| state.targeted_order_reports_finalized.lock().len() < BATCHES)
        {
            dst::time::sleep(Duration::from_millis(1)).await;
        }

        handle.stop();
    };

    let core_poll_ns = Cell::new(0u64);
    let run = node.run();
    tokio::pin!(run);

    let measured = std::future::poll_fn(|cx| {
        let start = std::time::Instant::now();
        let result = std::future::Future::poll(run.as_mut(), cx);
        core_poll_ns.set(core_poll_ns.get() + start.elapsed().as_nanos() as u64);
        result
    });

    let (result, ()) = dst::time::timeout(Duration::from_secs(10), async {
        tokio::join!(measured, driver)
    })
    .await
    .expect("representative targeted batches complete");

    result.unwrap();
    let metrics = RunnerMetricsDelta::from_snapshots(snapshot, handle.metrics_snapshot());

    for state in &states {
        assert_eq!(state.targeted_order_reports_finalized.lock().len(), BATCHES);
        assert_eq!(state.report_collections_max.load(Ordering::Relaxed), 1);
        assert_ne!(
            *state.report_thread.lock(),
            Some(std::thread::current().id())
        );
    }

    println!(
        "TARGETED_BENCH clients={CLIENTS} batches={BATCHES} core_poll_ns={} maintenance_ns={}",
        core_poll_ns.get(),
        metrics.maintenance_busy_ns
    );
}

fn targeted_acceptance_node(
    config: LiveNodeConfig,
    factories: Vec<BlockingReportExecutionClientFactory>,
) -> LiveNode {
    let mut builder = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("TargetedAcceptance");
    let mut orders = Vec::new();
    for (index, factory) in factories.into_iter().enumerate() {
        orders.push((
            factory.client_id,
            factory
                .targeted_order_report
                .clone()
                .expect("targeted test report"),
        ));
        builder = builder
            .add_exec_client_with_routing(
                Some(format!("targeted-{index}")),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
                RoutingConfig {
                    default: index == 0,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let node = builder.build().unwrap();
    add_reconciliation_test_state(&node);

    for (client_id, report) in orders {
        add_accepted_test_order(
            &node,
            report.client_order_id.unwrap(),
            report.venue_order_id,
            client_id,
        );
    }

    node
}
