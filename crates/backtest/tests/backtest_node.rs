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

#![cfg(feature = "streaming")]

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use nautilus_backtest::{
    config::{
        BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig,
        NautilusDataType,
    },
    node::BacktestNode,
};
use nautilus_common::actor::{DataActor, DataActorCore};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BarSpecification, QuoteTick, TradeTick},
    enums::{AccountType, AggressorSide, BarAggregation, BookType, OmsType, OrderSide, PriceType},
    identifiers::{InstrumentId, StrategyId, TradeId},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore};
use rstest::*;
use tempfile::TempDir;
use ustr::Ustr;

fn create_catalog_with_quotes(
    instrument: &InstrumentAny,
    count: usize,
    base_ts: u64,
) -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    catalog.write_instruments(vec![instrument.clone()]).unwrap();

    let instrument_id = instrument.id();
    let quotes: Vec<QuoteTick> = (0..count)
        .map(|i| {
            let mid = 1000.0 + (i as f64 * 0.5);
            QuoteTick::new(
                instrument_id,
                Price::from(format!("{:.2}", mid - 0.05).as_str()),
                Price::from(format!("{:.2}", mid + 0.05).as_str()),
                Quantity::from("1.000"),
                Quantity::from("1.000"),
                UnixNanos::from(base_ts + i as u64 * 1_000_000_000),
                UnixNanos::from(base_ts + i as u64 * 1_000_000_000),
            )
        })
        .collect();

    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    (temp_dir, catalog_path)
}

fn create_catalog_with_quotes_and_trades(
    instrument: &InstrumentAny,
    quote_count: usize,
    trade_count: usize,
    base_ts: u64,
) -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    catalog.write_instruments(vec![instrument.clone()]).unwrap();

    let instrument_id = instrument.id();
    let quotes: Vec<QuoteTick> = (0..quote_count)
        .map(|i| {
            let mid = 1000.0 + (i as f64 * 0.5);
            QuoteTick::new(
                instrument_id,
                Price::from(format!("{:.2}", mid - 0.05).as_str()),
                Price::from(format!("{:.2}", mid + 0.05).as_str()),
                Quantity::from("1.000"),
                Quantity::from("1.000"),
                UnixNanos::from(base_ts + i as u64 * 1_000_000_000),
                UnixNanos::from(base_ts + i as u64 * 1_000_000_000),
            )
        })
        .collect();

    // Interleave trades at 500ms offsets from quotes
    let trades: Vec<TradeTick> = (0..trade_count)
        .map(|i| {
            let ts = base_ts + i as u64 * 1_000_000_000 + 500_000_000;
            TradeTick::new(
                instrument_id,
                Price::from(format!("{:.2}", 1000.0 + i as f64 * 0.5).as_str()),
                Quantity::from("0.500"),
                AggressorSide::Buyer,
                TradeId::from(format!("T{i}").as_str()),
                UnixNanos::from(ts),
                UnixNanos::from(ts),
            )
        })
        .collect();

    catalog.write_to_parquet(quotes, None, None, None).unwrap();
    catalog.write_to_parquet(trades, None, None, None).unwrap();

    (temp_dir, catalog_path)
}

fn binance_venue_config() -> BacktestVenueConfig {
    BacktestVenueConfig::new(
        Ustr::from("BINANCE"),
        OmsType::Netting,
        AccountType::Margin,
        BookType::L1_MBP,
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
        None,
        None,
        vec!["1_000_000 USDT".to_string()],
        None,
        None,
        None,
        None,
    )
}

fn data_config(catalog_path: &str, instrument_id: InstrumentId) -> BacktestDataConfig {
    BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path.to_string(),
        None,
        None,
        Some(instrument_id),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn run_config(
    catalog_path: &str,
    instrument_id: InstrumentId,
    chunk_size: Option<usize>,
) -> BacktestRunConfig {
    BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data_config(catalog_path, instrument_id)],
        BacktestEngineConfig::default(),
        chunk_size,
        None,
        None,
        None,
    )
}

struct CountingStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    quote_count: usize,
}

impl CountingStrategy {
    fn new(instrument_id: InstrumentId) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("COUNTING-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            quote_count: 0,
        }
    }
}

impl Deref for CountingStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for CountingStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Debug for CountingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CountingStrategy)).finish()
    }
}

impl DataActor for CountingStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quote_count += 1;
        Ok(())
    }
}

impl Strategy for CountingStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

struct MarketOrderStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    submitted: bool,
}

impl MarketOrderStrategy {
    fn new(instrument_id: InstrumentId) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("MARKET-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size: Quantity::from("0.100"),
            submitted: false,
        }
    }
}

impl Deref for MarketOrderStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for MarketOrderStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Debug for MarketOrderStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(MarketOrderStrategy)).finish()
    }
}

impl DataActor for MarketOrderStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        if !self.submitted {
            self.submitted = true;
            let order = self.core.order_factory().market(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            );
            self.submit_order(order, None, None)?;
        }
        Ok(())
    }
}

impl Strategy for MarketOrderStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

#[rstest]
fn test_new_rejects_empty_configs() {
    let result = BacktestNode::new(vec![]);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("At least one run config")
    );
}

#[rstest]
fn test_new_validates_venue_exists_for_instruments(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = BacktestRunConfig::new(
        None,
        vec![],
        vec![data_config(&catalog_path, instrument.id())],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let result = BacktestNode::new(vec![config]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No venue config"));
}

#[rstest]
fn test_new_validates_time_range(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let data = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path,
        None,
        None,
        Some(instrument.id()),
        None,
        Some(UnixNanos::from(5_000_000_000u64)),
        Some(UnixNanos::from(1_000_000_000u64)),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let result = BacktestNode::new(vec![config]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("start_time"));
}

#[rstest]
fn test_build_creates_engine(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    assert!(node.get_engine(&config_id).is_some());
    assert_eq!(node.get_engines().len(), 1);
}

#[rstest]
fn test_build_is_idempotent(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);
    let mut node = BacktestNode::new(vec![config]).unwrap();

    node.build().unwrap();
    assert_eq!(node.get_engines().len(), 1);

    node.build().unwrap();
    assert_eq!(node.get_engines().len(), 1);
}

#[rstest]
fn test_run_oneshot(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 10, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(CountingStrategy::new(instrument.id()))
        .unwrap();

    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 10);
}

#[rstest]
fn test_run_auto_builds(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);

    let mut node = BacktestNode::new(vec![config]).unwrap();

    // Don't call build() — run() should auto-build
    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 5);
}

#[rstest]
fn test_run_oneshot_with_time_bounds(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let base_ts = 1_000_000_000u64;
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 10, base_ts);

    let data = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path,
        None,
        None,
        Some(instrument.id()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        None,
        Some(UnixNanos::from(base_ts + 3_000_000_000)),
        Some(UnixNanos::from(base_ts + 7_000_000_000)),
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 5);
}

#[rstest]
fn test_run_oneshot_with_strategy(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 10, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(MarketOrderStrategy::new(instrument.id()))
        .unwrap();

    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].total_orders >= 1);
    assert!(results[0].total_positions >= 1);
}

#[rstest]
fn test_run_streaming(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 20, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), Some(5));

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 20);
}

#[rstest]
fn test_run_streaming_with_strategy(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 20, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), Some(10));
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(MarketOrderStrategy::new(instrument.id()))
        .unwrap();

    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].total_orders >= 1);
}

#[rstest]
fn test_dispose_clears_engines(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();
    assert_eq!(node.get_engines().len(), 1);

    node.dispose();
    assert_eq!(node.get_engines().len(), 0);
}

#[rstest]
fn test_load_catalog(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = data_config(&catalog_path, instrument.id());
    let catalog = BacktestNode::load_catalog(&config).unwrap();

    let instruments = catalog.query_instruments(None).unwrap();
    assert_eq!(instruments.len(), 1);
}

#[rstest]
fn test_load_data_config(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = data_config(&catalog_path, instrument.id());
    let data = BacktestNode::load_data_config(&config, None, None).unwrap();

    assert_eq!(data.len(), 5);
}

#[rstest]
fn test_load_data_config_with_time_bounds(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let base_ts = 1_000_000_000u64;
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 10, base_ts);

    let config = data_config(&catalog_path, instrument.id());
    let data = BacktestNode::load_data_config(
        &config,
        Some(UnixNanos::from(base_ts + 3_000_000_000)),
        Some(UnixNanos::from(base_ts + 6_000_000_000)),
    )
    .unwrap();

    assert_eq!(data.len(), 4);
}

#[rstest]
fn test_data_config_query_identifiers_simple() {
    let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
    let config = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        "/tmp/catalog".to_string(),
        None,
        None,
        Some(instrument_id),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let ids = config.query_identifiers().unwrap();
    assert_eq!(ids, vec!["ETH/USDT.BINANCE"]);
}

#[rstest]
fn test_data_config_query_identifiers_bar_with_spec() {
    let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
    let bar_spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);

    let config = BacktestDataConfig::new(
        NautilusDataType::Bar,
        "/tmp/catalog".to_string(),
        None,
        None,
        Some(instrument_id),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(bar_spec),
        None,
        None,
    );

    let ids = config.query_identifiers().unwrap();
    assert_eq!(ids, vec!["ETH/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL"]);
}

#[rstest]
fn test_data_config_query_identifiers_explicit_bar_types() {
    let config = BacktestDataConfig::new(
        NautilusDataType::Bar,
        "/tmp/catalog".to_string(),
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
        Some(vec![
            "ETH/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL".to_string(),
            "BTC/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL".to_string(),
        ]),
        None,
    );

    let ids = config.query_identifiers().unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids[0].contains("ETH/USDT"));
    assert!(ids[1].contains("BTC/USDT"));
}

#[rstest]
fn test_data_config_query_identifiers_multiple_instruments() {
    let config = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        "/tmp/catalog".to_string(),
        None,
        None,
        None,
        Some(vec![
            InstrumentId::from("ETH/USDT.BINANCE"),
            InstrumentId::from("BTC/USDT.BINANCE"),
        ]),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let ids = config.query_identifiers().unwrap();
    assert_eq!(ids.len(), 2);
}

#[rstest]
fn test_data_config_query_identifiers_none_when_empty() {
    let config = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        "/tmp/catalog".to_string(),
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
    );

    assert!(config.query_identifiers().is_none());
}

#[rstest]
fn test_data_config_get_instrument_ids_from_single() {
    let instrument_id = InstrumentId::from("ETH/USDT.BINANCE");
    let config = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        "/tmp/catalog".to_string(),
        None,
        None,
        Some(instrument_id),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let ids = config.get_instrument_ids().unwrap();
    assert_eq!(ids, vec![instrument_id]);
}

#[rstest]
fn test_data_config_get_instrument_ids_from_multiple() {
    let id1 = InstrumentId::from("ETH/USDT.BINANCE");
    let id2 = InstrumentId::from("BTC/USDT.BINANCE");
    let config = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        "/tmp/catalog".to_string(),
        None,
        None,
        None,
        Some(vec![id1, id2]),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let ids = config.get_instrument_ids().unwrap();
    assert_eq!(ids.len(), 2);
}

#[rstest]
fn test_run_config_generates_id() {
    let config = BacktestRunConfig::new(
        None,
        vec![],
        vec![],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    assert!(!config.id().is_empty());
}

#[rstest]
fn test_run_config_accepts_custom_id() {
    let config = BacktestRunConfig::new(
        Some("my-run-001".to_string()),
        vec![],
        vec![],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    assert_eq!(config.id(), "my-run-001");
}

#[rstest]
fn test_venue_config_defaults() {
    let config = binance_venue_config();

    assert_eq!(config.name(), Ustr::from("BINANCE"));
    assert_eq!(config.oms_type(), OmsType::Netting);
    assert_eq!(config.account_type(), AccountType::Margin);
    assert_eq!(config.book_type(), BookType::L1_MBP);
    assert!(!config.routing());
    assert!(!config.frozen_account());
    assert!(config.reject_stop_orders());
    assert!(config.support_gtd_orders());
    assert!(config.support_contingent_orders());
    assert!(config.use_position_ids());
    assert!(!config.use_random_ids());
    assert!(config.use_reduce_only());
    assert!(config.bar_execution());
    assert!(!config.bar_adaptive_high_low_ordering());
    assert!(config.trade_execution());
    assert!(!config.use_market_order_acks());
    assert!(!config.liquidity_consumption());
    assert!(!config.allow_cash_borrowing());
    assert_eq!(config.price_protection_points(), 0);
}

#[rstest]
fn test_dispose_on_completion_true(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let data = data_config(&catalog_path, instrument.id());
    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        Some(true),
        None,
        None,
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);
}

#[rstest]
fn test_dispose_on_completion_false(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let data = data_config(&catalog_path, instrument.id());
    let config = BacktestRunConfig::new(
        Some("test-keep".to_string()),
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        Some(false),
        None,
        None,
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();
    assert_eq!(results.len(), 1);

    assert!(node.get_engine("test-keep").is_some());
}

#[rstest]
fn test_generates_orders(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 10, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), None);
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(MarketOrderStrategy::new(instrument.id()))
        .unwrap();

    let results = node.run().unwrap();

    let result = &results[0];
    assert_eq!(result.run_config_id.as_deref(), Some(config_id.as_str()));
    assert!(result.run_id.is_some());
    assert!(result.run_started.is_some());
    assert!(result.run_finished.is_some());
    assert!(result.backtest_start.is_some());
    assert!(result.backtest_end.is_some());
    assert!(result.total_orders >= 1);
    assert!(result.total_positions >= 1);
}

#[rstest]
fn test_run_streaming_uneven_chunks(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 20, 1_000_000_000);

    // chunk_size=7 doesn't divide evenly into 20 (chunks: 7, 7, 6)
    let config = run_config(&catalog_path, instrument.id(), Some(7));

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 20);
}

#[rstest]
fn test_multiple_data_configs_mixed_types(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let base_ts = 1_000_000_000u64;
    let (_temp_dir, catalog_path) =
        create_catalog_with_quotes_and_trades(&instrument, 10, 10, base_ts);

    let quote_data = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path.clone(),
        None,
        None,
        Some(instrument.id()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let trade_data = BacktestDataConfig::new(
        NautilusDataType::TradeTick,
        catalog_path,
        None,
        None,
        Some(instrument.id()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![quote_data, trade_data],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    // Should process both quotes and trades (10 + 10 = 20)
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 20);
}

#[rstest]
fn test_multiple_run_configs_rejected() {
    let config1 = BacktestRunConfig::new(
        Some("run-1".to_string()),
        vec![],
        vec![],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );
    let config2 = BacktestRunConfig::new(
        Some("run-2".to_string()),
        vec![],
        vec![],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let result = BacktestNode::new(vec![config1, config2]);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Only one run config")
    );
}

#[rstest]
fn test_chunk_size_zero_rejected(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let config = run_config(&catalog_path, instrument.id(), Some(0));
    let mut node = BacktestNode::new(vec![config]).unwrap();

    let result = node.run();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("chunk_size"));
}

#[rstest]
fn test_get_instrument_ids_from_composite_bar_types() {
    let config = BacktestDataConfig::new(
        NautilusDataType::Bar,
        "/tmp/catalog".to_string(),
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
        Some(vec![
            "ETH/USDT.BINANCE-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL".to_string(),
        ]),
        None,
    );

    let ids = config.get_instrument_ids().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], InstrumentId::from("ETH/USDT.BINANCE"));
}

#[rstest]
fn test_get_instrument_ids_rejects_invalid_bar_types() {
    let config = BacktestDataConfig::new(
        NautilusDataType::Bar,
        "/tmp/catalog".to_string(),
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
        Some(vec!["not-a-valid-bar-type".to_string()]),
        None,
    );

    let result = config.get_instrument_ids();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid bar type"));
}

#[rstest]
fn test_data_config_time_bounds_intersect_with_run_bounds(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let base_ts = 1_000_000_000u64;
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 20, base_ts);

    // Data config restricts to [5s, 15s]
    let data = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path,
        None,
        None,
        Some(instrument.id()),
        None,
        Some(UnixNanos::from(base_ts + 5_000_000_000)),
        Some(UnixNanos::from(base_ts + 15_000_000_000)),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    // Run config restricts to [3s, 10s]
    // Effective range should be max(5,3)=5s to min(15,10)=10s → 6 data points
    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        None,
        Some(UnixNanos::from(base_ts + 3_000_000_000)),
        Some(UnixNanos::from(base_ts + 10_000_000_000)),
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 6);
}

#[rstest]
fn test_empty_catalog_data_handled_gracefully(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let base_ts = 1_000_000_000u64;
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, base_ts);

    // Query time range with no data (far in the future)
    let data = BacktestDataConfig::new(
        NautilusDataType::QuoteTick,
        catalog_path,
        None,
        None,
        Some(instrument.id()),
        None,
        Some(UnixNanos::from(999_000_000_000u64)),
        Some(UnixNanos::from(999_999_000_000u64)),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 0);
}

#[rstest]
fn test_l2_venue_without_book_data_rejected(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let (_temp_dir, catalog_path) = create_catalog_with_quotes(&instrument, 5, 1_000_000_000);

    let venue_config = BacktestVenueConfig::new(
        Ustr::from("BINANCE"),
        OmsType::Netting,
        AccountType::Margin,
        BookType::L2_MBP,
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
        None,
        None,
        vec!["1_000_000 USDT".to_string()],
        None,
        None,
        None,
        None,
    );

    // QuoteTick data only — no order book data for L2 venue
    let data = data_config(&catalog_path, instrument.id());
    let config = BacktestRunConfig::new(
        None,
        vec![venue_config],
        vec![data],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    let result = BacktestNode::new(vec![config]);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("no order book data configured")
    );
}

#[rstest]
fn test_l2_venue_with_unfiltered_book_data_accepted() {
    let venue_config = BacktestVenueConfig::new(
        Ustr::from("BINANCE"),
        OmsType::Netting,
        AccountType::Margin,
        BookType::L2_MBP,
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
        None,
        None,
        vec!["1_000_000 USDT".to_string()],
        None,
        None,
        None,
        None,
    );

    // Unfiltered OrderBookDelta config (no instrument_id) covers all venues
    let book_data = BacktestDataConfig::new(
        NautilusDataType::OrderBookDelta,
        "/tmp/catalog".to_string(),
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
    );

    let config = BacktestRunConfig::new(
        None,
        vec![venue_config],
        vec![book_data],
        BacktestEngineConfig::default(),
        None,
        None,
        None,
        None,
    );

    // Should not error — unfiltered book data satisfies L2 requirement
    assert!(BacktestNode::new(vec![config]).is_ok());
}

#[rstest]
fn test_streaming_same_timestamp_events(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let temp_dir = TempDir::new().unwrap();
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    catalog.write_instruments(vec![instrument.clone()]).unwrap();

    let instrument_id = instrument.id();
    let base_ts = 1_000_000_000u64;

    // Create 12 quotes where groups of 3 share the same timestamp,
    // so chunk_size=5 would split a same-ts group without alignment
    let quotes: Vec<QuoteTick> = (0..12)
        .map(|i| {
            let ts = base_ts + (i / 3) as u64 * 1_000_000_000;
            let mid = 1000.0 + (i as f64 * 0.5);
            QuoteTick::new(
                instrument_id,
                Price::from(format!("{:.2}", mid - 0.05).as_str()),
                Price::from(format!("{:.2}", mid + 0.05).as_str()),
                Quantity::from("1.000"),
                Quantity::from("1.000"),
                UnixNanos::from(ts),
                UnixNanos::from(ts),
            )
        })
        .collect();

    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    let data = data_config(&catalog_path, instrument_id);
    let config = BacktestRunConfig::new(
        None,
        vec![binance_venue_config()],
        vec![data],
        BacktestEngineConfig::default(),
        Some(5),
        None,
        None,
        None,
    );

    let mut node = BacktestNode::new(vec![config]).unwrap();
    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, 12);
}
