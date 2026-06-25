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

use nautilus_core::correctness::{CorrectnessResultExt, FAILED, check_positive_usize};
use serde::{Deserialize, Deserializer, Serialize, de::Error};

use crate::{
    config::{ConfigError, ConfigErrorCollector, ConfigResult},
    enums::SerializationEncoding,
};

/// Configuration for `Cache` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    /// The encoding for database operations, controls the type of serializer used.
    #[builder(default = SerializationEncoding::Json)]
    pub encoding: SerializationEncoding,
    /// If timestamps should be persisted as ISO 8601 strings.
    #[builder(default)]
    pub timestamps_as_iso8601: bool,
    /// The buffer interval (milliseconds) between pipelined/batched transactions.
    pub buffer_interval_ms: Option<usize>,
    /// The batch size for bulk read operations (e.g., MGET).
    /// If set, bulk reads will be batched into chunks of this size.
    pub bulk_read_batch_size: Option<usize>,
    /// If a 'trader-' prefix is used for keys.
    #[builder(default = true)]
    pub use_trader_prefix: bool,
    /// If the trader's instance ID is used for keys.
    #[builder(default)]
    pub use_instance_id: bool,
    /// If the database should be flushed on start.
    #[builder(default)]
    pub flush_on_start: bool,
    /// If instrument data should be dropped from the cache's memory on reset.
    #[builder(default = true)]
    pub drop_instruments_on_reset: bool,
    /// The maximum length for internal tick deques.
    #[builder(default = 10_000)]
    #[serde(deserialize_with = "deserialize_positive_usize")]
    pub tick_capacity: usize,
    /// The maximum length for internal bar deques.
    #[builder(default = 10_000)]
    #[serde(deserialize_with = "deserialize_positive_usize")]
    pub bar_capacity: usize,
    /// If account events should be persisted to a backing database.
    #[builder(default = true)]
    pub persist_account_events: bool,
    /// If market data should be persisted to disk.
    #[builder(default)]
    pub save_market_data: bool,
}

impl<S: cache_config_builder::IsComplete> CacheConfigBuilder<S> {
    /// Validates and builds the [`CacheConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`CacheConfig::validate`]).
    pub fn build(self) -> ConfigResult<CacheConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("default `CacheConfig` should be valid")
    }
}

impl CacheConfig {
    /// Creates a new [`CacheConfig`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `tick_capacity` or `bar_capacity` is zero.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        encoding: SerializationEncoding,
        timestamps_as_iso8601: bool,
        buffer_interval_ms: Option<usize>,
        bulk_read_batch_size: Option<usize>,
        use_trader_prefix: bool,
        use_instance_id: bool,
        flush_on_start: bool,
        drop_instruments_on_reset: bool,
        tick_capacity: usize,
        bar_capacity: usize,
        persist_account_events: bool,
        save_market_data: bool,
    ) -> Self {
        check_positive_usize(tick_capacity, stringify!(tick_capacity)).expect_display(FAILED);
        check_positive_usize(bar_capacity, stringify!(bar_capacity)).expect_display(FAILED);

        Self {
            encoding,
            timestamps_as_iso8601,
            buffer_interval_ms,
            bulk_read_batch_size,
            use_trader_prefix,
            use_instance_id,
            flush_on_start,
            drop_instruments_on_reset,
            tick_capacity,
            bar_capacity,
            persist_account_events,
            save_market_data,
        }
    }

    /// Checks whether all cache settings are valid.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if a capacity setting is not positive.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        for (field, value) in [
            ("tick_capacity", self.tick_capacity),
            ("bar_capacity", self.bar_capacity),
        ] {
            errors.check(
                value > 0,
                ConfigError::range(field, format!("must be positive, was {value}")),
            );
        }

        errors.into_result()
    }
}

fn deserialize_positive_usize<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    check_positive_usize(value, "capacity").map_err(D::Error::custom)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_uses_json_encoding() {
        let config = CacheConfig::default();

        assert_eq!(config.encoding, SerializationEncoding::Json);
    }

    #[rstest]
    #[case(0, 1)]
    #[case(1, 0)]
    #[should_panic]
    fn test_new_rejects_zero_capacities(#[case] tick_capacity: usize, #[case] bar_capacity: usize) {
        let _ = CacheConfig::new(
            SerializationEncoding::MsgPack,
            false,
            None,
            None,
            true,
            false,
            false,
            true,
            tick_capacity,
            bar_capacity,
            true,
            false,
        );
    }

    #[rstest]
    fn test_builder_rejects_zero_tick_capacity() {
        let result = CacheConfig::builder().tick_capacity(0).build();
        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "tick_capacity")
        );
    }

    #[rstest]
    fn test_builder_rejects_zero_bar_capacity() {
        let result = CacheConfig::builder().bar_capacity(0).build();
        assert!(matches!(result, Err(ConfigError::Range { field, .. }) if field == "bar_capacity"));
    }

    #[rstest]
    #[case(0, 1, "tick_capacity")]
    #[case(1, 0, "bar_capacity")]
    fn test_validate_rejects_zero_capacities(
        #[case] tick_capacity: usize,
        #[case] bar_capacity: usize,
        #[case] expected_field: &str,
    ) {
        let config = CacheConfig {
            tick_capacity,
            bar_capacity,
            ..Default::default()
        };

        let err = config.validate().expect_err("zero capacity is invalid");

        assert!(matches!(err, ConfigError::Range { field, .. } if field == expected_field));
    }

    #[rstest]
    #[case(r#"{"tick_capacity":0}"#)]
    #[case(r#"{"bar_capacity":0}"#)]
    fn test_deserialize_rejects_zero_capacities(#[case] raw: &str) {
        let err = serde_json::from_str::<CacheConfig>(raw)
            .expect_err("zero capacity should fail deserialization");

        assert!(
            err.to_string()
                .contains("invalid usize for 'capacity' not positive")
        );
    }

    #[rstest]
    fn test_deserialize_uses_positive_default_capacities() {
        let config = serde_json::from_str::<CacheConfig>("{}").unwrap();

        assert_eq!(config.tick_capacity, 10_000);
        assert_eq!(config.bar_capacity, 10_000);
    }
}
