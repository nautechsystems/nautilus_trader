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

use nautilus_common::config::{ConfigError, ConfigErrorCollector, ConfigResult};
use nautilus_core::serialization::default_true;
use serde::{Deserialize, Serialize};

/// Configuration for `Portfolio` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.portfolio",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.portfolio")
)]
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "config fields mirror the existing Python and serialization surface"
)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
pub struct PortfolioConfig {
    /// The type of prices used for portfolio calculations, such as unrealized PnLs.
    /// If false (default), uses quote prices if available; otherwise, last trade prices
    /// (or falls back to bar prices if `bar_updates` is true).
    /// If true, uses mark prices.
    #[serde(default)]
    #[builder(default)]
    pub use_mark_prices: bool,
    /// The type of exchange rates used for portfolio calculations.
    /// If false (default), uses quote prices.
    /// If true, uses mark prices.
    #[serde(default)]
    #[builder(default)]
    pub use_mark_xrates: bool,
    /// If external bars should be considered for updating unrealized PnLs.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub bar_updates: bool,
    /// If calculations should be converted into each account's base currency.
    /// This setting is only effective for accounts with a specified base currency.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub convert_to_account_base_currency: bool,
    /// The minimum interval (milliseconds) between logging account state events for the same account.
    /// When set, account state updates will only be logged if this much time has passed since the last log.
    /// Useful for HFT deployments to prevent excessive logging when account states change rapidly.
    #[serde(default)]
    pub min_account_state_logging_interval_ms: Option<u64>,
    /// The interval (milliseconds) between portfolio snapshot emissions per account.
    /// When set, a [`PortfolioSnapshot`] is emitted at this cadence while the
    /// account holds at least one open position, carrying continuous
    /// mark-to-market equity. When `None` (the default), no periodic snapshots
    /// are emitted.
    ///
    /// [`PortfolioSnapshot`]: nautilus_model::events::PortfolioSnapshot
    #[serde(default)]
    pub snapshot_interval_ms: Option<u64>,
    /// If debug mode is active (will provide extra debug logging).
    #[serde(default)]
    #[builder(default)]
    pub debug: bool,
}

impl<S: portfolio_config_builder::IsComplete> PortfolioConfigBuilder<S> {
    /// Validates and builds the [`PortfolioConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`PortfolioConfig::validate`]).
    pub fn build(self) -> ConfigResult<PortfolioConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl PortfolioConfig {
    /// Validates the portfolio configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        for (field, value) in [
            (
                "min_account_state_logging_interval_ms",
                self.min_account_state_logging_interval_ms,
            ),
            ("snapshot_interval_ms", self.snapshot_interval_ms),
        ] {
            if let Some(ms) = value {
                errors.check(
                    ms > 0,
                    ConfigError::range(
                        field,
                        format!("must be a positive number of milliseconds, was {ms}"),
                    ),
                );
            }
        }

        errors.into_result()
    }
}

impl Default for PortfolioConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default `PortfolioConfig` should be valid")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config_is_valid() {
        assert!(PortfolioConfig::builder().build().is_ok());
    }

    #[rstest]
    fn test_zero_min_account_state_logging_interval_rejected() {
        let result = PortfolioConfig::builder()
            .min_account_state_logging_interval_ms(0)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "min_account_state_logging_interval_ms")
        );
    }

    #[rstest]
    fn test_zero_snapshot_interval_rejected() {
        let result = PortfolioConfig::builder().snapshot_interval_ms(0).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "snapshot_interval_ms")
        );
    }

    #[rstest]
    fn test_positive_intervals_accepted() {
        let result = PortfolioConfig::builder()
            .min_account_state_logging_interval_ms(1_000)
            .snapshot_interval_ms(5_000)
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_multiple_violations_collected() {
        let result = PortfolioConfig::builder()
            .min_account_state_logging_interval_ms(0)
            .snapshot_interval_ms(0)
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 2);
    }
}
