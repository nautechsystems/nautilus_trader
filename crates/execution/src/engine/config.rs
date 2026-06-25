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
use nautilus_model::identifiers::ClientId;
use serde::{Deserialize, Serialize};

/// Configuration for `ExecutionEngine` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
pub struct ExecutionEngineConfig {
    /// If the cache should be loaded on initialization.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub load_cache: bool,
    /// If the execution engine should maintain own/user order books based on commands and events.
    #[serde(default)]
    #[builder(default)]
    pub manage_own_order_books: bool,
    /// If order state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at every order state update (when events are applied).
    #[serde(default)]
    #[builder(default)]
    pub snapshot_orders: bool,
    /// If position state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at position opened, changed and closed (when events are applied).
    #[serde(default)]
    #[builder(default)]
    pub snapshot_positions: bool,
    /// The interval (seconds) at which additional position state snapshots are persisted.
    /// If `None` then no additional snapshots will be taken.
    #[serde(default)]
    pub snapshot_positions_interval_secs: Option<f64>,
    /// If order fills exceeding order quantity are allowed (logs warning instead of raising).
    /// Useful when position reconciliation races with exchange fill events.
    #[serde(default)]
    #[builder(default)]
    pub allow_overfills: bool,
    /// If unclaimed venue orders should be filtered during execution reconciliation.
    #[serde(default)]
    #[builder(default)]
    pub filter_unclaimed_external_orders: bool,
    /// The client IDs declared for external stream processing.
    ///
    /// The execution engine will not attempt to send trading commands to these
    /// client IDs, assuming an external process will consume the serialized
    /// command messages from the bus and handle execution.
    #[serde(default)]
    pub external_clients: Option<Vec<ClientId>>,
    /// The interval (minutes) between purging closed orders from the in-memory cache.
    #[serde(default)]
    pub purge_closed_orders_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed orders can be purged.
    #[serde(default)]
    pub purge_closed_orders_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging closed positions from the in-memory cache.
    #[serde(default)]
    pub purge_closed_positions_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed positions can be purged.
    #[serde(default)]
    pub purge_closed_positions_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging account events from the in-memory cache.
    #[serde(default)]
    pub purge_account_events_interval_mins: Option<u32>,
    /// The time buffer (minutes) before account events can be purged.
    #[serde(default)]
    pub purge_account_events_lookback_mins: Option<u32>,
    /// If purge operations should also delete from the backing database.
    #[serde(default)]
    #[builder(default)]
    pub purge_from_database: bool,
    /// If debug mode is active (will provide extra debug logging).
    #[serde(default)]
    #[builder(default)]
    pub debug: bool,
}

impl<S: execution_engine_config_builder::IsComplete> ExecutionEngineConfigBuilder<S> {
    /// Validates and builds the [`ExecutionEngineConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`ExecutionEngineConfig::validate`]).
    pub fn build(self) -> ConfigResult<ExecutionEngineConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl ExecutionEngineConfig {
    /// Validates the execution engine configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if let Some(secs) = self.snapshot_positions_interval_secs {
            errors.check(
                secs.is_finite() && secs > 0.0,
                ConfigError::range(
                    "snapshot_positions_interval_secs",
                    format!("must be a positive finite value, was {secs}"),
                ),
            );
        }

        for (field, value) in [
            (
                "purge_closed_orders_interval_mins",
                self.purge_closed_orders_interval_mins,
            ),
            (
                "purge_closed_positions_interval_mins",
                self.purge_closed_positions_interval_mins,
            ),
            (
                "purge_account_events_interval_mins",
                self.purge_account_events_interval_mins,
            ),
        ] {
            if let Some(mins) = value {
                errors.check(
                    mins > 0,
                    ConfigError::range(
                        field,
                        format!("must be a positive number of minutes, was {mins}"),
                    ),
                );
            }
        }

        errors.into_result()
    }
}

impl Default for ExecutionEngineConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default `ExecutionEngineConfig` should be valid")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config_is_valid() {
        assert!(ExecutionEngineConfig::builder().build().is_ok());
    }

    #[rstest]
    #[case(0.0)]
    #[case(-1.0)]
    #[case(f64::INFINITY)]
    #[case(f64::NAN)]
    fn test_invalid_snapshot_positions_interval_secs_rejected(#[case] secs: f64) {
        let result = ExecutionEngineConfig::builder()
            .snapshot_positions_interval_secs(secs)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "snapshot_positions_interval_secs")
        );
    }

    #[rstest]
    fn test_positive_snapshot_positions_interval_secs_accepted() {
        let result = ExecutionEngineConfig::builder()
            .snapshot_positions_interval_secs(5.0)
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_zero_purge_closed_orders_interval_rejected() {
        let result = ExecutionEngineConfig::builder()
            .purge_closed_orders_interval_mins(0)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "purge_closed_orders_interval_mins")
        );
    }

    #[rstest]
    fn test_zero_purge_closed_positions_interval_rejected() {
        let result = ExecutionEngineConfig::builder()
            .purge_closed_positions_interval_mins(0)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "purge_closed_positions_interval_mins")
        );
    }

    #[rstest]
    fn test_zero_purge_account_events_interval_rejected() {
        let result = ExecutionEngineConfig::builder()
            .purge_account_events_interval_mins(0)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "purge_account_events_interval_mins")
        );
    }

    #[rstest]
    fn test_positive_purge_intervals_accepted() {
        // A zero buffer is valid (no grace period), only the intervals must be positive
        let result = ExecutionEngineConfig::builder()
            .purge_closed_orders_interval_mins(10)
            .purge_closed_positions_interval_mins(10)
            .purge_account_events_interval_mins(10)
            .purge_closed_orders_buffer_mins(0)
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_multiple_violations_collected() {
        let result = ExecutionEngineConfig::builder()
            .snapshot_positions_interval_secs(0.0)
            .purge_closed_orders_interval_mins(0)
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().any(
            |e| matches!(e, ConfigError::Range { field, .. } if field == "snapshot_positions_interval_secs")
        ));
        assert!(errors.iter().any(
            |e| matches!(e, ConfigError::Range { field, .. } if field == "purge_closed_orders_interval_mins")
        ));
    }
}
