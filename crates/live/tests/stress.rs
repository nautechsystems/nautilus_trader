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

//! Stress harness for the Rust live node.
//!
//! # What this measures
//!
//! Each scenario stands up a real `LiveNode` (kernel, message bus, data engine,
//! risk engine, execution engine) with no data or execution clients attached,
//! then drives synthetic traffic directly into the runner's mpsc channels and
//! reads `MessageBus` counters before and after.
//!
//! - `trade_burst`: pushes `TRADE_BURST_COUNT` `TradeTick`s into the data event
//!   channel as fast as possible, drains, and reports end-to-end throughput
//!   from the bus counter deltas.
//! - `cancel_starvation`: alternates a batch of trade ticks with a single
//!   `CancelOrder`, repeatedly. The runner's biased `select!` prioritises
//!   exec commands over data events, so each cancel should be picked up on
//!   the next iteration regardless of how deep the trade backlog is.
//!   Reports cancel observation latency percentiles, timed
//!   `now - cancel.ts_init` (see "How the cancel latency is measured" below
//!   for the precise meaning).
//!
//! Output is a single `key=value` line per scenario (no JSON, no extra log
//! noise: the node is configured with `bypass_logging`).
//!
//! # How it stays single-threaded
//!
//! `MessageBus`, the engines, and the TLS-bound channel senders all live on
//! one thread. The harness uses `#[tokio::test(flavor = "current_thread")]`
//! and `tokio::join!` (not `tokio::spawn`) so the driver future and
//! `LiveNode::run` are polled cooperatively on the same thread. The driver
//! pushes events synchronously, then yields with `tokio::task::yield_now`
//! to let the runner drain.
//!
//! # How the cancel latency is measured
//!
//! The driver stamps `cancel.ts_init` from the kernel clock, then submits the
//! cancel and waits until `ExecutionEngine::command_count` advances. That
//! counter only ticks for trading commands, so it confirms the cancel itself
//! has been processed.
//!
//! The recorded `latency.cancel.*` is `now - cancel.ts_init` measured at the
//! moment the driver next regains control after the runner yields. On a
//! single-threaded `tokio::join!` runtime the runner can keep draining other
//! ready events (e.g. trades that follow the cancel into the same activation)
//! before yielding back, so the value is best read as cancel **observation
//! latency**: an upper bound on cancel dispatch plus any tail-end work the
//! runner did inside the same activation. Pin-point cancel-handler timing
//! would require an instrumentation hook inside the exec-command path; this
//! harness measures end-to-end "from when a strategy stamped the command to
//! when the runtime gives the strategy back the floor", which is the number a
//! strategy would care about.
//!
//! # Running
//!
//! These tests are `#[ignore]` so default `cargo test` does not run them.
//! Always use `--release`: a debug build is not representative.
//!
//! ```bash
//! cargo test --release -p nautilus-live --test stress \
//!     -- --ignored --nocapture --test-threads=1 stress_trade_burst
//! ```
//!
//! All scenarios under nextest for process isolation:
//!
//! ```bash
//! cargo nextest run --release -p nautilus-live --test stress --run-ignored=ignored-only
//! ```
//!
//! Process isolation matters because each scenario builds a node that
//! initialises global logging state.
//!
//! # Scale via env var
//!
//! `NAUTILUS_STRESS_SCALE=N` multiplies the burst counts by `N`. Default 1 keeps
//! the published baseline. Use a larger scale for `cargo flamegraph` so perf
//! collects enough samples for readable frames:
//!
//! ```bash
//! sudo sysctl kernel.perf_event_paranoid=1
//! NAUTILUS_STRESS_SCALE=100 cargo flamegraph --profile release-debugging \
//!     -p nautilus-live --test stress -o target/flamegraph/trade_burst.svg \
//!     -- --ignored --nocapture --test-threads=1 stress_trade_burst
//! ```

use std::time::{Duration, Instant};

use nautilus_common::{
    enums::Environment,
    live::{dst, runner::get_data_event_sender},
    logging::logger::LoggerConfig,
    messages::{
        DataEvent,
        execution::{CancelOrder, TradingCommand},
    },
    msgbus,
    runner::get_trading_cmd_sender,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    config::{LiveExecEngineConfig, LiveNodeConfig},
    node::{LiveNode, LiveNodeHandle},
};
use nautilus_model::{
    data::{Data, trade::TradeTick},
    enums::AggressorSide,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId},
    types::{Price, Quantity},
};

const TRADE_BURST_COUNT: usize = 100_000;
const CANCEL_STARVATION_COUNT: usize = 1_000;
const CANCEL_STARVATION_TRADES: usize = 200_000;
const TRADE_BATCH: usize = 1024;
const STABLE_DRAIN_ITERS: usize = 10;
const DRAIN_TICK: Duration = Duration::from_millis(1);

// Scale factor for burst counts, read from `NAUTILUS_STRESS_SCALE` (default 1).
// Bump this for `cargo flamegraph` so perf collects enough samples to make
// frames readable: at 997 Hz the default 100k-trade burst finishes in ~75 ms
// and produces ~75 samples spread across many frames. `NAUTILUS_STRESS_SCALE=100`
// yields ~7.5 s of runtime and ~7500 samples.
fn stress_scale() -> usize {
    std::env::var("NAUTILUS_STRESS_SCALE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

fn stress_config() -> LiveNodeConfig {
    let logging = LoggerConfig {
        bypass_logging: true,
        ..Default::default()
    };

    LiveNodeConfig {
        environment: Environment::Live,
        trader_id: TraderId::from("STRESS-001"),
        exec_engine: LiveExecEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::from_millis(50),
        logging,
        ..Default::default()
    }
}

fn sample_trade() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("EUR/USD.SIM"),
        price: Price::from("1.10000"),
        size: Quantity::from(100_000),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("123456"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    }
}

fn sample_cancel(seq: u64, ts_init: UnixNanos) -> CancelOrder {
    CancelOrder::new(
        TraderId::from("STRESS-001"),
        None,
        StrategyId::from("S-STRESS"),
        InstrumentId::from("EUR/USD.SIM"),
        ClientOrderId::from(format!("O-{seq:08}").as_str()),
        None,
        UUID4::new(),
        ts_init,
        None,
    )
}

#[derive(Clone, Copy)]
struct BusSnapshot {
    sent: u64,
    req: u64,
    res: u64,
    pub_: u64,
}

impl BusSnapshot {
    fn delta(&self, other: &Self) -> Self {
        Self {
            sent: self.sent.saturating_sub(other.sent),
            req: self.req.saturating_sub(other.req),
            res: self.res.saturating_sub(other.res),
            pub_: self.pub_.saturating_sub(other.pub_),
        }
    }
}

fn snapshot_bus() -> BusSnapshot {
    let bus = msgbus::get_message_bus();
    let bus = bus.borrow();
    BusSnapshot {
        sent: bus.sent_count(),
        req: bus.req_count(),
        res: bus.res_count(),
        pub_: bus.pub_count(),
    }
}

fn read_pub_count() -> u64 {
    msgbus::get_message_bus().borrow().pub_count()
}

// Yields cooperatively until the runner reports `Running`. Avoids
// `wait_until_async`, which sleeps via real `tokio::time` and panics under
// madsim where there is no real Tokio reactor.
async fn wait_until_running(handle: &LiveNodeHandle) {
    let mut iters = 0u32;

    while !handle.is_running() {
        dst::task::yield_now().await;
        iters += 1;
        assert!(iters < 100_000, "node failed to reach Running state");
    }
}

// Yields and sleeps until pub_count is unchanged for STABLE_DRAIN_ITERS
// consecutive samples spaced by DRAIN_TICK, used as a coarse drain barrier
// before snapshotting final counters.
async fn drain_until_stable() {
    let mut prev = read_pub_count();
    let mut stable = 0;
    while stable < STABLE_DRAIN_ITERS {
        dst::time::sleep(DRAIN_TICK).await;
        let cur = read_pub_count();
        if cur == prev {
            stable += 1;
        } else {
            stable = 0;
        }
        prev = cur;
    }
}

fn percentile(sorted_us: &[u128], pct: f64) -> u128 {
    if sorted_us.is_empty() {
        return 0;
    }
    let idx = ((sorted_us.len() as f64 - 1.0) * pct).round() as usize;
    sorted_us[idx.min(sorted_us.len() - 1)]
}

#[ignore]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(flavor = "current_thread")
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn stress_trade_burst() {
    let mut node = LiveNode::build("StressNode".to_string(), Some(stress_config())).unwrap();
    let handle = node.handle();

    let driver_handle = handle.clone();

    let driver = async move {
        wait_until_running(&driver_handle).await;

        let total = TRADE_BURST_COUNT * stress_scale();
        let before = snapshot_bus();
        let trade = sample_trade();
        let sender = get_data_event_sender();

        let start = Instant::now();
        let mut last_sample = start;
        let mut last_pub = before.pub_;
        let mut max_interval_rate = 0.0_f64;

        for i in 0..total {
            sender.send(DataEvent::Data(Data::Trade(trade))).unwrap();

            if (i + 1) % TRADE_BATCH == 0 {
                dst::task::yield_now().await;

                let now = Instant::now();
                let interval = now.duration_since(last_sample);
                if interval >= Duration::from_millis(100) {
                    let cur = read_pub_count();
                    let delta = cur.saturating_sub(last_pub) as f64;
                    let rate = delta / interval.as_secs_f64();
                    if rate > max_interval_rate {
                        max_interval_rate = rate;
                    }
                    last_sample = now;
                    last_pub = cur;
                }
            }
        }

        drain_until_stable().await;
        let elapsed = start.elapsed();
        let after = snapshot_bus();
        let delta = after.delta(&before);
        let mean_msg_s = total as f64 / elapsed.as_secs_f64();

        println!(
            "scenario=trade_burst messages={} elapsed_ms={} mean_msg_s={:.0} \
             max_interval_msg_s={:.0} counter.msgbus.sent={} counter.msgbus.pub={} \
             counter.msgbus.req={} counter.msgbus.res={}",
            total,
            elapsed.as_millis(),
            mean_msg_s,
            max_interval_rate,
            delta.sent,
            delta.pub_,
            delta.req,
            delta.res,
        );

        driver_handle.stop();
    };

    tokio::join!(driver, async {
        node.run().await.unwrap();
    });
}

#[ignore]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(flavor = "current_thread")
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn stress_cancel_starvation() {
    let mut node = LiveNode::build("StressNode".to_string(), Some(stress_config())).unwrap();
    let handle = node.handle();
    let clock = node.kernel().clock();
    let exec_engine = node.kernel().exec_engine().clone();

    let driver_handle = handle.clone();
    let driver_clock = clock.clone();
    let driver_exec_engine = exec_engine.clone();

    let driver = async move {
        wait_until_running(&driver_handle).await;

        let scale = stress_scale();
        let cancels = CANCEL_STARVATION_COUNT * scale;
        let total_trades = CANCEL_STARVATION_TRADES * scale;
        let before = snapshot_bus();
        let trade = sample_trade();
        let data_sender = get_data_event_sender();

        let start = Instant::now();
        let mut latencies_us: Vec<u128> = Vec::with_capacity(cancels);
        let mut trades_sent: usize = 0;
        let mut yield_iters_total: u64 = 0;
        let mut yield_iters_max: u32 = 0;
        let trade_step = total_trades / cancels;

        for seq in 0..cancels {
            for _ in 0..trade_step {
                data_sender
                    .send(DataEvent::Data(Data::Trade(trade)))
                    .unwrap();
                trades_sent += 1;
            }

            // Trigger on `ExecutionEngine::command_count` (incremented inside
            // `execute_command`), which only ticks for trading commands and
            // therefore confirms the cancel has been processed. The latency is
            // `now - cancel.ts_init` measured at the moment the driver next
            // gets control: on the single-threaded `tokio::join!` runtime, the
            // runner can keep draining ready events after the cancel before it
            // yields back, so the value is end-to-end observation latency, not
            // pure handler time. See the module doc for the full caveat.
            let pre_cmd = driver_exec_engine.borrow().command_count();
            let ts_init = driver_clock.borrow().timestamp_ns();
            get_trading_cmd_sender().execute(TradingCommand::CancelOrder(sample_cancel(
                seq as u64, ts_init,
            )));

            let mut yield_iters = 0u32;

            loop {
                dst::task::yield_now().await;
                yield_iters += 1;

                if driver_exec_engine.borrow().command_count() > pre_cmd {
                    break;
                }
            }
            let now = driver_clock.borrow().timestamp_ns();
            latencies_us.push(u128::from(now.as_u64() - ts_init.as_u64()) / 1_000);
            yield_iters_total += u64::from(yield_iters);
            if yield_iters > yield_iters_max {
                yield_iters_max = yield_iters;
            }
        }

        drain_until_stable().await;
        let total_elapsed = start.elapsed();
        let after = snapshot_bus();
        let delta = after.delta(&before);

        latencies_us.sort_unstable();
        let min = latencies_us.first().copied().unwrap_or(0);
        let p50 = percentile(&latencies_us, 0.50);
        let p95 = percentile(&latencies_us, 0.95);
        let p99 = percentile(&latencies_us, 0.99);
        let p999 = percentile(&latencies_us, 0.999);
        let max = latencies_us.last().copied().unwrap_or(0);

        let yield_iters_mean = yield_iters_total as f64 / cancels as f64;
        println!(
            "scenario=cancel_starvation cancels={} trades={} elapsed_ms={} \
             counter.msgbus.sent={} counter.msgbus.pub={} \
             latency.cancel.min_us={} latency.cancel.p50_us={} \
             latency.cancel.p95_us={} latency.cancel.p99_us={} \
             latency.cancel.p999_us={} latency.cancel.max_us={} \
             yield_iters.mean={:.1} yield_iters.max={}",
            cancels,
            trades_sent,
            total_elapsed.as_millis(),
            delta.sent,
            delta.pub_,
            min,
            p50,
            p95,
            p99,
            p999,
            max,
            yield_iters_mean,
            yield_iters_max,
        );

        driver_handle.stop();
    };

    tokio::join!(driver, async {
        node.run().await.unwrap();
    });
}
