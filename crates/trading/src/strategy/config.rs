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

use std::collections::HashMap;

use nautilus_common::config::{ConfigError, ConfigErrorCollector, ConfigResult};
use nautilus_core::serialization::{default_false, default_true};
use nautilus_model::{
    enums::{OmsType, TimeInForce},
    identifiers::{InstrumentId, StrategyId, check_order_id_tag},
};
use serde::{Deserialize, Serialize};

// Upper bound for `market_exit_interval_ms` so the nanosecond conversion on the market exit
// timer path (`interval_ms * 1_000_000`) cannot overflow a `u64`.
const MAX_MARKET_EXIT_INTERVAL_MS: u64 = u64::MAX / 1_000_000;

/// The base model for all trading strategy configurations.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Clone, Debug, Deserialize, Serialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.trading", subclass, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct StrategyConfig {
    /// The unique ID for the strategy. Will become the strategy ID if not None.
    pub strategy_id: Option<StrategyId>,
    /// The unique order ID tag for the strategy. Must be unique
    /// amongst all running strategies for a particular trader ID, and cannot contain the '-'
    /// strategy ID separator.
    pub order_id_tag: Option<String>,
    /// If UUID4's should be used for client order ID values.
    #[serde(default = "default_false")]
    #[builder(default)]
    pub use_uuid_client_order_ids: bool,
    /// If hyphens should be used in generated client order ID values.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub use_hyphens_in_client_order_ids: bool,
    /// The order management system type for the strategy. This will determine
    /// how the `ExecutionEngine` handles position IDs.
    pub oms_type: Option<OmsType>,
    /// Instrument IDs the strategy intends to claim for external orders, fills, and materialized
    /// reconciliation activity when registered.
    pub external_order_instrument_ids: Option<Vec<InstrumentId>>,
    /// If OTO, OCO, and OUO **open** contingent orders should be managed automatically by the strategy.
    /// Any emulated orders which are active local will be managed by the `OrderEmulator` instead.
    #[serde(default = "default_false")]
    #[builder(default)]
    pub manage_contingent_orders: bool,
    /// If all order GTD time in force expirations should be managed by the strategy.
    /// If True, then will ensure open orders have their GTD timers re-activated on start.
    #[serde(default = "default_false")]
    #[builder(default)]
    pub manage_gtd_expiry: bool,
    /// If the strategy should automatically perform a market exit when stopped.
    /// If true, calling `stop()` first cancels all orders and closes all positions
    /// before the strategy transitions to the `STOPPED` state.
    #[serde(default = "default_false")]
    #[builder(default)]
    pub manage_stop: bool,
    /// The interval in milliseconds to check for in-flight orders and open positions
    /// during a market exit.
    #[serde(default = "default_market_exit_interval_ms")]
    #[builder(default = 100)]
    pub market_exit_interval_ms: u64,
    /// The maximum number of attempts to wait for orders and positions to close
    /// during a market exit before completing. Defaults to 100 attempts
    /// (10 seconds at 100ms intervals).
    #[serde(default = "default_market_exit_max_attempts")]
    #[builder(default = 100)]
    pub market_exit_max_attempts: u64,
    /// The time in force for closing market orders during a market exit.
    #[serde(default = "default_market_exit_time_in_force")]
    #[builder(default = TimeInForce::Gtc)]
    pub market_exit_time_in_force: TimeInForce,
    /// If closing market orders during a market exit should be reduce only.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub market_exit_reduce_only: bool,
    /// If events should be logged by the strategy.
    /// If False, then only warning events and above are logged.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub log_events: bool,
    /// If commands should be logged by the strategy.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub log_commands: bool,
    /// If order rejected events where `due_post_only` is True should be logged as warnings.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub log_rejected_due_post_only_as_warning: bool,
}

const fn default_market_exit_interval_ms() -> u64 {
    100
}

const fn default_market_exit_max_attempts() -> u64 {
    100
}

const fn default_market_exit_time_in_force() -> TimeInForce {
    TimeInForce::Gtc
}

impl<S: strategy_config_builder::IsComplete> StrategyConfigBuilder<S> {
    /// Validates and builds the [`StrategyConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`StrategyConfig::validate`]).
    pub fn build(self) -> ConfigResult<StrategyConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl StrategyConfig {
    /// Validates the strategy configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if let Some(order_id_tag) = &self.order_id_tag
            && let Err(e) = check_order_id_tag(order_id_tag)
        {
            errors.push(ConfigError::invalid_value("order_id_tag", e.to_string()));
        }

        let interval_ms = self.market_exit_interval_ms;
        errors.check(
            interval_ms > 0,
            ConfigError::range(
                "market_exit_interval_ms",
                format!("must be a positive number of milliseconds, was {interval_ms}"),
            ),
        );
        errors.check(
            interval_ms <= MAX_MARKET_EXIT_INTERVAL_MS,
            ConfigError::range(
                "market_exit_interval_ms",
                format!(
                    "must be at most {MAX_MARKET_EXIT_INTERVAL_MS} milliseconds to convert to \
                    nanoseconds without overflow, was {interval_ms}"
                ),
            ),
        );

        let max_attempts = self.market_exit_max_attempts;
        errors.check(
            max_attempts > 0,
            ConfigError::range(
                "market_exit_max_attempts",
                format!("must be a positive number of attempts, was {max_attempts}"),
            ),
        );

        let time_in_force = self.market_exit_time_in_force;
        errors.check(
            time_in_force != TimeInForce::Gtd,
            ConfigError::unsupported_value(
                "market_exit_time_in_force",
                format!("{time_in_force} is not supported for market orders"),
            ),
        );

        errors.into_result()
    }
}

/// Configuration for creating strategies from importable paths.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct ImportableStrategyConfig {
    /// The fully qualified name of the Strategy class.
    pub strategy_path: String,
    /// The fully qualified name of the Strategy config class.
    pub config_path: String,
    /// The strategy configuration as a dictionary.
    pub config: HashMap<String, serde_json::Value>,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default `StrategyConfig` should be valid")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    #[rstest]
    fn test_default_config_is_valid() {
        assert!(StrategyConfig::builder().build().is_ok());
    }

    #[rstest]
    fn test_zero_market_exit_interval_rejected() {
        let result = StrategyConfig::builder().market_exit_interval_ms(0).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "market_exit_interval_ms")
        );
    }

    #[rstest]
    fn test_market_exit_interval_above_nanosecond_bound_rejected() {
        let result = StrategyConfig::builder()
            .market_exit_interval_ms(18_446_744_073_710)
            .build();
        let Err(ConfigError::Range { field, reason }) = result else {
            panic!("expected ConfigError::Range");
        };
        assert_eq!(field, "market_exit_interval_ms");
        assert_eq!(
            reason,
            "must be at most 18446744073709 milliseconds to convert to nanoseconds without overflow, \
            was 18446744073710"
        );
    }

    #[rstest]
    fn test_market_exit_interval_at_nanosecond_bound_accepted() {
        let config = StrategyConfig::builder()
            .market_exit_interval_ms(18_446_744_073_709)
            .build();

        assert!(config.is_ok());
    }

    #[rstest]
    fn test_zero_market_exit_max_attempts_rejected() {
        let result = StrategyConfig::builder()
            .market_exit_max_attempts(0)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "market_exit_max_attempts")
        );
    }

    #[rstest]
    #[case("001")]
    #[case("ABC")]
    fn test_order_id_tag_without_separator_accepted(#[case] order_id_tag: &str) {
        let config = StrategyConfig::builder()
            .order_id_tag(order_id_tag.to_string())
            .build()
            .unwrap();

        assert_eq!(config.order_id_tag.as_deref(), Some(order_id_tag));
    }

    #[rstest]
    #[case("A-B")]
    #[case("XNAS-T01")]
    fn test_order_id_tag_with_separator_rejected(#[case] order_id_tag: &str) {
        let result = StrategyConfig::builder()
            .order_id_tag(order_id_tag.to_string())
            .build();

        let ConfigError::InvalidValue { field, reason } = result.unwrap_err() else {
            panic!("expected ConfigError::InvalidValue");
        };
        assert_eq!(field, "order_id_tag");
        assert_eq!(
            reason,
            format!(
                "`order_id_tag` cannot contain the '-' strategy ID separator, was '{order_id_tag}'"
            )
        );
    }

    #[rstest]
    fn test_gtd_market_exit_time_in_force_rejected() {
        let result = StrategyConfig::builder()
            .market_exit_time_in_force(TimeInForce::Gtd)
            .build();
        let Err(ConfigError::UnsupportedValue { field, reason }) = result else {
            panic!("expected ConfigError::UnsupportedValue");
        };
        assert_eq!(field, "market_exit_time_in_force");
        assert_eq!(reason, "GTD is not supported for market orders");
    }

    // Iterates the enum rather than listing cases, so a variant added later is covered
    // without editing this test: the invariant is that every time in force except GTD
    // is accepted, mirroring `MarketOrder::new_checked`.
    #[rstest]
    fn test_non_gtd_market_exit_time_in_force_accepted() {
        for time_in_force in TimeInForce::iter().filter(|t| *t != TimeInForce::Gtd) {
            assert!(
                StrategyConfig::builder()
                    .market_exit_time_in_force(time_in_force)
                    .build()
                    .is_ok(),
                "{time_in_force} should be accepted"
            );
        }
    }

    #[rstest]
    fn test_multiple_violations_collected() {
        let result = StrategyConfig::builder()
            .market_exit_interval_ms(0)
            .market_exit_max_attempts(0)
            .market_exit_time_in_force(TimeInForce::Gtd)
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        // Asserted by index, not membership: the collector preserves insertion order, so
        // checking position also pins that the new check runs after the two numeric ones.
        assert_eq!(errors.len(), 3);
        assert!(matches!(
            &errors[0],
            ConfigError::Range { field, .. } if field == "market_exit_interval_ms"
        ));
        assert!(matches!(
            &errors[1],
            ConfigError::Range { field, .. } if field == "market_exit_max_attempts"
        ));
        assert!(matches!(
            &errors[2],
            ConfigError::UnsupportedValue { field, .. } if field == "market_exit_time_in_force"
        ));
    }

    #[rstest]
    fn test_strategy_config_default() {
        let config = StrategyConfig::default();

        assert!(config.strategy_id.is_none());
        assert!(config.order_id_tag.is_none());
        assert!(!config.use_uuid_client_order_ids);
        assert!(config.use_hyphens_in_client_order_ids);
        assert!(config.oms_type.is_none());
        assert!(config.external_order_instrument_ids.is_none());
        assert!(!config.manage_contingent_orders);
        assert!(!config.manage_gtd_expiry);
        assert!(!config.manage_stop);
        assert_eq!(config.market_exit_interval_ms, 100);
        assert_eq!(config.market_exit_max_attempts, 100);
        assert_eq!(config.market_exit_time_in_force, TimeInForce::Gtc);
        assert!(config.market_exit_reduce_only);
        assert!(config.log_events);
        assert!(config.log_commands);
        assert!(config.log_rejected_due_post_only_as_warning);
    }

    #[rstest]
    fn test_strategy_config_with_strategy_id() {
        let strategy_id = StrategyId::from("TEST-001");
        let config = StrategyConfig {
            strategy_id: Some(strategy_id),
            ..Default::default()
        };

        assert_eq!(config.strategy_id, Some(strategy_id));
    }

    #[rstest]
    fn test_strategy_config_serialization() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("TAG1".to_string()),
            use_uuid_client_order_ids: true,
            external_order_instrument_ids: Some(vec![InstrumentId::from("AUDUSD.SIM")]),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: StrategyConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.strategy_id, deserialized.strategy_id);
        assert_eq!(config.order_id_tag, deserialized.order_id_tag);
        assert_eq!(
            config.use_uuid_client_order_ids,
            deserialized.use_uuid_client_order_ids
        );
        assert_eq!(
            config.external_order_instrument_ids,
            deserialized.external_order_instrument_ids
        );
    }
}
