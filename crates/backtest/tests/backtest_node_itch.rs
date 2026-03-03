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

//! BacktestNode integration tests using AAPL ITCH L3 order book data.
//!
//! Requires both `streaming` (for BacktestNode) and `high-precision`
//! (for 128-bit ITCH parquet encoding) features.

#![cfg(all(feature = "streaming", feature = "high-precision"))]

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
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    throttler::RateLimit,
};
use nautilus_model::{
    data::QuoteTick,
    enums::{AccountType, BookType, OmsType, OrderSide},
    identifiers::{InstrumentId, StrategyId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Currency, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_testkit::common::{itch_aapl_equity, load_itch_aapl_deltas};
use nautilus_trading::{
    Strategy, StrategyConfig, StrategyCore,
    examples::strategies::{GridMarketMaker, GridMarketMakerConfig},
};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

// Subsample for CI (covers initial snapshot + active trading)
const CI_DELTA_LIMIT: usize = 50_000;
// Smaller limit for grid MM tests; order-intensive strategies are much slower
// than simple one-shot strategies in debug builds.
const CI_DELTA_LIMIT_GRID_MM: usize = 10_000;

fn create_itch_catalog(quotes: &[QuoteTick], instrument: &InstrumentAny) -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    catalog.write_instruments(vec![instrument.clone()]).unwrap();
    catalog
        .write_to_parquet(quotes.to_vec(), None, None, None)
        .unwrap();

    (temp_dir, catalog_path)
}

fn xnas_venue_config() -> BacktestVenueConfig {
    BacktestVenueConfig::new(
        Ustr::from("XNAS"),
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
        vec!["1_000_000 USD".to_string()],
        Some(Currency::from("USD")),
        None,
        None,
        None,
    )
}

fn quote_data_config(catalog_path: &str, instrument_id: InstrumentId) -> BacktestDataConfig {
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
            trade_size: Quantity::from("100"),
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
fn test_itch_node_oneshot() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT));
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas);
    let num_quotes = quotes.len();
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    let (_temp_dir, catalog_path) = create_itch_catalog(&quotes, &instrument);

    let config = BacktestRunConfig::new(
        None,
        vec![xnas_venue_config()],
        vec![quote_data_config(&catalog_path, instrument_id)],
        BacktestEngineConfig::default(),
        None,
        Some(false),
        None,
        None,
    );
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(MarketOrderStrategy::new(instrument_id))
        .unwrap();

    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, num_quotes);
    assert!(
        results[0].total_orders >= 1,
        "Expected at least 1 order, was {}",
        results[0].total_orders
    );
    assert!(
        results[0].total_positions >= 1,
        "Expected at least 1 position, was {}",
        results[0].total_positions
    );
}

#[rstest]
fn test_itch_node_streaming() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT));
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas);
    let num_quotes = quotes.len();
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    let (_temp_dir, catalog_path) = create_itch_catalog(&quotes, &instrument);

    // Stream in chunks of 500 quotes
    let config = BacktestRunConfig::new(
        None,
        vec![xnas_venue_config()],
        vec![quote_data_config(&catalog_path, instrument_id)],
        BacktestEngineConfig::default(),
        Some(500),
        Some(false),
        None,
        None,
    );
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(MarketOrderStrategy::new(instrument_id))
        .unwrap();

    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, num_quotes);
    assert!(
        results[0].total_orders >= 1,
        "Expected at least 1 order in streaming mode, was {}",
        results[0].total_orders
    );
}

#[rstest]
fn test_itch_node_grid_market_maker() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT_GRID_MM));
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas);
    let num_quotes = quotes.len();
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    let (_temp_dir, catalog_path) = create_itch_catalog(&quotes, &instrument);

    // Use an unrestricted throttle so the grid MM can place all orders without
    // hitting the default 100/sec limit on high-frequency ITCH data.
    let unlimited = RateLimit::new(1_000_000, 1_000_000_000);
    let engine_config = BacktestEngineConfig {
        risk_engine: Some(RiskEngineConfig {
            max_order_submit: unlimited.clone(),
            max_order_modify: unlimited,
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = BacktestRunConfig::new(
        None,
        vec![xnas_venue_config()],
        vec![quote_data_config(&catalog_path, instrument_id)],
        engine_config,
        None,
        Some(false),
        None,
        None,
    );
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let grid_config = GridMarketMakerConfig::new(instrument_id, Quantity::from("100"))
        .with_trade_size(Quantity::from("100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_skew_factor(0.01)
        .with_requote_threshold_bps(5);
    let strategy = GridMarketMaker::new(grid_config);

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine.add_strategy(strategy).unwrap();

    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, num_quotes);
    assert!(
        results[0].total_orders > 0,
        "Expected grid MM to place orders, was 0"
    );
}

#[rstest]
fn test_itch_node_streaming_grid_market_maker() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT_GRID_MM));
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas);
    let num_quotes = quotes.len();
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    let (_temp_dir, catalog_path) = create_itch_catalog(&quotes, &instrument);

    // Use an unrestricted throttle so the grid MM can place all orders without
    // hitting the default 100/sec limit on high-frequency ITCH data.
    let unlimited = RateLimit::new(1_000_000, 1_000_000_000);
    let engine_config = BacktestEngineConfig {
        risk_engine: Some(RiskEngineConfig {
            max_order_submit: unlimited.clone(),
            max_order_modify: unlimited,
            ..Default::default()
        }),
        ..Default::default()
    };

    // Stream in chunks of 1000
    let config = BacktestRunConfig::new(
        None,
        vec![xnas_venue_config()],
        vec![quote_data_config(&catalog_path, instrument_id)],
        engine_config,
        Some(1000),
        Some(false),
        None,
        None,
    );
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let grid_config = GridMarketMakerConfig::new(instrument_id, Quantity::from("100"))
        .with_trade_size(Quantity::from("100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_skew_factor(0.01)
        .with_requote_threshold_bps(5);
    let strategy = GridMarketMaker::new(grid_config);

    let engine = node.get_engine_mut(&config_id).unwrap();
    engine.add_strategy(strategy).unwrap();

    let results = node.run().unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].iterations, num_quotes);
    assert!(
        results[0].total_orders > 0,
        "Expected grid MM to place orders in streaming mode, was 0"
    );
}
