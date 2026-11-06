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
    collections::BTreeMap,
    fmt::Debug,
    fs::File,
    hint::black_box,
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::LazyLock,
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
use nautilus_execution::models::fee::{FeeModelAny, MakerTakerFeeModel};
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
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DATA_FILE: &str = "btc-perp-20211231-20220201_1m.csv";
const DATA_ROWS: usize = 10_000;
const SCHEDULED_ACTIONS: usize = 64;
const TRADE_SIZE: &str = "0.010";
const EMA_FAST_PERIOD: usize = 10;
const EMA_SLOW_PERIOD: usize = 20;
const NUMERICAL_ABSOLUTE_TOLERANCE: f64 = 1e-12;
const NUMERICAL_RELATIVE_TOLERANCE: f64 = 1e-12;

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
                // Matches the canonical instrument's defined rates; fingerprints pin this behavior.
                .fee_model(
                    FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.0002), dec!(0.0004)))
                        .into(),
                )
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
        let actual = CanonicalReference::from_value(result.as_value())?;
        actual
            .verify(self.expected())
            .with_context(|| format!("canonical workload '{}' result mismatch", self.name()))
    }

    fn expected(self) -> &'static CanonicalReference {
        static REFERENCES: LazyLock<[CanonicalReference; SCENARIOS.len()]> = LazyLock::new(|| {
            let references = if HIGH_PRECISION_MODE != 0 {
                [
                    include_str!("../../test_data/canonical/high/replay_only.json"),
                    include_str!("../../test_data/canonical/high/scheduled_market_orders.json"),
                    include_str!("../../test_data/canonical/high/passive_limit_orders.json"),
                    include_str!("../../test_data/canonical/high/bar_ema_cross.json"),
                ]
            } else {
                [
                    include_str!("../../test_data/canonical/standard/replay_only.json"),
                    include_str!("../../test_data/canonical/standard/scheduled_market_orders.json"),
                    include_str!("../../test_data/canonical/standard/passive_limit_orders.json"),
                    include_str!("../../test_data/canonical/standard/bar_ema_cross.json"),
                ]
            };

            references.map(|reference| {
                serde_json::from_str(reference)
                    .expect("canonical reference fixtures contain valid JSON")
            })
        });
        &REFERENCES[self as usize]
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
    Ok(Price::from_raw(price.raw(), precision))
}

fn parse_volume(value: &str, precision: u8, row: usize) -> anyhow::Result<Quantity> {
    let volume = value
        .parse::<Decimal>()
        .with_context(|| format!("failed to parse canonical volume '{value}' at row {row}"))?;
    Quantity::from_decimal_dp(volume, precision)
        .with_context(|| format!("failed to normalize canonical volume '{value}' at row {row}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    state_digest: String,
}

impl CanonicalFingerprint {
    fn from_value(value: &Value) -> anyhow::Result<Self> {
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
        let state_bytes = serde_json::to_vec(value)?;
        let state_digest = format!("blake3:{}", blake3::hash(&state_bytes).to_hex());

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
            state_digest,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CanonicalReference {
    high_precision: bool,
    fingerprint: CanonicalFingerprint,
    numerical_values: BTreeMap<String, String>,
}

impl CanonicalReference {
    fn from_value(value: &Value) -> anyhow::Result<Self> {
        let mut state = value.clone();
        let numerical_values = extract_numerical_values(&mut state)?;
        let fingerprint = CanonicalFingerprint::from_value(&state)?;
        Ok(Self {
            high_precision: HIGH_PRECISION_MODE != 0,
            fingerprint,
            numerical_values,
        })
    }

    fn verify(&self, expected: &Self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.high_precision == expected.high_precision,
            "canonical reference precision does not match the active precision mode"
        );
        anyhow::ensure!(
            self.fingerprint == expected.fingerprint,
            "exact state fingerprint mismatch\nexpected: {:#?}\nactual: {:#?}",
            expected.fingerprint,
            self.fingerprint,
        );
        anyhow::ensure!(
            self.numerical_values.len() == expected.numerical_values.len(),
            "canonical numerical field counts differ, expected {}, actual {}",
            expected.numerical_values.len(),
            self.numerical_values.len(),
        );

        for (path, expected) in &expected.numerical_values {
            let actual = self
                .numerical_values
                .get(path)
                .with_context(|| format!("canonical numerical field '{path}' is missing"))?;
            let expected = decode_numerical_value(expected)?;
            let actual = decode_numerical_value(actual)?;
            anyhow::ensure!(
                numerical_values_match(expected, actual),
                "canonical numerical field '{path}' differs, expected {expected:?}, actual {actual:?} \
                 (absolute tolerance {NUMERICAL_ABSOLUTE_TOLERANCE}, \
                 relative tolerance {NUMERICAL_RELATIVE_TOLERANCE})",
            );
        }

        Ok(())
    }
}

fn extract_numerical_values(value: &mut Value) -> anyhow::Result<BTreeMap<String, String>> {
    let mut numerical_values = BTreeMap::new();
    let statistics = value
        .get_mut("statistics")
        .context("canonical statistics are missing")?;

    for group in ["general", "returns"] {
        extract_numerical_map(
            &mut statistics[group],
            &format!("/statistics/{group}"),
            &mut numerical_values,
        )?;
    }

    let pnls = statistics["pnls"]
        .as_object_mut()
        .context("canonical PnL statistics should be an object")?;

    for (currency, statistics) in pnls {
        let currency = escape_pointer_token(currency);
        extract_numerical_map(
            statistics,
            &format!("/statistics/pnls/{currency}"),
            &mut numerical_values,
        )?;
    }

    let returns = statistics["returns_series"]
        .as_array_mut()
        .context("canonical returns series should be an array")?;

    for (index, entry) in returns.iter_mut().enumerate() {
        let value = entry
            .get_mut("value")
            .context("canonical return value is missing")?;
        extract_numerical_value(
            value,
            format!("/statistics/returns_series/{index}/value"),
            &mut numerical_values,
        )?;
    }

    for collection in ["positions", "position_snapshots"] {
        let positions = value[collection]
            .as_array_mut()
            .context("canonical positions should be an array")?;

        for (index, position) in positions.iter_mut().enumerate() {
            let value = position
                .get_mut("realized_return")
                .context("canonical position return is missing")?;
            extract_numerical_value(
                value,
                format!("/{collection}/{index}/realized_return"),
                &mut numerical_values,
            )?;
        }
    }

    Ok(numerical_values)
}

fn extract_numerical_map(
    value: &mut Value,
    prefix: &str,
    numerical_values: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let values = value
        .as_object_mut()
        .context("canonical numerical statistics should be an object")?;

    for (name, value) in values {
        let name = escape_pointer_token(name);
        extract_numerical_value(value, format!("{prefix}/{name}"), numerical_values)?;
    }

    Ok(())
}

fn extract_numerical_value(
    value: &mut Value,
    path: String,
    numerical_values: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    if value.is_null() {
        return Ok(());
    }

    let encoded = value
        .as_str()
        .with_context(|| format!("canonical numerical field '{path}' should be a string"))?;
    decode_numerical_value(encoded)
        .with_context(|| format!("invalid canonical numerical field '{path}'"))?;
    numerical_values.insert(path, encoded.to_string());
    *value = Value::Null;
    Ok(())
}

fn decode_numerical_value(encoded: &str) -> anyhow::Result<f64> {
    match encoded {
        "nan" => Ok(f64::NAN),
        "+inf" => Ok(f64::INFINITY),
        "-inf" => Ok(f64::NEG_INFINITY),
        _ => {
            anyhow::ensure!(
                encoded.len() == 16
                    && encoded
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "invalid canonical numerical encoding '{encoded}'"
            );
            let bits = u64::from_str_radix(encoded, 16)?;
            let value = f64::from_bits(bits);
            anyhow::ensure!(
                value.is_finite(),
                "non-finite canonical values require a named encoding"
            );
            Ok(value)
        }
    }
}

fn numerical_values_match(expected: f64, actual: f64) -> bool {
    if expected == actual || (expected.is_nan() && actual.is_nan()) {
        return true;
    }

    if !expected.is_finite() || !actual.is_finite() {
        return false;
    }

    let tolerance = NUMERICAL_ABSOLUTE_TOLERANCE
        .max(NUMERICAL_RELATIVE_TOLERANCE * expected.abs().max(actual.abs()));
    (expected - actual).abs() <= tolerance
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
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

    write_result(&case, &preloaded)?;

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

pub(crate) fn write_result(case: &str, result: &CanonicalBacktestResult) -> anyhow::Result<()> {
    if let Some(directory) = std::env::var_os("NAUTILUS_BENCH_RESULT_DIR") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join(format!("{case}.json")),
            serde_json::to_vec_pretty(result.as_value())?,
        )?;
        let reference = CanonicalReference::from_value(result.as_value())?;
        std::fs::write(
            directory.join(format!("{case}.reference.json")),
            serde_json::to_vec_pretty(&reference)?,
        )?;
    }

    Ok(())
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
