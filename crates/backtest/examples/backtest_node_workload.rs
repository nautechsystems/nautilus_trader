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
//! and creates one bounded JSON result file. The result uses the existing
//! [`BacktestResult`](nautilus_backtest::result::BacktestResult) representation
//! as native execution evidence, not the canonical parity result.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-backtest --features examples,streaming \
//!   --example backtest-node-workload -- <CATALOG_PATH> <RESULT_PATH>
//! ```

use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
    result::BacktestResult,
};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::InstrumentId,
    types::Quantity,
};
use nautilus_trading::examples::strategies::EmaCross;
use ustr::Ustr;

const VENUE: &str = "SIM";
const INSTRUMENT_ID: &str = "AUD/USD.SIM";
const STARTING_BALANCE: &str = "1_000_000 USD";
const TRADE_SIZE: &str = "100000";
const EMA_FAST_PERIOD: usize = 10;
const EMA_SLOW_PERIOD: usize = 20;
const RUN_CONFIG_ID: &str = "eng-454-backtest-node-v1";
const RESULT_BYTES_MAX: usize = 1_048_576;
const EXPECTED_ITERATIONS: usize = 145;
const EXPECTED_EVENTS: usize = 4;
const EXPECTED_ORDERS: usize = 2;
const EXPECTED_POSITIONS: usize = 2;

fn main() -> anyhow::Result<()> {
    let (catalog_path, result_path) = parse_paths(env::args_os().skip(1))?;
    run_workload(&catalog_path, &result_path)
}

fn parse_paths(mut args: impl Iterator<Item = OsString>) -> anyhow::Result<(PathBuf, PathBuf)> {
    let catalog_path = args
        .next()
        .context("missing catalog path; expected <CATALOG_PATH> <RESULT_PATH>")?;
    let result_path = args
        .next()
        .context("missing result path; expected <CATALOG_PATH> <RESULT_PATH>")?;
    anyhow::ensure!(
        args.next().is_none(),
        "unexpected argument; expected <CATALOG_PATH> <RESULT_PATH>"
    );
    Ok((PathBuf::from(catalog_path), PathBuf::from(result_path)))
}

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
    write_result(result_path, result)
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

fn write_result(path: &Path, result: &BacktestResult) -> anyhow::Result<()> {
    let mut bytes = CappedBuffer::new(RESULT_BYTES_MAX);
    serde_json::to_writer(&mut bytes, result).context("failed to serialize backtest result")?;

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
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::QuoteTick,
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        types::Price,
    };
    use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    use rstest::rstest;
    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn test_parse_paths_accepts_exact_catalog_and_result_paths() {
        let args = vec![
            OsString::from("/input/catalog"),
            OsString::from("/output/result.json"),
        ];

        let paths = parse_paths(args.into_iter()).unwrap();

        assert_eq!(paths.0, PathBuf::from("/input/catalog"));
        assert_eq!(paths.1, PathBuf::from("/output/result.json"));
    }

    #[rstest]
    #[case(
        Vec::<OsString>::new(),
        "missing catalog path; expected <CATALOG_PATH> <RESULT_PATH>"
    )]
    #[case(
        vec![OsString::from("/input/catalog")],
        "missing result path; expected <CATALOG_PATH> <RESULT_PATH>"
    )]
    #[case(
        vec![
            OsString::from("/input/catalog"),
            OsString::from("/output/result.json"),
            OsString::from("extra"),
        ],
        "unexpected argument; expected <CATALOG_PATH> <RESULT_PATH>"
    )]
    fn test_parse_paths_rejects_incomplete_or_extra_arguments(
        #[case] args: Vec<OsString>,
        #[case] expected: &str,
    ) {
        let error = parse_paths(args.into_iter()).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

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
    fn test_workload_runs_backtest_node_and_writes_one_bounded_result() {
        let temp_dir = TempDir::new().unwrap();
        let catalog_path = temp_dir.path().join("catalog");
        let result_path = temp_dir.path().join("test-result.json");
        fs::create_dir(&catalog_path).unwrap();
        write_catalog(&catalog_path);

        run_workload(&catalog_path, &result_path).unwrap();

        let bytes = fs::read(&result_path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let second_run = run_workload(&catalog_path, &result_path);
        let expected_error = format!("failed to create result file {}", result_path.display());
        assert!(bytes.len() <= RESULT_BYTES_MAX);
        assert_eq!(value["run_config_id"], RUN_CONFIG_ID);
        assert_eq!(value["iterations"], EXPECTED_ITERATIONS);
        assert_eq!(value["total_events"], EXPECTED_EVENTS);
        assert_eq!(value["total_orders"], EXPECTED_ORDERS);
        assert_eq!(value["total_positions"], EXPECTED_POSITIONS);
        assert_eq!(second_run.unwrap_err().to_string(), expected_error);
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
