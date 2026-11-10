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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use nautilus_common::{
    cache::CacheView,
    clients::{ExecutionClient, ExecutionReportTask},
    clock::Clock,
    factories::{ClientConfig, ExecutionClientFactory},
    live::{get_runtime, runner::get_data_event_sender},
    logging::logger::LoggerConfig,
    messages::{DataEvent, execution::GenerateOrderStatusReports},
    msgbus::{self, TypedHandler},
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::LiveNode,
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Data, TradeTick},
    enums::{AggressorSide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, InstrumentId, TradeId, TraderId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, stubs::TestOrderEventStubs},
    reports::OrderStatusReport,
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use nautilus_polymarket::{
    common::consts::POLYMARKET, factories::PolymarketExecutionClientFactory,
};
use rstest::rstest;
use serde_json::json;

use crate::{
    harness,
    mock_venue::{TestServerState, execution_config, load_json, start_mock_server},
};

const PAGE_SIZE: usize = 64;
const PERIOD: Duration = Duration::from_micros(100);
const CHECK_PERIOD_SECS: f64 = 0.05;
const DEADLINE: Duration = Duration::from_secs(30);

#[rstest]
#[case::baseline(CollectionMode::Baseline, 0)]
#[case::inline_64(CollectionMode::Inline, 64)]
#[case::worker_64(CollectionMode::Worker, 64)]
#[case::inline_1024(CollectionMode::Inline, 1024)]
#[case::worker_1024(CollectionMode::Worker, 1024)]
#[ignore = "Wall-clock benchmark; run optimized in isolated processes"]
#[tokio::test(flavor = "current_thread")]
async fn reconciliation_latency(#[case] mode: CollectionMode, #[case] orders: usize) {
    run_scenario(mode, orders, 30, 15_000).await;
}

#[rstest]
#[case::inline(CollectionMode::Inline)]
#[case::worker(CollectionMode::Worker)]
#[tokio::test(flavor = "current_thread")]
async fn reconciliation_benchmark_smoke(#[case] mode: CollectionMode) {
    run_scenario(mode, PAGE_SIZE + 1, 2, 1_000).await;
}

async fn run_scenario(mode: CollectionMode, orders: usize, cycles: usize, samples: usize) {
    let state = TestServerState::default();
    state.configure_default_order_success().await;
    populate_pages(&state, orders).await;
    let server_state = state.clone();
    let addr = get_runtime()
        .spawn(async move { start_mock_server(server_state).await })
        .await
        .unwrap();
    let ready = Arc::new(AtomicBool::new(mode == CollectionMode::Baseline));

    let probe = Rc::new(RefCell::new(ReportProbe {
        orders,
        limit: cycles + 1,
        completed: 0,
        durations: Vec::with_capacity(cycles),
        ready: ready.clone(),
    }));

    let config = LiveNodeConfig {
        trader_id: TraderId::from(harness::TRADER_ID),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 0,
            open_check_interval_secs: (mode != CollectionMode::Baseline)
                .then_some(CHECK_PERIOD_SECS),
            open_check_lookback_mins: None,
            open_check_threshold_ms: 0,
            open_check_open_only: true,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("PolymarketReconciliationBench")
        .add_exec_client(
            None,
            Box::new(ReportFactory {
                mode,
                probe: probe.clone(),
            }),
            Box::new(execution_config(addr)),
        )
        .unwrap()
        .build()
        .unwrap();

    let instrument = harness::instrument();
    seed_orders(&node, &instrument, orders);
    let origin = Instant::now();
    let received = Rc::new(Cell::new(0));
    let latencies = Rc::new(RefCell::new(Vec::with_capacity(samples)));
    let callback_received = received.clone();
    let callback_latencies = latencies.clone();
    let boundaries = Rc::new(Cell::new((0, 0)));
    let callback_boundaries = boundaries.clone();
    let callback_probe = probe.clone();
    msgbus::subscribe_trades(
        "data.trades.*".into(),
        TypedHandler::from(move |trade: &TradeTick| {
            let now = origin.elapsed().as_nanos() as u64;
            callback_received.set(callback_received.get() + 1);
            if callback_received.get() == 1 {
                callback_boundaries.set((callback_probe.borrow().completed, 0));
            }

            if callback_received.get() == samples {
                callback_boundaries.set((
                    callback_boundaries.get().0,
                    callback_probe.borrow().completed,
                ));
            }

            callback_latencies.borrow_mut().push((
                now - trade.ts_init.as_u64(),
                now - trade.ts_event.as_u64(),
                trade.ts_init.as_u64() - trade.ts_event.as_u64(),
            ));
        }),
        None,
    );

    let handle = node.handle();
    let sender = get_data_event_sender();
    let producer_handle = handle.clone();

    let producer = std::thread::spawn(move || {
        let deadline = Instant::now() + DEADLINE;

        while !producer_handle.is_running() || !ready.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "benchmark node did not become ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }

        let start = Instant::now() + Duration::from_millis(10);
        for index in 0..samples {
            let due = start + PERIOD * index as u32;
            while Instant::now() < due {
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
            }

            let trade = TradeTick::new(
                instrument.id(),
                Price::from("0.5000"),
                Quantity::from("0.0010"),
                AggressorSide::Buy,
                TradeId::from("RECONCILIATION-BENCH"),
                UnixNanos::from(due.duration_since(origin).as_nanos() as u64),
                UnixNanos::from(origin.elapsed().as_nanos() as u64),
            );
            sender.send(DataEvent::Data(Data::Trade(trade))).unwrap();
        }
    });

    let driver_handle = handle.clone();
    let driver_probe = probe.clone();
    let driver_received = received.clone();

    let driver = async move {
        wait_until_async(
            || async {
                driver_received.get() == samples
                    && (mode == CollectionMode::Baseline
                        || driver_probe.borrow().completed == cycles + 1)
            },
            DEADLINE,
        )
        .await;

        driver_handle.stop();
    };

    let (result, ()) = tokio::time::timeout(DEADLINE, async { tokio::join!(node.run(), driver) })
        .await
        .unwrap();
    result.unwrap();
    producer.join().unwrap();
    assert_eq!(received.get(), samples);
    assert_eq!(state.trade_queries.lock().await.len(), 0);
    let probe = probe.borrow();

    let expected_cycles = if mode == CollectionMode::Baseline {
        0
    } else {
        cycles + 1
    };

    assert_eq!(probe.completed, expected_cycles);
    assert_eq!(
        state.orders_get_count.load(Ordering::Acquire),
        (expected_cycles + 1) * orders.div_ceil(PAGE_SIZE)
    );
    let cache = node.kernel().cache();
    let cache = cache.borrow();
    assert_eq!(
        cache.orders_total_count(None, None, None, None, None),
        orders
    );

    for index in 0..orders {
        let id = format!("O-BENCH-{index:06}");
        let order = cache.order(&id.as_str().into()).unwrap();
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.filled_qty(), Quantity::from("0.0000"));
        assert_eq!(order.quantity(), Quantity::from("100.0000"));
        assert_eq!(order.price(), Some(Price::from("0.5000")));
        assert_eq!(order.venue_order_id(), Some(venue_order_id(index)));
    }

    let latencies = latencies.borrow();
    let output =
        std::env::temp_dir().join(format!("polymarket-reconciliation-{}.json", UUID4::new()));
    let summary = json!({
        "mode": mode.label(), "orders": orders, "pages_per_cycle": orders.div_ceil(PAGE_SIZE),
        "cycles": probe.durations.len(), "cycles_during_ticks": boundaries.get().1 - boundaries.get().0,
    "reports": expected_cycles * orders, "raw_samples": output.display().to_string(),
        "http_requests": state.orders_get_count.load(Ordering::Acquire), "market_data_samples": samples,
        "delivery_p50_us": percentile(latencies.iter().map(|sample| sample.0), 50),
        "delivery_p99_us": percentile(latencies.iter().map(|sample| sample.0), 99),
        "delivery_max_us": percentile(latencies.iter().map(|sample| sample.0), 100),
        "scheduled_p99_us": percentile(latencies.iter().map(|sample| sample.1), 99),
        "producer_p99_us": percentile(latencies.iter().map(|sample| sample.2), 99),
        "report_p50_us": percentile(probe.durations.iter().copied(), 50),
        "report_p99_us": percentile(probe.durations.iter().copied(), 99),
        });
    std::fs::write(
        &output,
        serde_json::to_vec(&json!({
            "summary": summary,
            "latencies_ns": &*latencies,
            "report_ns": probe.durations,
        }))
        .unwrap(),
    )
    .unwrap();
    println!("POLYMARKET_RECONCILIATION_BENCH {summary}");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectionMode {
    Baseline,
    Inline,
    Worker,
}

impl CollectionMode {
    fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Inline => "inline",
            Self::Worker => "worker",
        }
    }
}

#[derive(Debug)]
struct ReportProbe {
    orders: usize,
    limit: usize,
    completed: usize,
    durations: Vec<u64>,
    ready: Arc<AtomicBool>,
}

impl ReportProbe {
    fn record(&mut self, started: Instant, reports: &[OrderStatusReport]) {
        let elapsed = started.elapsed().as_nanos() as u64;
        assert_eq!(reports.len(), self.orders);

        if self.completed == 0 {
            for (index, report) in reports.iter().enumerate() {
                assert_eq!(report.account_id, AccountId::from(harness::ACCOUNT_ID));
                assert_eq!(
                    report.instrument_id,
                    InstrumentId::from(harness::INSTRUMENT_ID)
                );
                assert_eq!(report.venue_order_id, venue_order_id(index));
                assert_eq!(
                    report.client_order_id,
                    Some(format!("O-BENCH-{index:06}").as_str().into())
                );
                assert_eq!(report.order_side, Some(OrderSide::Buy));
                assert_eq!(report.order_type, OrderType::Limit);
                assert_eq!(report.time_in_force, TimeInForce::Gtc);
                assert_eq!(report.order_status, OrderStatus::Accepted);
                assert_eq!(report.quantity, Quantity::from("100.0000"));
                assert_eq!(report.filled_qty, Quantity::from("0.0000"));
                assert_eq!(report.price, Some(Price::from("0.5000")));
                assert_eq!(report.avg_px, None);
            }

            self.ready.store(true, Ordering::Release);
        } else {
            self.durations.push(elapsed);
        }

        self.completed += 1;
    }
}

#[derive(Debug)]
struct ReportFactory {
    mode: CollectionMode,
    probe: Rc<RefCell<ReportProbe>>,
}

impl ExecutionClientFactory for ReportFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let client =
            PolymarketExecutionClientFactory.create(trader_id, name, config, cache, clock)?;
        Ok(Box::new(ReportClient {
            client,
            mode: self.mode,
            probe: self.probe.clone(),
        }))
    }

    fn name(&self) -> &str {
        POLYMARKET
    }

    fn config_type(&self) -> &'static str {
        "PolymarketExecutionClientConfig"
    }
}

struct ReportClient {
    client: Box<dyn ExecutionClient>,
    mode: CollectionMode,
    probe: Rc<RefCell<ReportProbe>>,
}

#[async_trait(?Send)]
impl ExecutionClient for ReportClient {
    fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.client.client_id()
    }

    fn account_id(&self) -> AccountId {
        self.client.account_id()
    }

    fn venue(&self) -> Venue {
        self.client.venue()
    }

    fn oms_type(&self) -> OmsType {
        self.client.oms_type()
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.client.get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.client
            .generate_account_state(balances, margins, reported, ts_event, info)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.client.start()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.client.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.connect().await
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.disconnect().await
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.client.on_instrument(instrument);
    }

    fn generate_order_status_reports_task(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        if self.mode != CollectionMode::Worker
            || self.probe.borrow().completed == self.probe.borrow().limit
        {
            return None;
        }

        let started = Instant::now();
        let core_thread = std::thread::current().id();
        let task = self.client.generate_order_status_reports_task(cmd).unwrap();
        let probe = self.probe.clone();
        Some(ExecutionReportTask {
            collection: Box::pin(async move {
                task.collection.await;
                assert_ne!(std::thread::current().id(), core_thread);
            }),
            finish: Box::new(move || {
                assert_eq!(std::thread::current().id(), core_thread);
                let reports = (task.finish)()?;
                probe.borrow_mut().record(started, &reports);
                Ok(reports)
            }),
        })
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        if self.probe.borrow().completed == self.probe.borrow().limit {
            return std::future::pending().await;
        }

        let started = Instant::now();
        let reports = self.client.generate_order_status_reports(cmd).await?;
        self.probe.borrow_mut().record(started, &reports);
        Ok(reports)
    }
}

async fn populate_pages(state: &TestServerState, orders: usize) {
    let template = load_json("http_open_order.json");
    let mut pages = state.orders_response_pages.lock().await;

    for page in 0..orders.div_ceil(PAGE_SIZE) {
        let rows = (page * PAGE_SIZE..orders.min((page + 1) * PAGE_SIZE))
            .map(|index| {
                let mut row = template.clone();
                row["id"] = json!(venue_order_id(index).as_str());
                row["original_size"] = json!("100.0000");
                row["size_matched"] = json!("0.0000");
                row
            })
            .collect::<Vec<_>>();

        let cursor = if page == 0 {
            "MA==".to_string()
        } else {
            format!("bench-{page}")
        };

        let next = if (page + 1) * PAGE_SIZE >= orders {
            "LTE=".to_string()
        } else {
            format!("bench-{}", page + 1)
        };

        pages.insert(cursor, json!({"data": rows, "next_cursor": next}));
    }
}

fn seed_orders(node: &LiveNode, instrument: &InstrumentAny, orders: usize) {
    let cache = node.kernel().cache();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    node.kernel()
        .exec_engine
        .borrow_mut()
        .get_client_adapter_mut(&ClientId::from(POLYMARKET))
        .unwrap()
        .on_instrument(instrument.clone());
    for index in 0..orders {
        let mut order = harness::limit_order(instrument.id(), &format!("O-BENCH-{index:06}"));
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        let submitted =
            TestOrderEventStubs::submitted(&order, AccountId::from(harness::ACCOUNT_ID));
        order = cache.borrow_mut().update_order(&submitted).unwrap();
        let accepted = TestOrderEventStubs::accepted(
            &order,
            AccountId::from(harness::ACCOUNT_ID),
            venue_order_id(index),
        );
        cache.borrow_mut().update_order(&accepted).unwrap();
    }
}

fn venue_order_id(index: usize) -> VenueOrderId {
    VenueOrderId::from(format!("0x{index:064x}").as_str())
}

fn percentile(values: impl Iterator<Item = u64>, percent: usize) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }

    values.sort_unstable();
    let index = (values.len() * percent).div_ceil(100).saturating_sub(1);
    values[index] as f64 / 1_000.0
}
