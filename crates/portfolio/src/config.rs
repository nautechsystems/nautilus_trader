// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use serde::{Deserialize, Serialize};

/// Configuration for `Portfolio` instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioConfig {
    /// The type of prices used for portfolio calculations, such as unrealized PnLs.
    /// If false (default), uses quote prices if available; otherwise, last trade prices
    /// (or falls back to bar prices if `bar_updates` is true).
    /// If true, uses mark prices.
    #[serde(default)]
    pub use_mark_prices: bool,
    /// The type of exchange rates used for portfolio calculations.
    /// If false (default), uses quote prices.
    /// If true, uses mark prices.
    #[serde(default)]
    pub use_mark_xrates: bool,
    /// If external bars should be considered for updating unrealized PnLs.
    #[serde(default = "default_true")]
    pub bar_updates: bool,
    /// If calculations should be converted into each account's base currency.
    /// This setting is only effective for accounts with a specified base currency.
    #[serde(default = "default_true")]
    pub convert_to_account_base_currency: bool,
    /// The minimum interval (milliseconds) between logging account state events for the same account.
    /// When set, account state updates will only be logged if this much time has passed since the last log.
    /// Useful for HFT deployments to prevent excessive logging when account states change rapidly.
    #[serde(default)]
    pub min_account_state_logging_interval_ms: Option<u64>,
    /// If debug mode is active (will provide extra debug logging).
    #[serde(default)]
    pub debug: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for PortfolioConfig {
    fn default() -> Self {
        Self {
            use_mark_prices: false,
            use_mark_xrates: false,
            bar_updates: true,
            convert_to_account_base_currency: true,
            min_account_state_logging_interval_ms: None,
            debug: false,
        }
    }
}
