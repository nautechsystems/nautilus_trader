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

//! First-party native `BacktestNode` workload foundation for isolation proofs.
//!
//! The workload reads an existing Parquet catalog, constructs and runs a real
//! [`BacktestNode`], registers a compiled-in strategy through the native API,
//! and creates one bounded canonical JSON result file. The workload retains the
//! completed engine long enough to project its observable state through
//! [`CanonicalBacktestResult`](nautilus_backtest::result::CanonicalBacktestResult).
//!
//! Run with:
//! ```bash
//! cargo test -p nautilus-backtest --features streaming \
//!   --test integration backtest_node_workload::
//! ```

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use anyhow::Context;
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
    result::{BacktestResult, CanonicalBacktestResult},
};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::InstrumentId,
    types::Quantity,
};
use nautilus_trading::examples::strategies::EmaCross;
use serde_json::Value;
use ustr::Ustr;

const VENUE: &str = "SIM";
const INSTRUMENT_ID: &str = "AUD/USD.SIM";
const STARTING_BALANCE: &str = "1_000_000 USD";
const TRADE_SIZE: &str = "100000";
const EMA_FAST_PERIOD: usize = 10;
const EMA_SLOW_PERIOD: usize = 20;
const RUN_CONFIG_ID: &str = "backtest-node-workload-v1";
const RESULT_BYTES_MAX: usize = 1_048_576;
const EXPECTED_ITERATIONS: usize = 145;
const EXPECTED_EVENTS: usize = 4;
const EXPECTED_ORDERS: usize = 2;
const EXPECTED_POSITIONS: usize = 2;

fn run_workload(catalog_path: &Path, result_path: &Path) -> anyhow::Result<()> {
    let catalog_metadata = fs::metadata(catalog_path).with_context(|| {
        format!(
            "failed to read catalog metadata at {}",
            catalog_path.display()
        )
    })?;
    anyhow::ensure!(
        catalog_metadata.is_dir(),
        "catalog path is not a directory: {}",
        catalog_path.display()
    );
    let catalog_path = catalog_path
        .to_str()
        .context("catalog path is not valid UTF-8")?
        .to_string();
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let venue_config = BacktestVenueConfig::builder()
        .name(Ustr::from(VENUE))
        .oms_type(OmsType::Hedging)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![STARTING_BALANCE.to_string()])
        .build()?;
    let data_config = BacktestDataConfig::builder()
        .data_type(NautilusDataType::QuoteTick)
        .catalog_path(catalog_path)
        .instrument_id(instrument_id)
        .build()?;
    let run_config = BacktestRunConfig::builder()
        .id(RUN_CONFIG_ID.to_string())
        .venues(vec![venue_config])
        .data(vec![data_config])
        .dispose_on_completion(false)
        .raise_exception(true)
        .build()?;

    let mut node = BacktestNode::new(vec![run_config])?;
    node.build()?;
    let engine = node
        .get_engine_mut(RUN_CONFIG_ID)
        .with_context(|| format!("backtest engine was not built for run {RUN_CONFIG_ID}"))?;
    engine.add_strategy(EmaCross::new(
        instrument_id,
        Quantity::from(TRADE_SIZE),
        EMA_FAST_PERIOD,
        EMA_SLOW_PERIOD,
    ))?;

    let results = node.run()?;
    anyhow::ensure!(
        results.len() == 1,
        "expected one backtest result, received {}",
        results.len()
    );
    let result = &results[0];
    validate_result(result)?;
    let canonical = node
        .get_engine(RUN_CONFIG_ID)
        .with_context(|| format!("backtest engine was not retained for run {RUN_CONFIG_ID}"))?
        .get_canonical_result()?;
    validate_canonical_result(&canonical)?;
    write_result(result_path, &canonical)
}

fn validate_result(result: &BacktestResult) -> anyhow::Result<()> {
    anyhow::ensure!(
        result.run_config_id.as_deref() == Some(RUN_CONFIG_ID),
        "result run config ID did not match {RUN_CONFIG_ID}"
    );
    anyhow::ensure!(
        result.iterations == EXPECTED_ITERATIONS,
        "expected {EXPECTED_ITERATIONS} iterations, received {}",
        result.iterations
    );
    anyhow::ensure!(
        result.total_events == EXPECTED_EVENTS,
        "expected {EXPECTED_EVENTS} events, received {}",
        result.total_events
    );
    anyhow::ensure!(
        result.total_orders == EXPECTED_ORDERS,
        "expected {EXPECTED_ORDERS} orders, received {}",
        result.total_orders
    );
    anyhow::ensure!(
        result.total_positions == EXPECTED_POSITIONS,
        "expected {EXPECTED_POSITIONS} positions, received {}",
        result.total_positions
    );
    Ok(())
}

fn validate_canonical_result(result: &CanonicalBacktestResult) -> anyhow::Result<()> {
    let run = &result.as_value()["run"];
    anyhow::ensure!(
        result.as_value()["schema"] == "nautilus-backtest-result/v1",
        "canonical result schema did not match version 1"
    );
    anyhow::ensure!(
        run["run_config_id"] == RUN_CONFIG_ID,
        "canonical result run config ID did not match {RUN_CONFIG_ID}"
    );
    anyhow::ensure!(
        parse_count(&run["iterations"]) == Some(EXPECTED_ITERATIONS),
        "canonical result iteration count did not match {EXPECTED_ITERATIONS}"
    );
    anyhow::ensure!(
        parse_count(&run["total_events"]) == Some(EXPECTED_EVENTS),
        "canonical result event count did not match {EXPECTED_EVENTS}"
    );
    anyhow::ensure!(
        parse_count(&run["total_orders"]) == Some(EXPECTED_ORDERS),
        "canonical result order count did not match {EXPECTED_ORDERS}"
    );
    anyhow::ensure!(
        parse_count(&run["total_positions"]) == Some(EXPECTED_POSITIONS),
        "canonical result position count did not match {EXPECTED_POSITIONS}"
    );
    Ok(())
}

fn parse_count(value: &Value) -> Option<usize> {
    value.as_str()?.parse().ok()
}

fn write_result(path: &Path, result: &CanonicalBacktestResult) -> anyhow::Result<()> {
    let mut bytes = CappedBuffer::new(RESULT_BYTES_MAX);
    bytes
        .write_all(&result.to_bytes()?)
        .context("failed to serialize canonical backtest result")?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create result file {}", path.display()))?;
    if let Err(e) = file
        .write_all(bytes.as_slice())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e).with_context(|| format!("failed to write result file {}", path.display()));
    }
    Ok(())
}

#[derive(Debug)]
struct CappedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl CappedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(8_192)),
            limit,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

impl Write for CappedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(format!(
                "result exceeds the {} byte limit",
                self.limit
            )));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf};

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::QuoteTick,
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        types::Price,
    };
    use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn test_capped_buffer_rejects_write_past_limit() {
        let mut buffer = CappedBuffer::new(5);

        assert_eq!(buffer.write(b"12345").unwrap(), 5);
        let error = buffer.write(b"6").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "result exceeds the 5 byte limit");
        assert_eq!(buffer.as_slice(), b"12345");
    }

    #[rstest]
    fn test_workload_writes_repeatable_bounded_canonical_result() {
        let temp_dir = TempDir::new().unwrap();
        let catalog_path = temp_dir.path().join("catalog");
        let result_path = temp_dir.path().join("test-result-1.json");
        let repeated_path = temp_dir.path().join("test-result-2.json");
        fs::create_dir(&catalog_path).unwrap();
        write_catalog(&catalog_path);

        run_workload(&catalog_path, &result_path).unwrap();
        run_workload(&catalog_path, &repeated_path).unwrap();

        let bytes = fs::read(&result_path).unwrap();
        let repeated_bytes = fs::read(&repeated_path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let result = CanonicalBacktestResult::from_slice(&bytes).unwrap();
        let repeated = CanonicalBacktestResult::from_slice(&repeated_bytes).unwrap();
        let second_run = run_workload(&catalog_path, &result_path);
        let expected_error = format!("failed to create result file {}", result_path.display());
        assert!(bytes.len() <= RESULT_BYTES_MAX);
        assert_eq!(bytes, repeated_bytes);
        assert_eq!(result.digest().unwrap(), repeated.digest().unwrap());
        assert_eq!(result.first_divergence(&repeated), None);
        assert_eq!(value["schema"], "nautilus-backtest-result/v1");
        assert_eq!(value["run"]["run_config_id"], RUN_CONFIG_ID);
        assert_eq!(
            parse_count(&value["run"]["iterations"]),
            Some(EXPECTED_ITERATIONS)
        );
        assert_eq!(
            parse_count(&value["run"]["total_events"]),
            Some(EXPECTED_EVENTS)
        );
        assert_eq!(
            parse_count(&value["run"]["total_orders"]),
            Some(EXPECTED_ORDERS)
        );
        assert_eq!(
            parse_count(&value["run"]["total_positions"]),
            Some(EXPECTED_POSITIONS)
        );
        assert_eq!(value["run"]["outcome"], "completed");
        assert!(
            value["positions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|position| position.get("id").is_none())
        );
        assert_eq!(value["positions"][0]["position_id"], "position-1");
        assert_eq!(second_run.unwrap_err().to_string(), expected_error);
    }

    #[rstest]
    #[ignore = "generates the immutable catalog used by the optimized native repeat proof"]
    fn generate_native_repeat_catalog() {
        let catalog_path = env::var_os("NAUTILUS_NATIVE_REPEAT_CATALOG_PATH")
            .map(PathBuf::from)
            .expect("NAUTILUS_NATIVE_REPEAT_CATALOG_PATH must be set");
        fs::create_dir(&catalog_path).unwrap();
        write_catalog(&catalog_path);
    }

    #[rstest]
    fn test_workload_rejects_non_directory_catalog_without_result() {
        let temp_dir = TempDir::new().unwrap();
        let catalog_path = temp_dir.path().join("catalog.parquet");
        let result_path = temp_dir.path().join("result.json");
        fs::write(&catalog_path, b"not a catalog").unwrap();

        let error = run_workload(&catalog_path, &result_path).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "catalog path is not a directory: {}",
                catalog_path.display()
            )
        );
        assert!(!result_path.exists());
    }

    fn write_catalog(path: &Path) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let instrument_id = instrument.id();
        let catalog = ParquetDataCatalog::new(path, None, None, None, None);
        catalog.write_instruments(vec![instrument]).unwrap();
        catalog
            .write_to_parquet(&generate_quotes(instrument_id), None, None, None)
            .unwrap();
    }

    fn generate_quotes(instrument_id: InstrumentId) -> Vec<QuoteTick> {
        let base_ts = 1_735_689_600_000_000_000_u64;
        let interval = 1_000_000_000_u64;
        let mut quotes = Vec::new();
        let mut tick = 0_u64;
        let mut add = |mid: f64| {
            let bid = format!("{mid:.5}");
            let ask = format!("{:.5}", mid + 0.00020);
            let ts = UnixNanos::from(base_ts + tick * interval);
            quotes.push(QuoteTick::new(
                instrument_id,
                Price::from(bid.as_str()),
                Price::from(ask.as_str()),
                Quantity::from("100000"),
                Quantity::from("100000"),
                ts,
                ts,
            ));
            tick += 1;
        };

        for _ in 0..25 {
            add(0.65000);
        }

        for i in 0..40 {
            add(0.65000 + (i as f64 * 0.00050));
        }

        for i in 0..80 {
            add(0.66950 - (i as f64 * 0.00050));
        }
        quotes
    }
}
