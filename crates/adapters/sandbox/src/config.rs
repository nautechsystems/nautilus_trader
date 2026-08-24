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

//! Configuration for sandbox execution client.

use ahash::AHashMap;
use nautilus_execution::{
    matching_engine::config::OrderMatchingEngineConfig,
    models::{fee::FeeModelAny, fill::FillModelAny},
};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, InstrumentId, Venue},
    types::{Currency, Money},
};
use rust_decimal::Decimal;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IgnoredAny},
};

/// Configuration for `SandboxExecutionClient` instances.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.sandbox", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.sandbox")
)]
pub struct SandboxExecutionClientConfig {
    /// The account ID for this client.
    #[builder(default = AccountId::from("SANDBOX-001"))]
    pub account_id: AccountId,
    /// The venue for this sandbox execution client.
    #[builder(default = Venue::new("SANDBOX"))]
    pub venue: Venue,
    /// The starting balances for this sandbox venue.
    #[builder(default)]
    pub starting_balances: Vec<Money>,
    /// The base currency for this venue (None for multi-currency).
    pub base_currency: Option<Currency>,
    /// The order management system type used by the exchange.
    #[builder(default = OmsType::Netting)]
    pub oms_type: OmsType,
    /// The account type for the client.
    #[builder(default = AccountType::Margin)]
    pub account_type: AccountType,
    /// The account default leverage (for margin accounts).
    #[builder(default = Decimal::ONE)]
    pub default_leverage: Decimal,
    /// Per-instrument leverage overrides.
    #[builder(default)]
    pub leverages: AHashMap<InstrumentId, Decimal>,
    /// The order book type for the matching engine.
    #[builder(default = BookType::L1_MBP)]
    pub book_type: BookType,
    /// The fee model for sandbox matching engines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_fee_model",
        deserialize_with = "deserialize_fee_model"
    )]
    pub fee_model: Option<FeeModelAny>,
    /// The fill model for sandbox matching engines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_fill_model",
        deserialize_with = "deserialize_fill_model"
    )]
    pub fill_model: Option<FillModelAny>,
    /// If True, account balances won't change (frozen).
    #[builder(default)]
    pub frozen_account: bool,
    /// If bars should be processed by the matching engine (and move the market).
    #[builder(default = true)]
    pub bar_execution: bool,
    /// If trades should be processed by the matching engine (and move the market).
    #[builder(default = true)]
    pub trade_execution: bool,
    /// If stop orders are rejected on submission if trigger price is in the market.
    #[builder(default = true)]
    pub reject_stop_orders: bool,
    /// If orders with GTD time in force will be supported by the venue.
    #[builder(default = true)]
    pub support_gtd_orders: bool,
    /// If contingent orders will be supported/respected by the venue.
    #[builder(default = true)]
    pub support_contingent_orders: bool,
    /// If venue position IDs will be generated on order fills.
    #[builder(default = true)]
    pub use_position_ids: bool,
    /// If venue order IDs and position IDs will be random UUID4's.
    /// Trade IDs are always deterministic and not affected by this flag.
    #[builder(default)]
    pub use_random_ids: bool,
    /// If the `reduce_only` execution instruction on orders will be honored.
    #[builder(default = true)]
    pub use_reduce_only: bool,
    /// If limit order queue position tracking is enabled during trade execution.
    #[builder(default)]
    pub queue_position: bool,
    /// If order book liquidity consumption should be tracked per level.
    #[builder(default)]
    pub liquidity_consumption: bool,
    /// If bar high/low processing order adapts to the bar's shape.
    #[builder(default)]
    pub bar_adaptive_high_low_ordering: bool,
    /// If `OrderAccepted` events should be generated for market orders.
    #[builder(default)]
    pub use_market_order_acks: bool,
    /// If OTO child orders wait for a full parent fill before release.
    #[builder(default)]
    pub oto_full_trigger: bool,
    /// Exchange-calculated price boundary for aggressive market fills.
    ///
    /// A value of `0` disables protection.
    #[builder(default)]
    pub price_protection_points: u32,
}

impl SandboxExecutionClientConfig {
    /// Creates an [`OrderMatchingEngineConfig`] from this sandbox config.
    #[must_use]
    pub fn to_matching_engine_config(&self) -> OrderMatchingEngineConfig {
        let price_protection = if self.price_protection_points == 0 {
            None
        } else {
            Some(self.price_protection_points)
        };

        OrderMatchingEngineConfig::builder()
            .bar_execution(self.bar_execution)
            .bar_adaptive_high_low_ordering(self.bar_adaptive_high_low_ordering)
            .trade_execution(self.trade_execution)
            .liquidity_consumption(self.liquidity_consumption)
            .reject_stop_orders(self.reject_stop_orders)
            .support_gtd_orders(self.support_gtd_orders)
            .support_contingent_orders(self.support_contingent_orders)
            .use_position_ids(self.use_position_ids)
            .use_random_ids(self.use_random_ids)
            .use_reduce_only(self.use_reduce_only)
            .use_market_order_acks(self.use_market_order_acks)
            .queue_position(self.queue_position)
            .oto_full_trigger(self.oto_full_trigger)
            .maybe_price_protection_points(price_protection)
            .build()
    }
}

impl Default for SandboxExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

fn serialize_fee_model<S>(fee_model: &Option<FeeModelAny>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match fee_model {
        None => serializer.serialize_none(),
        Some(_) => Err(serde::ser::Error::custom(
            "SandboxExecutionClientConfig.fee_model is runtime-only and cannot be serialized",
        )),
    }
}

fn deserialize_fee_model<'de, D>(deserializer: D) -> Result<Option<FeeModelAny>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<IgnoredAny>::deserialize(deserializer)?;

    match value {
        None => Ok(None),
        Some(_) => Err(de::Error::custom(
            "SandboxExecutionClientConfig.fee_model must be configured at runtime, not deserialized",
        )),
    }
}

fn serialize_fill_model<S>(
    fill_model: &Option<FillModelAny>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match fill_model {
        None => serializer.serialize_none(),
        Some(_) => Err(serde::ser::Error::custom(
            "SandboxExecutionClientConfig.fill_model is runtime-only and cannot be serialized",
        )),
    }
}

fn deserialize_fill_model<'de, D>(deserializer: D) -> Result<Option<FillModelAny>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<IgnoredAny>::deserialize(deserializer)?;

    match value {
        None => Ok(None),
        Some(_) => Err(de::Error::custom(
            "SandboxExecutionClientConfig.fill_model must be configured at runtime, not deserialized",
        )),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_execution::models::{
        fee::{FeeModelAny, ProbabilityPriceFeeModel},
        fill::FillModelAny,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: SandboxExecutionClientConfig = toml::from_str("").unwrap();
        let expected = SandboxExecutionClientConfig::default();
        assert_eq!(config.account_id, expected.account_id);
        assert_eq!(config.venue, expected.venue);
        assert_eq!(config.oms_type, expected.oms_type);
        assert_eq!(config.account_type, expected.account_type);
        assert_eq!(config.default_leverage, expected.default_leverage);
        assert_eq!(config.book_type, expected.book_type);
        assert!(config.fee_model.is_none());
        assert!(config.fill_model.is_none());
        assert_eq!(config.bar_execution, expected.bar_execution);
        assert_eq!(config.trade_execution, expected.trade_execution);
        assert_eq!(config.use_position_ids, expected.use_position_ids);
        assert!(!config.queue_position);
        assert!(!config.liquidity_consumption);
        assert!(!config.bar_adaptive_high_low_ordering);
        assert!(!config.use_market_order_acks);
        assert!(!config.oto_full_trigger);
        assert_eq!(config.price_protection_points, 0);
    }

    #[rstest]
    fn test_to_matching_engine_config_forwards_matching_knobs() {
        let config = SandboxExecutionClientConfig {
            queue_position: true,
            liquidity_consumption: true,
            bar_adaptive_high_low_ordering: true,
            use_market_order_acks: true,
            oto_full_trigger: true,
            price_protection_points: 100,
            ..SandboxExecutionClientConfig::default()
        };
        let engine_config = config.to_matching_engine_config();

        assert!(engine_config.queue_position);
        assert!(engine_config.liquidity_consumption);
        assert!(engine_config.bar_adaptive_high_low_ordering);
        assert!(engine_config.use_market_order_acks);
        assert!(engine_config.oto_full_trigger);
        assert_eq!(engine_config.price_protection_points, Some(100));
    }

    #[rstest]
    fn test_exec_config_toml_rejects_fill_model_field() {
        let result =
            toml::from_str::<SandboxExecutionClientConfig>("fill_model = \"runtime-only\"");

        assert!(result.is_err());
    }

    #[rstest]
    fn test_exec_config_toml_rejects_fee_model_field() {
        let result = toml::from_str::<SandboxExecutionClientConfig>("fee_model = \"runtime-only\"");

        assert!(result.is_err());
    }

    #[rstest]
    fn test_exec_config_toml_rejects_serializing_runtime_fee_model() {
        let config = SandboxExecutionClientConfig {
            fee_model: Some(FeeModelAny::ProbabilityPrice(ProbabilityPriceFeeModel)),
            ..SandboxExecutionClientConfig::default()
        };

        let result = toml::Value::try_from(&config);

        assert!(result.is_err());
    }

    #[rstest]
    fn test_exec_config_toml_rejects_serializing_runtime_fill_model() {
        let config = SandboxExecutionClientConfig {
            fill_model: Some(FillModelAny::Default(Default::default())),
            ..SandboxExecutionClientConfig::default()
        };

        let result = toml::Value::try_from(&config);

        assert!(result.is_err());
    }
}
