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

//! Provides a configuration for `RiskEngine` instances.

use ahash::AHashMap;
use nautilus_common::{
    config::{ConfigError, ConfigErrorCollector, ConfigResult},
    throttler::RateLimit,
};
use nautilus_core::datetime::NANOSECONDS_IN_SECOND;
use nautilus_model::identifiers::InstrumentId;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for `RiskEngineConfig` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.risk", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.risk")
)]
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Debug, Clone, Deserialize, Serialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(default, deny_unknown_fields)]
pub struct RiskEngineConfig {
    #[builder(default)]
    pub bypass: bool,
    #[builder(default = RateLimit::new(100, NANOSECONDS_IN_SECOND))]
    pub max_order_submit: RateLimit,
    #[builder(default = RateLimit::new(100, NANOSECONDS_IN_SECOND))]
    pub max_order_modify: RateLimit,
    #[builder(default)]
    pub max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    #[builder(default)]
    pub debug: bool,
}

impl<S: risk_engine_config_builder::IsComplete> RiskEngineConfigBuilder<S> {
    /// Validates and builds the [`RiskEngineConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`RiskEngineConfig::validate`]).
    pub fn build(self) -> ConfigResult<RiskEngineConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl RiskEngineConfig {
    /// Validates the risk engine configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        for (instrument_id, notional) in &self.max_notional_per_order {
            errors.check(
                *notional > Decimal::ZERO,
                ConfigError::range(
                    "max_notional_per_order",
                    format!("notional for {instrument_id} must be positive, was {notional}"),
                ),
            );
        }

        errors.into_result()
    }
}

impl Default for RiskEngineConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default `RiskEngineConfig` should be valid")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config_is_valid() {
        assert!(RiskEngineConfig::builder().build().is_ok());
    }

    #[rstest]
    #[case(Decimal::ZERO)]
    #[case(Decimal::from(-1))]
    fn test_non_positive_notional_rejected(#[case] notional: Decimal) {
        let mut notionals = AHashMap::new();
        notionals.insert(InstrumentId::from("ESZ21.GLBX"), notional);
        let result = RiskEngineConfig::builder()
            .max_notional_per_order(notionals)
            .build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "max_notional_per_order")
        );
    }

    #[rstest]
    fn test_positive_notional_accepted() {
        let mut notionals = AHashMap::new();
        notionals.insert(InstrumentId::from("ESZ21.GLBX"), Decimal::from(1_000_000));
        let result = RiskEngineConfig::builder()
            .max_notional_per_order(notionals)
            .build();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_multiple_violations_collected() {
        let mut notionals = AHashMap::new();
        notionals.insert(InstrumentId::from("ESZ21.GLBX"), Decimal::ZERO);
        notionals.insert(InstrumentId::from("CLZ21.NYMEX"), Decimal::from(-1));
        let result = RiskEngineConfig::builder()
            .max_notional_per_order(notionals)
            .build();
        let ConfigError::Multiple { errors } = result.unwrap_err() else {
            panic!("expected ConfigError::Multiple");
        };
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(
            |e| matches!(e, ConfigError::Range { field, .. } if field == "max_notional_per_order")
        ));
    }
}
