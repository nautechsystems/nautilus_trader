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

//! Results from completed backtest runs.

use ahash::AHashMap;
use nautilus_core::{UUID4, UnixNanos};
use serde::Serialize;

/// Results from a completed backtest run.
#[derive(Debug, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.backtest",
        skip_from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct BacktestResult {
    pub trader_id: String,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub run_config_id: Option<String>,
    pub run_id: Option<UUID4>,
    pub run_started: Option<UnixNanos>,
    pub run_finished: Option<UnixNanos>,
    pub backtest_start: Option<UnixNanos>,
    pub backtest_end: Option<UnixNanos>,
    pub elapsed_time_secs: f64,
    pub iterations: usize,
    pub total_events: usize,
    pub total_orders: usize,
    pub total_positions: usize,
    pub summary: AHashMap<String, String>,
    pub stats_pnls: AHashMap<String, AHashMap<String, f64>>,
    pub stats_returns: AHashMap<String, f64>,
    pub stats_general: AHashMap<String, f64>,
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_backtest_result_serializes_to_json() {
        let instance_id = UUID4::from("11111111-1111-4111-8111-111111111111");
        let run_id = UUID4::from("22222222-2222-4222-8222-222222222222");
        let mut summary = AHashMap::new();
        summary.insert("PnL (total)".to_string(), "10.00 USD".to_string());
        let mut usd_pnls = AHashMap::new();
        usd_pnls.insert("Returns Volatility (252 days)".to_string(), 1.25);
        let mut stats_pnls = AHashMap::new();
        stats_pnls.insert("USD".to_string(), usd_pnls);
        let mut stats_returns = AHashMap::new();
        stats_returns.insert("Sharpe Ratio (252 days)".to_string(), 0.75);
        let mut stats_general = AHashMap::new();
        stats_general.insert("Long Ratio".to_string(), 1.0);

        let result = BacktestResult {
            trader_id: "TRADER-001".to_string(),
            machine_id: "machine-1".to_string(),
            instance_id,
            run_config_id: Some("config-1".to_string()),
            run_id: Some(run_id),
            run_started: Some(UnixNanos::new(1)),
            run_finished: Some(UnixNanos::new(2)),
            backtest_start: Some(UnixNanos::new(3)),
            backtest_end: Some(UnixNanos::new(4)),
            elapsed_time_secs: 1.5,
            iterations: 10,
            total_events: 20,
            total_orders: 2,
            total_positions: 1,
            summary,
            stats_pnls,
            stats_returns,
            stats_general,
        };

        let value = serde_json::to_value(&result).unwrap();

        assert_eq!(value["trader_id"], json!("TRADER-001"));
        assert_eq!(value["machine_id"], json!("machine-1"));
        assert_eq!(value["instance_id"], json!(instance_id.to_string()));
        assert_eq!(value["run_id"], json!(run_id.to_string()));
        assert_eq!(value["run_started"], json!(1));
        assert_eq!(value["backtest_end"], json!(4));
        assert_eq!(value["elapsed_time_secs"], json!(1.5));
        assert_eq!(value["iterations"], json!(10));
        assert_eq!(value["summary"]["PnL (total)"], json!("10.00 USD"));
        assert_eq!(
            value["stats_pnls"]["USD"]["Returns Volatility (252 days)"],
            json!(1.25)
        );
        assert_eq!(
            value["stats_returns"]["Sharpe Ratio (252 days)"],
            json!(0.75)
        );
        assert_eq!(value["stats_general"]["Long Ratio"], json!(1.0));
    }
}
