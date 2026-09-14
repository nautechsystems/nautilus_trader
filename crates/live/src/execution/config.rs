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

//! Configuration for execution reconciliation decisions and cache retention.
//!
//! Thresholds, retry limits, filters, and lookbacks govern manager decisions. Startup enablement
//! and polling intervals belong to the live node's execution-engine configuration.

use indexmap::IndexSet;
use nautilus_common::config::{ConfigError, ConfigErrorCollector, ConfigResult};
use nautilus_core::{DurationNanos, datetime::checked_mins_to_secs};
use nautilus_model::identifiers::{ClientOrderId, InstrumentId, TraderId};

/// Configuration for execution manager.
#[expect(
    clippy::struct_excessive_bools,
    reason = "config flags mirror the live execution engine configuration surface"
)]
#[derive(Debug, Clone)]
pub struct ExecutionManagerConfig {
    /// The trader ID for generated orders.
    pub trader_id: TraderId,
    /// Number of minutes to look back during reconciliation.
    pub lookback_mins: Option<u64>,
    /// Instrument IDs to include during reconciliation (empty => all).
    pub reconciliation_instrument_ids: IndexSet<InstrumentId>,
    /// Whether to filter unclaimed external orders.
    pub filter_unclaimed_external: bool,
    /// Whether to filter position status reports during reconciliation.
    pub filter_position_reports: bool,
    /// Client order IDs excluded from reconciliation.
    pub filtered_client_order_ids: IndexSet<ClientOrderId>,
    /// Whether to generate missing orders from reports.
    pub generate_missing_orders: bool,
    /// Threshold in milliseconds for inflight order checks.
    pub inflight_threshold_ms: u64,
    /// Maximum number of retries for inflight checks.
    pub inflight_max_retries: u32,
    /// The lookback minutes for open order checks.
    pub open_check_lookback_mins: Option<u64>,
    /// Threshold before acting on venue discrepancies for open orders.
    pub open_check_threshold_ns: DurationNanos,
    /// Maximum retries before resolving an open order missing at the venue.
    pub open_check_missing_retries: u32,
    /// Whether open-order polling should only request open orders from the venue.
    pub open_check_open_only: bool,
    /// The maximum number of single-order queries per consistency check cycle.
    pub max_single_order_queries_per_cycle: u32,
    /// The delay (milliseconds) between consecutive single-order queries.
    pub single_order_query_delay_ms: u32,
    /// The lookback minutes for position consistency checks.
    pub position_check_lookback_mins: u64,
    /// Threshold before acting on venue discrepancies for positions.
    pub position_check_threshold_ns: DurationNanos,
    /// Maximum retries before stopping position discrepancy reconciliation.
    pub position_check_retries: u32,
    /// The time buffer (minutes) before closed orders can be purged.
    pub purge_closed_orders_buffer_mins: Option<u32>,
    /// The time buffer (minutes) before closed positions can be purged.
    pub purge_closed_positions_buffer_mins: Option<u32>,
    /// The time buffer (minutes) before account events can be purged.
    pub purge_account_events_lookback_mins: Option<u32>,
    /// If purge operations should also delete from the backing database.
    pub purge_from_database: bool,
}

impl Default for ExecutionManagerConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            lookback_mins: Some(60),
            reconciliation_instrument_ids: IndexSet::new(),
            filter_unclaimed_external: false,
            filter_position_reports: false,
            filtered_client_order_ids: IndexSet::new(),
            generate_missing_orders: true,
            inflight_threshold_ms: 5_000,
            inflight_max_retries: 5,
            open_check_lookback_mins: Some(60),
            open_check_threshold_ns: DurationNanos::from_secs(5),
            open_check_missing_retries: 5,
            open_check_open_only: true,
            max_single_order_queries_per_cycle: 5,
            single_order_query_delay_ms: 100,
            position_check_lookback_mins: 60,
            position_check_threshold_ns: DurationNanos::from_mins(1),
            position_check_retries: 3,
            purge_closed_orders_buffer_mins: None,
            purge_closed_positions_buffer_mins: None,
            purge_account_events_lookback_mins: None,
            purge_from_database: false,
        }
    }
}

impl ExecutionManagerConfig {
    /// Validates the execution manager configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::with_capacity(3);

        if let Some(mins) = self.lookback_mins {
            errors.check(
                checked_mins_to_secs(mins).is_some(),
                ConfigError::range(
                    "ExecutionManagerConfig.lookback_mins",
                    format!("{mins} minutes (must fit in `u64` seconds)"),
                ),
            );
        }

        if let Some(mins) = self.open_check_lookback_mins {
            errors.check(
                DurationNanos::try_from_mins(mins).is_ok(),
                ConfigError::range(
                    "ExecutionManagerConfig.open_check_lookback_mins",
                    format!("{mins} minutes (must fit in `u64` nanoseconds)"),
                ),
            );
        }

        errors.check(
            DurationNanos::try_from_mins(self.position_check_lookback_mins).is_ok(),
            ConfigError::range(
                "ExecutionManagerConfig.position_check_lookback_mins",
                format!(
                    "{} minutes (must fit in `u64` nanoseconds)",
                    self.position_check_lookback_mins
                ),
            ),
        );

        errors.into_result()
    }

    /// Sets the trader ID on the configuration.
    #[must_use]
    pub fn with_trader_id(mut self, trader_id: TraderId) -> Self {
        self.trader_id = trader_id;
        self
    }
}
