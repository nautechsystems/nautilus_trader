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
