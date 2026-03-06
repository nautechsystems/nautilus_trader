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

//! Provides a [`BacktestNode`] that orchestrates catalog-driven backtests.

use std::iter::Peekable;

use ahash::{AHashMap, AHashSet};
use nautilus_core::UnixNanos;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{
        Bar, Data, HasTsInit, IndexPriceUpdate, InstrumentClose, MarkPriceUpdate, OrderBookDelta,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{BookType, OtoTriggerMode},
    identifiers::{InstrumentId, Venue},
    types::Money,
};
use nautilus_persistence::backend::{catalog::ParquetDataCatalog, session::QueryResult};
use rust_decimal::{Decimal, prelude::FromPrimitive};

use crate::{
    config::{BacktestDataConfig, BacktestRunConfig, NautilusDataType},
    engine::BacktestEngine,
    result::BacktestResult,
};

/// Orchestrates catalog-driven backtests from run configurations.
///
/// `BacktestNode` connects the [`ParquetDataCatalog`] with [`BacktestEngine`] to load
/// historical data and run backtests. Supports both oneshot and streaming modes.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.backtest", unsendable)
)]
pub struct BacktestNode {
    configs: Vec<BacktestRunConfig>,
    engines: AHashMap<String, BacktestEngine>,
}

impl BacktestNode {
    /// Creates a new [`BacktestNode`] instance.
    ///
    /// Validates that configs are non-empty and internally consistent:
    /// - All data config instrument venues must have a matching venue config.
    /// - L2/L3 book types require order book data in the data configs.
    /// - Data config time ranges must be valid (start <= end).
    ///
    /// # Errors
    ///
    /// Returns an error if `configs` is empty or validation fails.
    pub fn new(configs: Vec<BacktestRunConfig>) -> anyhow::Result<Self> {
        anyhow::ensure!(!configs.is_empty(), "At least one run config is required");
        validate_configs(&configs)?;
        Ok(Self {
            configs,
            engines: AHashMap::new(),
        })
    }

    /// Returns the run configurations.
    #[must_use]
    pub fn configs(&self) -> &[BacktestRunConfig] {
        &self.configs
    }

    /// Builds backtest engines from the run configurations.
    ///
    /// For each config, creates a [`BacktestEngine`], adds venues, and loads
    /// instruments from the catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if engine creation, venue setup, or instrument loading fails.
    pub fn build(&mut self) -> anyhow::Result<()> {
        for config in &self.configs {
            if self.engines.contains_key(config.id()) {
                continue;
            }

            let engine_config = config.engine().clone();
            let mut engine = BacktestEngine::new(engine_config)?;

            for venue_config in config.venues() {
                let starting_balances: Vec<Money> = venue_config
                    .starting_balances()
                    .iter()
                    .map(|s| s.parse::<Money>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| anyhow::anyhow!("Invalid starting balance: {e}"))?;

                let default_leverage = venue_config.default_leverage().and_then(Decimal::from_f64);
                let leverages: AHashMap<InstrumentId, Decimal> = venue_config
                    .leverages()
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| {
                                Decimal::from_f64(*v).map(|d| (*k, d)).ok_or_else(|| {
                                    anyhow::anyhow!("Invalid leverage {v} for instrument {k}")
                                })
                            })
                            .collect::<anyhow::Result<AHashMap<_, _>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                engine.add_venue(
                    Venue::from(venue_config.name().as_str()),
                    venue_config.oms_type(),
                    venue_config.account_type(),
                    venue_config.book_type(),
                    starting_balances,
                    venue_config.base_currency(),
                    default_leverage,
                    leverages,
                    None, // margin_model
                    Vec::new(),
                    FillModelAny::default(),
                    FeeModelAny::default(),
                    None, // latency_model
                    Some(venue_config.routing()),
                    Some(venue_config.reject_stop_orders()),
                    Some(venue_config.support_gtd_orders()),
                    Some(venue_config.support_contingent_orders()),
                    Some(venue_config.use_position_ids()),
                    Some(venue_config.use_random_ids()),
                    Some(venue_config.use_reduce_only()),
                    None, // use_message_queue
                    Some(venue_config.use_market_order_acks()),
                    Some(venue_config.bar_execution()),
                    Some(venue_config.bar_adaptive_high_low_ordering()),
                    Some(venue_config.trade_execution()),
                    Some(venue_config.liquidity_consumption()),
                    Some(venue_config.allow_cash_borrowing()),
                    Some(venue_config.frozen_account()),
                    Some(venue_config.queue_position()),
                    Some(venue_config.oto_trigger_mode() == OtoTriggerMode::Full),
                    Some(venue_config.price_protection_points()),
                )?;
            }

            for data_config in config.data() {
                let catalog = create_catalog(data_config)?;
                let instr_ids: Vec<InstrumentId> = data_config.get_instrument_ids()?;
                let filter = if instr_ids.is_empty() {
                    None
                } else {
                    Some(instr_ids.iter().map(ToString::to_string).collect())
                };

                let instruments = catalog.query_instruments(filter)?;

                if !instr_ids.is_empty() && instruments.is_empty() {
                    let ids: Vec<String> = instr_ids.iter().map(ToString::to_string).collect();
                    anyhow::bail!(
                        "No instruments found in catalog for requested IDs: [{}]",
                        ids.join(", ")
                    );
                }

                for instrument in instruments {
                    engine.add_instrument(instrument)?;
                }
            }

            self.engines.insert(config.id().to_string(), engine);
        }

        Ok(())
    }

    /// Returns a mutable reference to the engine for the given run config ID.
    #[must_use]
    pub fn get_engine_mut(&mut self, id: &str) -> Option<&mut BacktestEngine> {
        self.engines.get_mut(id)
    }

    /// Returns a reference to the engine for the given run config ID.
    #[must_use]
    pub fn get_engine(&self, id: &str) -> Option<&BacktestEngine> {
        self.engines.get(id)
    }

    /// Returns all created backtest engines.
    #[must_use]
    pub fn get_engines(&self) -> Vec<&BacktestEngine> {
        self.engines.values().collect()
    }

    /// Runs all configured backtests and returns results.
    ///
    /// Automatically calls [`build()`](Self::build) if engines have not been created yet.
    /// For each run config, loads data from the catalog and runs the engine.
    /// Supports both oneshot (`chunk_size = None`) and streaming modes.
    ///
    /// # Errors
    ///
    /// Returns an error if building, data loading, or engine execution fails.
    pub fn run(&mut self) -> anyhow::Result<Vec<BacktestResult>> {
        // Auto-build if not already done
        if self.engines.is_empty() {
            self.build()?;
        }

        let mut results = Vec::new();

        for config in &self.configs {
            let engine = self.engines.get_mut(config.id()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Engine not found for config '{}'. Call build() first.",
                    config.id()
                )
            })?;

            match config.chunk_size() {
                None => run_oneshot(engine, config)?,
                Some(chunk_size) => {
                    anyhow::ensure!(chunk_size > 0, "chunk_size must be > 0");
                    run_streaming(engine, config, chunk_size)?;
                }
            }

            results.push(engine.get_result());

            if config.dispose_on_completion() {
                engine.dispose();
            } else {
                engine.clear_data();
            }
        }

        Ok(results)
    }

    /// Creates a [`ParquetDataCatalog`] from a data config.
    ///
    /// # Errors
    ///
    /// Returns an error if the catalog cannot be created from the URI.
    pub fn load_catalog(config: &BacktestDataConfig) -> anyhow::Result<ParquetDataCatalog> {
        create_catalog(config)
    }

    /// Loads data from the catalog for a specific data config.
    ///
    /// # Errors
    ///
    /// Returns an error if catalog creation or data querying fails.
    pub fn load_data_config(
        config: &BacktestDataConfig,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<Data>> {
        load_data(config, start, end)
    }

    /// Disposes all engines and releases resources.
    pub fn dispose(&mut self) {
        for engine in self.engines.values_mut() {
            engine.dispose();
        }
        self.engines.clear();
    }
}

fn validate_configs(configs: &[BacktestRunConfig]) -> anyhow::Result<()> {
    // Kernel initialization sets a thread-local MessageBus that can only be
    // initialized once per thread, so multiple engines cannot coexist
    anyhow::ensure!(
        configs.len() <= 1,
        "Only one run config per BacktestNode is supported \
         (kernel MessageBus is a thread-local singleton)"
    );

    let mut seen_ids = AHashSet::new();
    for config in configs {
        anyhow::ensure!(
            seen_ids.insert(config.id()),
            "Duplicate run config ID '{}'",
            config.id()
        );

        let venue_names: Vec<String> = config
            .venues()
            .iter()
            .map(|v| v.name().to_string())
            .collect();

        for data_config in config.data() {
            if let (Some(start), Some(end)) = (data_config.start_time(), data_config.end_time()) {
                anyhow::ensure!(
                    start <= end,
                    "Data config start_time ({start}) must be <= end_time ({end})"
                );
            }

            for instrument_id in data_config.get_instrument_ids()? {
                let venue = instrument_id.venue.to_string();
                anyhow::ensure!(
                    venue_names.contains(&venue),
                    "No venue config found for venue '{venue}' (required by instrument {instrument_id})"
                );
            }
        }

        for venue_config in config.venues() {
            let needs_book_data = matches!(
                venue_config.book_type(),
                BookType::L2_MBP | BookType::L3_MBO
            );

            if needs_book_data {
                let venue_name = venue_config.name().to_string();
                let has_book_data = config.data().iter().any(|dc| {
                    let is_book_type = matches!(
                        dc.data_type(),
                        NautilusDataType::OrderBookDelta | NautilusDataType::OrderBookDepth10
                    );

                    if !is_book_type {
                        return false;
                    }

                    // Unfiltered config (no instrument filter) covers all venues
                    let ids = dc.get_instrument_ids().unwrap_or_default();
                    ids.is_empty() || ids.iter().any(|id| id.venue.to_string() == venue_name)
                });
                anyhow::ensure!(
                    has_book_data,
                    "Venue '{venue_name}' has book_type {:?} but no order book data configured",
                    venue_config.book_type()
                );
            }
        }
    }
    Ok(())
}

fn run_oneshot(engine: &mut BacktestEngine, config: &BacktestRunConfig) -> anyhow::Result<()> {
    for data_config in config.data() {
        let data = load_data(data_config, config.start(), config.end())?;
        if data.is_empty() {
            log::warn!("No data found for config: {:?}", data_config.data_type());
            continue;
        }
        engine.add_data(data, data_config.client_id(), false, false);
    }

    engine.sort_data();
    engine.run(
        config.start(),
        config.end(),
        Some(config.id().to_string()),
        false,
    )
}

fn run_streaming(
    engine: &mut BacktestEngine,
    config: &BacktestRunConfig,
    chunk_size: usize,
) -> anyhow::Result<()> {
    let data_configs = config.data();

    if data_configs.len() == 1 {
        // Single config: stream directly from catalog iterator without
        // materializing the full dataset, bounded by chunk_size
        let data_config = &data_configs[0];
        let mut catalog = create_catalog(data_config)?;
        let result = dispatch_query(&mut catalog, data_config, config.start(), config.end())?;
        stream_chunks(engine, config, result.peekable(), chunk_size)?;
    } else {
        // Multiple configs require loading all data to merge-sort across types
        let all_data = load_and_merge_data(config)?;
        stream_chunks(engine, config, all_data.into_iter().peekable(), chunk_size)?;
    }

    Ok(())
}

// Feeds data from an iterator to the engine in timestamp-aligned chunks.
// Each chunk contains up to `chunk_size` events, extended to include all
// events sharing the boundary timestamp so timers flush correctly.
fn stream_chunks<I: Iterator<Item = Data>>(
    engine: &mut BacktestEngine,
    config: &BacktestRunConfig,
    mut iter: Peekable<I>,
    chunk_size: usize,
) -> anyhow::Result<()> {
    if iter.peek().is_none() {
        engine.end();
        return Ok(());
    }

    let mut next_start = config.start();
    loop {
        let chunk = take_aligned_chunk(&mut iter, chunk_size);
        if chunk.is_empty() {
            break;
        }

        let is_last = iter.peek().is_none();
        let end = if is_last {
            config.end()
        } else {
            chunk.last().map(HasTsInit::ts_init)
        };

        engine.add_data(chunk, None, false, true);
        engine.run(next_start, end, Some(config.id().to_string()), true)?;
        engine.clear_data();

        // Carry forward the end timestamp so the next chunk's run_impl
        // sets clocks contiguously and processes gap timers correctly
        next_start = end;
    }

    engine.end();
    Ok(())
}

// Takes up to `chunk_size` items, then extends to include all remaining
// items sharing the boundary timestamp to avoid splitting same-ts events.
fn take_aligned_chunk<I: Iterator<Item = Data>>(
    iter: &mut Peekable<I>,
    chunk_size: usize,
) -> Vec<Data> {
    let mut chunk = Vec::with_capacity(chunk_size);

    for _ in 0..chunk_size {
        match iter.next() {
            Some(item) => chunk.push(item),
            None => return chunk,
        }
    }

    if let Some(boundary_ts) = chunk.last().map(HasTsInit::ts_init) {
        while iter.peek().is_some_and(|d| d.ts_init() == boundary_ts) {
            chunk.push(iter.next().unwrap());
        }
    }

    chunk
}

fn load_and_merge_data(config: &BacktestRunConfig) -> anyhow::Result<Vec<Data>> {
    let mut all_data = Vec::new();
    for data_config in config.data() {
        let data = load_data(data_config, config.start(), config.end())?;
        if data.is_empty() {
            log::warn!("No data found for config: {:?}", data_config.data_type());
            continue;
        }
        all_data.extend(data);
    }
    all_data.sort_by_key(HasTsInit::ts_init);
    Ok(all_data)
}

fn create_catalog(config: &BacktestDataConfig) -> anyhow::Result<ParquetDataCatalog> {
    let uri = match config.catalog_fs_protocol() {
        Some(protocol) => format!("{protocol}://{}", config.catalog_path()),
        None => config.catalog_path().to_string(),
    };
    ParquetDataCatalog::from_uri(
        &uri,
        config.catalog_fs_storage_options().cloned(),
        None,
        None,
        None,
    )
}

fn load_data(
    config: &BacktestDataConfig,
    run_start: Option<UnixNanos>,
    run_end: Option<UnixNanos>,
) -> anyhow::Result<Vec<Data>> {
    let mut catalog = create_catalog(config)?;
    let result = dispatch_query(&mut catalog, config, run_start, run_end)?;
    Ok(result.collect())
}

fn dispatch_query(
    catalog: &mut ParquetDataCatalog,
    config: &BacktestDataConfig,
    run_start: Option<UnixNanos>,
    run_end: Option<UnixNanos>,
) -> anyhow::Result<QueryResult> {
    catalog.reset_session();

    let identifiers = config.query_identifiers();
    let start = max_opt(config.start_time(), run_start);
    let end = min_opt(config.end_time(), run_end);
    let filter = config.filter_expr();
    let optimize = config.optimize_file_loading();

    match config.data_type() {
        NautilusDataType::QuoteTick => {
            catalog.query::<QuoteTick>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::TradeTick => {
            catalog.query::<TradeTick>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::Bar => {
            catalog.query::<Bar>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::OrderBookDelta => {
            catalog.query::<OrderBookDelta>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::OrderBookDepth10 => {
            catalog.query::<OrderBookDepth10>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::MarkPriceUpdate => {
            catalog.query::<MarkPriceUpdate>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::IndexPriceUpdate => {
            catalog.query::<IndexPriceUpdate>(identifiers, start, end, filter, None, optimize)
        }
        NautilusDataType::InstrumentClose => {
            catalog.query::<InstrumentClose>(identifiers, start, end, filter, None, optimize)
        }
    }
}

fn max_opt(a: Option<UnixNanos>, b: Option<UnixNanos>) -> Option<UnixNanos> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn min_opt(a: Option<UnixNanos>, b: Option<UnixNanos>) -> Option<UnixNanos> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
