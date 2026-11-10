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

//! Shared tick-to-trade workload for the engine sink and Bybit loopback benchmarks.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    enums::Environment,
    logging::logger::LoggerConfig,
    messages::execution::TradingCommand,
    msgbus::{self, BusTap, Endpoint, MStr, MessagingSwitchboard, Topic},
};
use nautilus_live::{
    config::{LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig},
    node::{LiveNode, NodeRunMode, NodeState},
};
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use serde_json::{Value, json};
#[cfg(feature = "python")]
use {
    nautilus_trading::{ImportableStrategyConfig, python::strategy::PyStrategy},
    pyo3::{prelude::*, types::PyDict},
    std::{collections::HashMap, ffi::CString},
};

pub(crate) const DATA_CLIENT: &str = "BENCH-DATA";
pub(crate) const EXEC_CLIENT: &str = "BENCH-EXEC";
pub(crate) const QUANTITY: &str = "0.010";
pub(crate) const SIZE_BID: &str = "1.250";
pub(crate) const SIZE_ASK: &str = "2.750";

#[derive(Clone, Copy, Debug)]
pub(crate) struct Case {
    pub(crate) quote: bool,
    pub(crate) bypass: bool,
    pub(crate) python: bool,
    pub(crate) stream: bool,
    pub(crate) diagnostic: bool,
    pub(crate) warmup: usize,
    pub(crate) samples: usize,
    pub(crate) period: Duration,
}

impl Case {
    pub(crate) fn new(quote: bool, bypass: bool, python: bool, stream: bool) -> Self {
        let read = |name: &str, default| {
            std::env::var(name).map_or(default, |value| value.parse::<usize>().unwrap())
        };

        let samples = read("NAUTILUS_TTT_SAMPLES", 2_000);
        let warmup = read("NAUTILUS_TTT_WARMUP", 200);
        let rate = read("NAUTILUS_TTT_RATE", 1_000);
        assert!(samples > 0 && rate > 0 && rate <= 1_000_000);
        assert!(samples.checked_add(warmup).unwrap() <= 1_000_000);

        Self {
            quote,
            bypass,
            python,
            stream,
            diagnostic: std::env::var_os("NAUTILUS_TTT_DIAGNOSTIC").is_some(),
            warmup,
            samples,
            period: Duration::from_nanos(1_000_000_000 / rate as u64),
        }
    }

    pub(crate) fn total(self) -> usize {
        self.warmup + self.samples
    }

    fn name(self) -> String {
        format!(
            "{}_{}_{}_{}_{}",
            if self.python { "python" } else { "rust" },
            if self.quote { "quote" } else { "trade" },
            if self.bypass { "bypass" } else { "checked" },
            if self.stream { "stream" } else { "single" },
            if self.diagnostic {
                "diagnostic"
            } else {
                "headline"
            },
        )
    }
}

#[derive(Debug)]
pub(crate) enum Observation {
    Sent {
        index: usize,
        scheduled: Instant,
        sent: Instant,
    },
    Received {
        index: usize,
        received: Instant,
        order: Value,
    },
}

pub(crate) fn node_config(case: Case) -> LiveNodeConfig {
    LiveNodeConfig {
        trader_id: TraderId::from("BENCH-001"),
        environment: Environment::Sandbox,
        risk_engine: LiveRiskEngineConfig {
            bypass: case.bypass,
            max_order_submit_rate: "1000000/00:00:01".to_string(),
            ..Default::default()
        },
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_interval_ms: 0,
            open_check_interval_secs: None,
            position_check_interval_secs: None,
            ..Default::default()
        },
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_secs(5),
        timeout_portfolio: Duration::from_secs(5),
        timeout_reconciliation: Duration::from_secs(5),
        timeout_disconnection: Duration::from_secs(5),
        timeout_shutdown: Duration::from_secs(5),
        ..Default::default()
    }
}

pub(crate) async fn measure<F>(
    mut node: LiveNode,
    instrument: InstrumentAny,
    case: Case,
    boundary: &str,
    boundary_source: &str,
    observations: tokio::sync::mpsc::UnboundedReceiver<Observation>,
    produce: F,
) where
    F: FnOnce(mpsc::Receiver<()>) -> thread::JoinHandle<()>,
{
    add_strategy(&mut node, instrument.id(), case);
    let handle = node.handle();
    let cache = node.kernel().cache();
    let timeline = Rc::new(Timeline::new(case.total()));
    if case.diagnostic {
        msgbus::set_bus_tap(timeline.clone());
    }

    let (ack, ack_rx) = mpsc::channel();

    let collect = async {
        while !handle.is_running() {
            assert_ne!(
                handle.state(),
                NodeState::Stopped,
                "node stopped before the workload started"
            );
            tokio::task::yield_now().await;
        }

        let producer = produce(ack_rx);
        let samples = collect(case, observations, ack, cache.clone()).await;
        producer.join().unwrap();
        handle.stop();
        samples
    };

    let (result, samples) = tokio::time::timeout(Duration::from_secs(120), async {
        tokio::join!(node.run_with_mode(NodeRunMode::Hosted), collect)
    })
    .await
    .expect("tick-to-trade workload timed out");

    msgbus::clear_bus_tap();
    result.unwrap();
    let cache = cache.borrow();
    assert_eq!(
        cache.orders(None, None, None, None, None).len(),
        case.total()
    );

    for index in 0..case.total() {
        let order = cache.order_owned(&order_id(index)).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.instrument_id(), instrument.id());
        assert_eq!(order_snapshot(&order), expected_order(index));
    }

    drop(cache);
    assert_eq!(
        node.kernel().risk_engine.borrow().command_count(),
        case.total() as u64
    );
    write_results(case, boundary, boundary_source, &samples, &timeline);
}

async fn collect(
    case: Case,
    mut observations: tokio::sync::mpsc::UnboundedReceiver<Observation>,
    ack: mpsc::Sender<()>,
    cache: Rc<RefCell<Cache>>,
) -> Vec<Sample> {
    let mut sent = vec![None; case.total()];
    let mut received = vec![None; case.total()];
    let mut count = 0;
    while count < case.total() {
        let observation = tokio::time::timeout(Duration::from_secs(10), observations.recv())
            .await
            .unwrap_or_else(|_| panic!("received {count}/{} outbound orders", case.total()))
            .expect("observation sender stopped");

        match observation {
            Observation::Sent {
                index,
                scheduled,
                sent: at,
            } => {
                assert!(
                    sent[index].replace((scheduled, at)).is_none(),
                    "duplicate tick"
                );
            }
            Observation::Received {
                index,
                received: at,
                order,
            } => {
                assert_eq!(order, expected_order(index));
                assert!(
                    received[index].replace(at).is_none(),
                    "duplicate outbound order"
                );
                count += 1;

                if !case.stream {
                    wait_closed(&cache, index).await;
                    ack.send(()).unwrap();
                }
            }
        }
    }

    // A send can be observed after its order on a different producer thread
    while sent.iter().any(Option::is_none) {
        match observations.recv().await.unwrap() {
            Observation::Sent {
                index,
                scheduled,
                sent: at,
            } => {
                assert!(sent[index].replace((scheduled, at)).is_none());
            }
            Observation::Received { .. } => panic!("duplicate outbound order"),
        }
    }

    for index in 0..case.total() {
        wait_closed(&cache, index).await;
    }

    assert!(
        observations.try_recv().is_err(),
        "unexpected extra observation"
    );
    sent.into_iter()
        .zip(received)
        .enumerate()
        .map(|(index, (sent, received))| {
            let (scheduled, sent) = sent.unwrap();
            let received = received.unwrap();

            Sample {
                index,
                scheduled,
                sent,
                received,
            }
        })
        .collect()
}

async fn wait_closed(cache: &Rc<RefCell<Cache>>, index: usize) {
    let id = order_id(index);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if cache
                .borrow()
                .order(&id)
                .is_some_and(|order| order.is_closed())
            {
                return;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("order {id} did not close"));
}

pub(crate) fn produce<F>(case: Case, ack: mpsc::Receiver<()>, mut send: F) -> thread::JoinHandle<()>
where
    F: FnMut(usize, Instant) + Send + 'static,
{
    thread::spawn(move || {
        let origin = Instant::now();

        for index in 0..case.total() {
            let scheduled = if case.stream {
                origin + case.period * index as u32
            } else {
                Instant::now()
            };

            if let Some(delay) = scheduled.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            }

            send(index, scheduled);

            if !case.stream {
                ack.recv_timeout(Duration::from_secs(30))
                    .expect("outbound order timed out");
            }
        }
    })
}

pub(crate) fn price(index: usize) -> Price {
    Price::from(format!("{}.00", 3_000 + index % 10).as_str())
}

pub(crate) fn price_ask(index: usize) -> Price {
    Price::from(format!("{}.50", 3_001 + index % 10).as_str())
}

pub(crate) fn order_id(index: usize) -> ClientOrderId {
    ClientOrderId::from(format!("O-TTT-{index}"))
}

pub(crate) fn order_index(id: &str) -> usize {
    id.strip_prefix("O-TTT-").unwrap().parse().unwrap()
}

pub(crate) fn order_snapshot(order: &impl Order) -> Value {
    assert_eq!(order.order_type(), OrderType::Limit);
    json!({
        "id": order.client_order_id(),
        "side": order.order_side(),
        "quantity": order.quantity().as_decimal().normalize(),
        "price": order.price().unwrap().as_decimal().normalize(),
        "time_in_force": order.time_in_force(),
        "post_only": order.is_post_only(),
        "reduce_only": order.is_reduce_only(),
        "quote_quantity": order.is_quote_quantity(),
    })
}

fn expected_order(index: usize) -> Value {
    json!({
        "id": order_id(index), "side": OrderSide::Buy,
        "quantity": Quantity::from(QUANTITY).as_decimal().normalize(), "price": price(index).as_decimal().normalize(),
        "time_in_force": TimeInForce::Gtc, "post_only": false,
        "reduce_only": false, "quote_quantity": false,
    })
}

fn add_strategy(node: &mut LiveNode, instrument_id: InstrumentId, case: Case) {
    if case.python {
        #[cfg(feature = "python")]
        add_python_strategy(node, instrument_id, case.quote);
        #[cfg(not(feature = "python"))]
        panic!("Python strategy benchmarks require --features python");
    } else {
        node.add_strategy(TickStrategy {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("TickToTrade-001")),
                log_events: false,
                log_commands: false,
                ..Default::default()
            }),
            instrument_id,
            quote: case.quote,
            quantity: Quantity::from(QUANTITY),
            count: 0,
        })
        .unwrap();
    }
}

#[derive(Debug)]
struct TickStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    quote: bool,
    quantity: Quantity,
    count: usize,
}

impl DataActor for TickStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        if self.quote {
            self.subscribe_quotes(self.instrument_id, Some(ClientId::from(DATA_CLIENT)), None);
        } else {
            self.subscribe_trades(self.instrument_id, Some(ClientId::from(DATA_CLIENT)), None);
        }

        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.submit(quote.bid_price)
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        self.submit(trade.price)
    }
}

nautilus_strategy!(TickStrategy);

impl TickStrategy {
    fn submit(&mut self, price: Price) -> anyhow::Result<()> {
        let order = self.order().limit(
            self.instrument_id,
            OrderSide::Buy,
            self.quantity,
            price,
            Some(TimeInForce::Gtc),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(order_id(self.count)),
        );
        self.count += 1;
        self.submit_order(order, None, Some(ClientId::from(EXEC_CLIENT)), None)
    }
}

#[cfg(feature = "python")]
fn add_python_strategy(node: &mut LiveNode, instrument_id: InstrumentId, quote: bool) {
    Python::initialize();
    Python::attach(|py| {
        assert!(
            py.version_info() >= (3, 13),
            "Select the project interpreter through PYO3_PYTHON, linked Python was {}",
            Python::version_str(),
        );
        let module = PyModule::new(py, "_tick_to_trade").unwrap();
        module
            .add("StrategyConfig", py.get_type::<StrategyConfig>())
            .unwrap();
        let code = CString::new(include_str!("tick_to_trade.py")).unwrap();
        py.run(&code, Some(&module.dict()), None).unwrap();
        let strategy = module
            .getattr("_strategy_type")
            .unwrap()
            .call1((
                py.get_type::<PyStrategy>(),
                instrument_id,
                quote,
                Quantity::from(QUANTITY),
                (
                    ClientId::from(DATA_CLIENT),
                    ClientId::from(EXEC_CLIENT),
                    OrderSide::Buy,
                    py.get_type::<ClientOrderId>(),
                ),
            ))
            .unwrap();
        module.add("TickStrategy", strategy).unwrap();
        py.import("sys")
            .unwrap()
            .getattr("modules")
            .unwrap()
            .cast::<PyDict>()
            .unwrap()
            .set_item("_tick_to_trade", module)
            .unwrap();
    });

    node.add_python_strategy_from_config(&ImportableStrategyConfig {
        strategy_path: "_tick_to_trade:TickStrategy".to_string(),
        config_path: "_tick_to_trade:StrategyConfig".to_string(),
        config: HashMap::from([
            ("strategy_id".to_string(), json!("TickToTrade-001")),
            ("log_events".to_string(), json!(false)),
            ("log_commands".to_string(), json!(false)),
        ]),
    })
    .unwrap();
}

#[derive(Debug)]
struct Timeline {
    callbacks: Cell<usize>,
    stages: RefCell<Vec<[Option<Instant>; 5]>>,
}

impl Timeline {
    fn new(total: usize) -> Self {
        Self {
            callbacks: Cell::new(0),
            stages: RefCell::new(vec![[None; 5]; total]),
        }
    }
}

impl BusTap for Timeline {
    fn on_publish(&self, _topic: MStr<Topic>, message: &dyn Any) {
        if message.is::<QuoteTick>() || message.is::<TradeTick>() {
            let index = self.callbacks.get();
            self.stages.borrow_mut()[index][0] = Some(Instant::now());
            self.callbacks.set(index + 1);
        }
    }

    fn on_send(&self, endpoint: MStr<Endpoint>, message: &dyn Any) {
        let stage = if endpoint == MessagingSwitchboard::risk_engine_queue_execute() {
            1
        } else if endpoint == MessagingSwitchboard::risk_engine_execute() {
            2
        } else if endpoint == MessagingSwitchboard::exec_engine_queue_execute() {
            3
        } else if endpoint == MessagingSwitchboard::exec_engine_execute() {
            4
        } else {
            return;
        };

        if let Some(TradingCommand::SubmitOrder(command)) = message.downcast_ref::<TradingCommand>()
        {
            let index = order_index(command.client_order_id.as_str());
            assert!(
                self.stages.borrow_mut()[index][stage]
                    .replace(Instant::now())
                    .is_none()
            );
        }
    }
}

#[derive(Debug)]
struct Sample {
    index: usize,
    scheduled: Instant,
    sent: Instant,
    received: Instant,
}

fn write_results(
    case: Case,
    boundary: &str,
    boundary_source: &str,
    samples: &[Sample],
    timeline: &Timeline,
) {
    let rows = samples
        .iter()
        .skip(case.warmup)
        .map(|sample| {
            let mut row = json!({
                "index": sample.index,
                "latency_ns": elapsed(sample.sent, sample.received),
                "scheduled_ns": elapsed(sample.scheduled, sample.received),
                "producer_ns": elapsed(sample.scheduled, sample.sent),
            });

            if case.diagnostic {
                let stages = timeline.stages.borrow()[sample.index].map(Option::unwrap);

                let names = [
                    "ingress",
                    "strategy",
                    "risk_queue",
                    "risk",
                    "execution_queue",
                    "execution_and_adapter",
                ];

                let points = [
                    sample.sent,
                    stages[0],
                    stages[1],
                    stages[2],
                    stages[3],
                    stages[4],
                    sample.received,
                ];

                for (name, pair) in names.into_iter().zip(points.windows(2)) {
                    row[format!("{name}_ns")] = json!(elapsed(pair[0], pair[1]));
                }
            }

            row
        })
        .collect::<Vec<_>>();

    let mut metrics = json!({});
    for key in rows[0]
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| key.ends_with("_ns"))
    {
        metrics[key] = distribution(rows.iter().map(|row| row[key].as_u64().unwrap()).collect());
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("BENCHMARKING.md").is_file())
        .unwrap()
        .to_path_buf();
    let directory = std::env::var_os("NAUTILUS_TTT_OUTPUT")
        .map_or(root.join("target/tick-to-trade"), PathBuf::from);
    fs::create_dir_all(&directory).unwrap();
    let recorded_at = nanos(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
    let path = directory.join(format!(
        "{}-{}-{}-{recorded_at}.json",
        boundary,
        case.name(),
        std::process::id()
    ));
    #[cfg(feature = "python")]
    let python = case.python.then(|| Python::version_str().to_string());
    #[cfg(not(feature = "python"))]
    let python: Option<String> = None;
    let result = json!({
        "case": case.name(), "boundary": boundary, "samples": case.samples,
        "recorded_at_unix_ns": recorded_at,
        "warmup": case.warmup, "period_ns": nanos(case.period), "metrics": metrics,
        "revision": command_output("git", &["rev-parse", "HEAD"]),
        "patch": command_output("git", &["diff", "HEAD"]),
        "workload_source": include_str!("tick_to_trade.rs"),
        "python_source": include_str!("tick_to_trade.py"),
        "boundary_source": boundary_source,
        "executable": std::env::current_exe().unwrap(),
        "debug_assertions": cfg!(debug_assertions),
        "rustc": command_output("rustc", &["--version"]),
        "build_profile": std::env::var("NAUTILUS_TTT_PROFILE").ok(),
        "kernel_release": command_output("uname", &["-r"]),
        "python": python,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "cpu": fs::read_to_string("/proc/cpuinfo").ok(),
        "worker_threads": std::env::var("NAUTILUS_WORKER_THREADS").ok(),
        "fixed_precision": FIXED_PRECISION,
        "raw_samples": rows,
    });
    fs::write(&path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!(
        "{}",
        json!({"case": case.name(), "boundary": boundary, "metrics": metrics, "output": path})
    );
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => {
            eprintln!(
                "Metadata command {program} {args:?} failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
            );
            None
        }
        Err(e) => {
            eprintln!("Metadata command {program} {args:?} failed: {e}");
            None
        }
    }
}

fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap()
}

fn elapsed(start: Instant, end: Instant) -> u64 {
    nanos(
        end.checked_duration_since(start)
            .expect("timestamps are out of order"),
    )
}

fn distribution(mut samples: Vec<u64>) -> Value {
    samples.sort_unstable();
    let rank = |n: usize| samples[(samples.len() * n).div_ceil(1_000) - 1];
    json!({"count": samples.len(), "p50": rank(500), "p95": rank(950),
        "p99": rank(990), "p999": rank(999), "max": samples.last().unwrap()})
}

#[rstest::rstest]
fn percentiles_use_nearest_rank() {
    assert_eq!(
        distribution((1..=1_000).rev().collect()),
        json!({"count": 1000, "p50": 500, "p95": 950, "p99": 990, "p999": 999, "max": 1000})
    );
    assert_eq!(
        distribution(vec![7]),
        json!({"count": 1, "p50": 7, "p95": 7, "p99": 7, "p999": 7, "max": 7})
    );
}
