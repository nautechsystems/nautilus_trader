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

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use nautilus_common::enums::Environment;
use nautilus_model::types::Quantity;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct GridMarketMakerTomlConfig {
    pub exchange: String,
    pub trader_id: String,
    pub instrument_id: String,
    pub max_position: String,
    pub trade_size: String,
    #[serde(default = "default_num_levels")]
    pub num_levels: usize,
    #[serde(default = "default_grid_step_bps")]
    pub grid_step_bps: u32,
    #[serde(default)]
    pub skew_factor: f64,
    #[serde(default = "default_requote_threshold_bps")]
    pub requote_threshold_bps: u32,
    pub expire_time_secs: Option<u64>,
    #[serde(default)]
    pub on_cancel_resubmit: bool,
    #[serde(default = "default_recorder_path")]
    pub path: String,
    #[serde(deserialize_with = "deserialize_environment")]
    pub execution_environment: Environment,
}

#[derive(Debug, Deserialize)]
pub struct RecorderTomlConfig {
    pub exchange: String,
    pub trader_id: String,
    pub instrument_id: Vec<String>,
    #[serde(default = "default_recorder_path")]
    pub catalog_path: String,
    pub book_depth: usize,
    pub interval_parquet_dump_seconds: u64,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
pub struct MattiasMarketMakerTomlConfig {
    pub exchange: String,
    pub trader_id: String,
    pub instrument_id: String,
    #[serde(default = "default_recorder_path")]
    pub path: String,
    pub Q_max: Quantity,
    pub Φ_0: Quantity,
    pub Φ_n: u8,
    pub Δ_0: Decimal,
    pub Δ_μ: Decimal,
    pub β: Decimal,

    /// execution environment. possible values are live and backtest
    #[serde(deserialize_with = "deserialize_environment")]
    pub execution_environment: Environment,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunnerTomlConfig {
    /// Name of the strategy to run. Must match a registered strategy
    /// (e.g. `"grid_mm"`, `"mmm"`). Switching strategies only requires
    /// editing this value, no recompilation.
    pub strategy: String,
    /// Backtest venue name.
    #[serde(default = "default_venue")]
    pub venue: String,
    /// Account id used for backtest snapshot reporting.
    #[serde(default = "default_account_id")]
    pub account_id: String,
    /// Backtest run id. Defaults to `{strategy}-backtest`.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Backtest start date (RFC 3339). Required for backtesting.
    #[serde(default)]
    pub start_date: Option<DateTime<Utc>>,
    /// Backtest end date (RFC 3339). Required for backtesting.
    #[serde(default)]
    pub end_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "grid_mm")]
    pub grid_mm: Option<GridMarketMakerTomlConfig>,
    pub recorder: Option<RecorderTomlConfig>,
    pub mmm: Option<MattiasMarketMakerTomlConfig>,
    pub runner: Option<RunnerTomlConfig>,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config at {:?}", path.as_ref().display()))?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn load(path: String) -> Result<Self> {
        Self::from_file(path)
    }
}

fn deserialize_environment<'de, D>(deserializer: D) -> Result<Environment, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Environment::from_str(&s).map_err(de::Error::custom)
}

fn default_num_levels() -> usize {
    3
}

fn default_venue() -> String {
    "BYBIT".into()
}

fn default_account_id() -> String {
    "BYBIT-001".into()
}

fn default_grid_step_bps() -> u32 {
    10
}

fn default_requote_threshold_bps() -> u32 {
    5
}

fn default_recorder_path() -> String {
    "data/".into()
}
