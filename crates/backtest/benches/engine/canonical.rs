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
    fmt::Debug,
    fs::File,
    hint::black_box,
    io::{BufRead, BufReader},
    time::{Duration, Instant},
};

use anyhow::Context;
use jiff::Timestamp;
use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
    result::CanonicalBacktestResult,
};
use nautilus_common::{actor::DataActor, logging::logger::LoggerConfig, timer::TimeEvent};
use nautilus_core::{UnixNanos, paths::get_test_data_path};
use nautilus_indicators::{
    average::ema::ExponentialMovingAverage,
    indicator::{Indicator, MovingAverage},
};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, Data, DataBatch, QuoteTick},
    enums::{
        AccountType, AggregationSource, BarAggregation, BookType, OmsType, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, StrategyId, Venue},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Currency, Money, Price, Quantity, fixed::HIGH_PRECISION_MODE},
};
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rust_decimal::Decimal;
use serde_json::Value;

const DATA_FILE: &str = "btc-perp-20211231-20220201_1m.csv";
const DATA_ROWS: usize = 10_000;
const SCHEDULED_ACTIONS: usize = 64;
const TRADE_SIZE: &str = "0.010";
const EMA_FAST_PERIOD: usize = 10;
const EMA_SLOW_PERIOD: usize = 20;

pub(crate) const SCENARIOS: [CanonicalScenario; 4] = [
    CanonicalScenario::Replay,
    CanonicalScenario::ScheduledMarketOrders,
    CanonicalScenario::PassiveLimitOrders,
    CanonicalScenario::BarEmaCross,
];

#[allow(dead_code, reason = "iterated by the Criterion benchmark target")]
pub(crate) const INPUTS: [CanonicalInput; 2] = [CanonicalInput::Legacy, CanonicalInput::Typed];

// Legacy submits one interleaved `Vec<Data>` through `add_data`; Typed submits a quote batch and a
// bar batch through `add_data_batch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalInput {
    Legacy,
    Typed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalScenario {
    Replay,
    ScheduledMarketOrders,
    PassiveLimitOrders,
    BarEmaCross,
}

impl CanonicalScenario {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Replay => "replay_only",
            Self::ScheduledMarketOrders => "scheduled_market_orders",
            Self::PassiveLimitOrders => "passive_limit_orders",
            Self::BarEmaCross => "bar_ema_cross",
        }
    }

    // Legacy cases keep the bare scenario name so saved baselines stay comparable.
    pub(crate) fn case_name(self, input: CanonicalInput) -> String {
        match input {
            CanonicalInput::Legacy => self.name().to_string(),
            CanonicalInput::Typed => format!("{}_typed", self.name()),
        }
    }

    pub(crate) fn build_engine(self, fixture: &CanonicalFixture) -> anyhow::Result<BacktestEngine> {
        let config = BacktestEngineConfig {
            logging: LoggerConfig::from_spec("bypass_logging")?,
            bypass_logging: true,
            run_analysis: false,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config)?;
        engine.add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .queue_position(true)
                .build()?,
        )?;
        engine.add_instrument(&canonical_instrument())?;

        match self {
            Self::Replay => {}
            Self::ScheduledMarketOrders => {
                engine.add_strategy(ScheduledOrders::market(fixture.action_times()))?;
            }
            Self::PassiveLimitOrders => {
                engine.add_strategy(ScheduledOrders::passive(fixture.action_times()))?;
            }
            Self::BarEmaCross => {
                engine.add_strategy(BarEmaCross::new(fixture.bar_type))?;
            }
        }

        match &fixture.data {
            CanonicalData::Legacy(data) => engine.add_data(data.clone(), None, true, true)?,
            CanonicalData::Typed { quotes, bars } => {
                engine.add_data_batch(DataBatch::from(quotes.clone()), None, true, true)?;
                engine.add_data_batch(DataBatch::from(bars.clone()), None, true, true)?;
            }
        }
        Ok(engine)
    }

    pub(crate) fn run(self, fixture: &CanonicalFixture) -> anyhow::Result<CanonicalBacktestResult> {
        let mut engine = self.build_engine(fixture)?;
        engine.run(None, None, None, false)?;
        let result = engine.get_canonical_result()?;
        engine.dispose();
        Ok(result)
    }

    pub(crate) fn verify(self, result: &CanonicalBacktestResult) -> anyhow::Result<()> {
        let actual = CanonicalFingerprint::from_result(result)?;
        let expected = self.expected();
        anyhow::ensure!(
            actual == expected,
            "canonical workload '{}' fingerprint mismatch\nexpected: {expected:#?}\nactual: {actual:#?}",
            self.name(),
        );
        Ok(())
    }

    fn expected(self) -> CanonicalFingerprint {
        match self {
            Self::Replay => CanonicalFingerprint {
                data_events: 20_000,
                execution_events: 0,
                orders_submitted: 0,
                orders_rejected: 0,
                fills: 0,
                cancels: 0,
                positions: 0,
                accounts: 1,
                account_digest:
                    "blake3:405c9f219087dbff0f0641cfdc5fc41c84296981f4158bc9ea0978a1c81aee2c"
                        .to_string(),
                result_digest:
                    "blake3:7dd768c892f560c967c2aae291c6b9315d619bafab1b43b6a9f80cfad81e6914"
                        .to_string(),
            },
            Self::ScheduledMarketOrders => CanonicalFingerprint {
                data_events: 20_000,
                execution_events: 128,
                orders_submitted: 64,
                orders_rejected: 0,
                fills: 64,
                cancels: 0,
                positions: 32,
                accounts: 1,
                account_digest:
                    "blake3:be57c858fd2d34e157342f64260c4d517abc2f7a5f67dce73d43858ea1b2bf1a"
                        .to_string(),
                result_digest: expected_result_digest(
                    "blake3:14fee63698f0b3afd4ed0fe0710733a2c677929efc975d82456c64d4e9e732c1",
                    "blake3:c2cf16fff2bc490e553c6c2958e4e1d66a3d4d7ddbe413752bd542fcfd7d100e",
                ),
            },
            Self::PassiveLimitOrders => CanonicalFingerprint {
                data_events: 20_000,
                execution_events: 192,
                orders_submitted: 64,
                orders_rejected: 0,
                fills: 0,
                cancels: 64,
                positions: 0,
                accounts: 1,
                account_digest:
                    "blake3:7702ff4aa9ca1d26061419e9185a5bcfed0418fb0f24725ee36fd7d4323d79f5"
                        .to_string(),
                result_digest:
                    "blake3:4aaa0bdd015b1d4df4c1c850d3f1527f3d099ac717dd38500625c1d0ed24b403"
                        .to_string(),
            },
            Self::BarEmaCross => CanonicalFingerprint {
                data_events: 20_000,
                execution_events: 900,
                orders_submitted: 450,
                orders_rejected: 0,
                fills: 450,
                cancels: 0,
                positions: 225,
                accounts: 1,
                account_digest:
                    "blake3:ba1b5311a979bcfb6b58a4a9c478b4f00d0c577ce0c5c9cb3328f84ee921d9fc"
                        .to_string(),
                result_digest: expected_result_digest(
                    "blake3:e62f1f8cb2f77dc5c118b02ae0d9481236d1fd69a7f26928482e2406d9556a1e",
                    "blake3:d269453558b68dd3dae266c1ab44fa454fdb034678fc383eb4f2b3edb4156bc1",
                ),
            },
        }
    }
}

fn expected_result_digest(standard: &str, high_precision: &str) -> String {
    if HIGH_PRECISION_MODE != 0 {
        high_precision.to_string()
    } else {
        standard.to_string()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CanonicalFixture {
    data: CanonicalData,
    bar_type: BarType,
    timestamps: Vec<UnixNanos>,
}

#[derive(Debug, Clone)]
enum CanonicalData {
    Legacy(Vec<Data>),
    Typed {
        quotes: Vec<QuoteTick>,
        bars: Vec<Bar>,
    },
}

impl CanonicalData {
    fn with_capacity(input: CanonicalInput, rows: usize) -> Self {
        match input {
            CanonicalInput::Legacy => Self::Legacy(Vec::with_capacity(rows * 2)),
            CanonicalInput::Typed => Self::Typed {
                quotes: Vec::with_capacity(rows),
                bars: Vec::with_capacity(rows),
            },
        }
    }

    fn push(&mut self, quote: QuoteTick, bar: Bar) {
        match self {
            Self::Legacy(data) => {
                data.push(Data::Quote(quote));
                data.push(Data::Bar(bar));
            }
            Self::Typed { quotes, bars } => {
                quotes.push(quote);
                bars.push(bar);
            }
        }
    }

    const fn len(&self) -> usize {
        match self {
            Self::Legacy(data) => data.len(),
            Self::Typed { quotes, bars } => quotes.len() + bars.len(),
        }
    }
}

impl CanonicalFixture {
    fn load(input: CanonicalInput) -> anyhow::Result<Self> {
        let path = get_test_data_path().join(DATA_FILE);
        let file = File::open(&path)
            .with_context(|| format!("failed to open canonical data at {}", path.display()))?;
        let mut lines = BufReader::new(file).lines();
        anyhow::ensure!(
            lines.next().transpose()?.as_deref() == Some("timestamp,open,high,low,close,volume"),
            "canonical data header did not match the expected six-column format",
        );
        let instrument = canonical_instrument();
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let mut data = CanonicalData::with_capacity(input, DATA_ROWS);
        let mut timestamps = Vec::with_capacity(DATA_ROWS);

        for (index, line) in lines.take(DATA_ROWS).enumerate() {
            let line = line.with_context(|| format!("failed to read canonical bar row {index}"))?;
            let mut fields = line.split(',');
            let timestamp_raw = fields
                .next()
                .context("canonical bar row missing timestamp")?;
            let open = fields.next().context("canonical bar row missing open")?;
            let high = fields.next().context("canonical bar row missing high")?;
            let low = fields.next().context("canonical bar row missing low")?;
            let close = fields.next().context("canonical bar row missing close")?;
            let volume = fields.next().context("canonical bar row missing volume")?;
            anyhow::ensure!(
                fields.next().is_none(),
                "canonical bar row {index} contained more than six columns",
            );
            let timestamp = format!("{}Z", timestamp_raw.replace(' ', "T"))
                .parse::<Timestamp>()
                .with_context(|| {
                    format!("failed to parse canonical bar timestamp {timestamp_raw}")
                })?;
            let ts = UnixNanos::from(timestamp);
            let open = parse_price(open, price_precision, index)?;
            let high = parse_price(high, price_precision, index)?;
            let low = parse_price(low, price_precision, index)?;
            let close = parse_price(close, price_precision, index)?;
            let volume = parse_volume(volume, size_precision, index)?;
            // LAST bars do not seed bid and ask prices for market orders
            let quote = QuoteTick::new(instrument_id, close, close, volume, volume, ts, ts);
            let bar = Bar::new_checked(bar_type, open, high, low, close, volume, ts, ts)?;
            data.push(quote, bar);
            timestamps.push(ts);
        }
        anyhow::ensure!(
            timestamps.len() == DATA_ROWS,
            "canonical data contained {} rows, expected {DATA_ROWS}",
            timestamps.len(),
        );
        Ok(Self {
            data,
            bar_type,
            timestamps,
        })
    }

    #[allow(dead_code, reason = "called by the Criterion benchmark target")]
    pub(crate) const fn len(&self) -> usize {
        self.data.len()
    }

    fn action_times(&self) -> Vec<UnixNanos> {
        let stride = self.timestamps.len() / (SCHEDULED_ACTIONS + 1);
        (1..=SCHEDULED_ACTIONS)
            .map(|index| self.timestamps[index * stride])
            .collect()
    }
}

fn parse_price(value: &str, precision: u8, row: usize) -> anyhow::Result<Price> {
    let price = value
        .parse::<Price>()
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to parse canonical price '{value}' at row {row}"))?;
    anyhow::ensure!(
        price.precision <= precision,
        "canonical price '{value}' at row {row} exceeds instrument precision {precision}",
    );
    Ok(Price::from_raw(price.raw, precision))
}

fn parse_volume(value: &str, precision: u8, row: usize) -> anyhow::Result<Quantity> {
    let volume = value
        .parse::<Decimal>()
        .with_context(|| format!("failed to parse canonical volume '{value}' at row {row}"))?;
    Quantity::from_decimal_dp(volume, precision)
        .with_context(|| format!("failed to normalize canonical volume '{value}' at row {row}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalFingerprint {
    data_events: usize,
    execution_events: usize,
    orders_submitted: usize,
    orders_rejected: usize,
    fills: usize,
    cancels: usize,
    positions: usize,
    accounts: usize,
    account_digest: String,
    result_digest: String,
}

impl CanonicalFingerprint {
    fn from_result(result: &CanonicalBacktestResult) -> anyhow::Result<Self> {
        let value = result.as_value();
        let run = value["run"]
            .as_object()
            .context("canonical result run should be an object")?;
        let orders = value["orders"]
            .as_array()
            .context("canonical result orders should be an array")?;
        let mut orders_rejected = 0;
        let mut cancels = 0;

        for order in orders {
            let status = canonical_order_status(order)?;
            match status {
                "DENIED" | "REJECTED" => orders_rejected += 1,
                "CANCELED" => cancels += 1,
                _ => {}
            }
        }
        let accounts = value["accounts"]
            .as_array()
            .context("canonical result accounts should be an array")?;
        let account_bytes = serde_json::to_vec(accounts)?;
        let account_digest = format!("blake3:{}", blake3::hash(&account_bytes).to_hex());
        let result_digest = result.digest()?;

        Ok(Self {
            data_events: parse_count(run, "iterations")?,
            execution_events: parse_count(run, "total_events")?,
            orders_submitted: parse_count(run, "total_orders")?,
            orders_rejected,
            fills: value["fills"]
                .as_array()
                .context("canonical result fills should be an array")?
                .len(),
            cancels,
            positions: parse_count(run, "total_positions")?,
            accounts: accounts.len(),
            account_digest,
            result_digest,
        })
    }
}

fn canonical_order_status(order: &Value) -> anyhow::Result<&str> {
    order
        .as_object()
        .and_then(|order| order.values().next())
        .and_then(|payload| payload.get("core"))
        .and_then(|core| core.get("status"))
        .and_then(Value::as_str)
        .context("canonical order should contain a string core status")
}

fn parse_count(run: &serde_json::Map<String, Value>, field: &str) -> anyhow::Result<usize> {
    run.get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("canonical run field '{field}' should be a string"))?
        .parse()
        .with_context(|| format!("canonical run field '{field}' should be an integer"))
}

fn canonical_instrument() -> InstrumentAny {
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    instrument.raw_symbol = "BTCUSDT".into();
    instrument.base_currency = Currency::BTC();
    instrument.max_price = Some(Price::from("1000000.00"));
    InstrumentAny::CryptoPerpetual(instrument)
}

#[derive(Clone, Copy)]
enum ScheduledOrderKind {
    Market,
    Passive,
}

struct ScheduledOrders {
    core: StrategyCore,
    instrument_id: InstrumentId,
    times: Vec<UnixNanos>,
    kind: ScheduledOrderKind,
    submitted: usize,
}

impl ScheduledOrders {
    fn market(times: Vec<UnixNanos>) -> Self {
        Self::new(times, ScheduledOrderKind::Market, "CANONICAL-MARKET-001")
    }

    fn passive(times: Vec<UnixNanos>) -> Self {
        Self::new(times, ScheduledOrderKind::Passive, "CANONICAL-PASSIVE-001")
    }

    fn new(times: Vec<UnixNanos>, kind: ScheduledOrderKind, strategy_id: &str) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from(strategy_id)),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id: canonical_instrument().id(),
            times,
            kind,
            submitted: 0,
        }
    }

    fn submit_scheduled_order(&mut self) -> anyhow::Result<()> {
        let side = if self.submitted.is_multiple_of(2) {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let order = match self.kind {
            ScheduledOrderKind::Market => self.order().market(
                self.instrument_id,
                side,
                Quantity::from(TRADE_SIZE),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            ScheduledOrderKind::Passive => self.order().limit(
                self.instrument_id,
                side,
                Quantity::from(TRADE_SIZE),
                match side {
                    OrderSide::Buy => Price::from("30000.00"),
                    OrderSide::Sell => Price::from("70000.00"),
                },
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
                None,
                None,
            ),
        };
        self.submitted += 1;
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(ScheduledOrders);

impl Debug for ScheduledOrders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ScheduledOrders)).finish()
    }
}

impl DataActor for ScheduledOrders {
    fn on_start(&mut self) -> anyhow::Result<()> {
        for (index, time) in self.times.iter().copied().enumerate() {
            self.clock().set_time_alert_ns(
                format!("canonical-order-{index}").as_str(),
                time,
                None,
                None,
            )?;
        }
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        self.submit_scheduled_order()
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if matches!(self.kind, ScheduledOrderKind::Passive) {
            self.cancel_all_orders(self.instrument_id, None, None, true, None)?;
        }
        Ok(())
    }
}

struct BarEmaCross {
    core: StrategyCore,
    bar_type: BarType,
    instrument_id: InstrumentId,
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    prev_fast_above: Option<bool>,
}

impl BarEmaCross {
    fn new(bar_type: BarType) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("CANONICAL-BAR-EMA-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            bar_type,
            instrument_id: bar_type.instrument_id(),
            ema_fast: ExponentialMovingAverage::new(EMA_FAST_PERIOD, Some(PriceType::Last)),
            ema_slow: ExponentialMovingAverage::new(EMA_SLOW_PERIOD, Some(PriceType::Last)),
            prev_fast_above: None,
        }
    }

    fn enter(&mut self, side: OrderSide) -> anyhow::Result<()> {
        let order = self.order().market(
            self.instrument_id,
            side,
            Quantity::from(TRADE_SIZE),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(BarEmaCross);

impl Debug for BarEmaCross {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BarEmaCross)).finish()
    }
}

impl DataActor for BarEmaCross {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_bars(self.bar_type, None, None);
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.ema_fast.handle_bar(bar);
        self.ema_slow.handle_bar(bar);
        if !self.ema_fast.initialized() || !self.ema_slow.initialized() {
            return Ok(());
        }

        let fast_above = self.ema_fast.value() > self.ema_slow.value();
        if let Some(previous) = self.prev_fast_above {
            if fast_above && !previous {
                self.enter(OrderSide::Buy)?;
            } else if !fast_above && previous {
                self.enter(OrderSide::Sell)?;
            }
        }
        self.prev_fast_above = Some(fast_above);
        Ok(())
    }
}

pub(crate) fn verify_matrix() -> anyhow::Result<()> {
    let mut mismatches = Vec::new();

    for scenario in SCENARIOS {
        let legacy = verify_input(scenario, CanonicalInput::Legacy, &mut mismatches)?;
        let typed = verify_input(scenario, CanonicalInput::Typed, &mut mismatches)?;

        if let Some(divergence) = legacy.first_divergence(&typed) {
            mismatches.push(format!(
                "canonical workload '{}' differed between legacy and typed input: {divergence:?}",
                scenario.name(),
            ));
        }
    }
    anyhow::ensure!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    Ok(())
}

fn verify_input(
    scenario: CanonicalScenario,
    input: CanonicalInput,
    mismatches: &mut Vec<String>,
) -> anyhow::Result<CanonicalBacktestResult> {
    let case = scenario.case_name(input);
    let fixture = load_fixture(input)?;
    let preloaded = scenario.run(&fixture)?;
    if let Err(e) = scenario.verify(&preloaded) {
        mismatches.push(format!("[{case}] {e}"));
    }
    let loaded = scenario.run(&load_fixture(input)?)?;
    if let Err(e) = scenario.verify(&loaded) {
        mismatches.push(format!("[{case}] {e}"));
    }

    if let Some(divergence) = preloaded.first_divergence(&loaded) {
        mismatches.push(format!(
            "canonical workload '{case}' differed between preloaded and full paths: {divergence:?}",
        ));
    }
    Ok(preloaded)
}

pub(crate) fn load_fixture(input: CanonicalInput) -> anyhow::Result<CanonicalFixture> {
    CanonicalFixture::load(input)
}

#[allow(dead_code, reason = "called by the Criterion benchmark target")]
pub(crate) fn run_preloaded_iterations(
    iterations: u64,
    scenario: CanonicalScenario,
    fixture: &CanonicalFixture,
) -> Duration {
    let mut elapsed = Duration::ZERO;

    for _ in 0..iterations {
        let mut engine = scenario
            .build_engine(fixture)
            .expect("canonical preloaded engine should build");
        let started = Instant::now();
        engine
            .run(None, None, None, false)
            .expect("canonical preloaded workload should run");
        elapsed += started.elapsed();
        let result = engine
            .get_canonical_result()
            .expect("canonical preloaded result should project");
        scenario
            .verify(black_box(&result))
            .expect("canonical preloaded workload should retain its fingerprint");
        engine.dispose();
    }
    elapsed
}

#[allow(dead_code, reason = "called by the Criterion benchmark target")]
pub(crate) fn run_full_iterations(
    iterations: u64,
    scenario: CanonicalScenario,
    input: CanonicalInput,
) -> Duration {
    let mut elapsed = Duration::ZERO;

    for _ in 0..iterations {
        let started = Instant::now();
        let fixture = load_fixture(input).expect("canonical full workload fixture should load");
        let mut engine = scenario
            .build_engine(&fixture)
            .expect("canonical full workload engine should build");
        engine
            .run(None, None, None, false)
            .expect("canonical full workload should run");
        elapsed += started.elapsed();
        let result = engine
            .get_canonical_result()
            .expect("canonical full workload result should project");
        scenario
            .verify(black_box(&result))
            .expect("canonical full workload should retain its fingerprint");
        engine.dispose();
    }
    elapsed
}
