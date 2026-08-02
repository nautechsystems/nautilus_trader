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

//! Binance instrument loading filters.

use std::str::FromStr;

use ahash::AHashSet;
use nautilus_model::identifiers::InstrumentId;

use crate::config::BinanceInstrumentProviderConfig;

/// Normalized Binance instrument selector.
#[derive(Debug, Clone)]
pub struct BinanceInstrumentSelector {
    load_all: bool,
    load_ids: AHashSet<InstrumentId>,
    symbols: Option<AHashSet<String>>,
    bases: Option<AHashSet<String>>,
    quotes: Option<AHashSet<String>>,
    contract_types: Option<AHashSet<String>>,
}

impl BinanceInstrumentSelector {
    /// Creates a selector from validated provider configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a configured instrument ID or filter value is malformed.
    pub fn new(config: &BinanceInstrumentProviderConfig) -> anyhow::Result<Self> {
        let load_ids = config
            .load_ids
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|value| InstrumentId::from_str(value))
            .collect::<Result<AHashSet<_>, _>>()?;

        Ok(Self {
            load_all: config.load_all,
            load_ids,
            symbols: filter_values(config, "symbols")?,
            bases: filter_values(config, "bases")?,
            quotes: filter_values(config, "quotes")?,
            contract_types: filter_values(config, "contract_types")?,
        })
    }

    /// Returns whether the definition passes startup selection and venue filters.
    #[must_use]
    pub fn includes(
        &self,
        instrument_id: InstrumentId,
        symbol: &str,
        base: &str,
        quote: &str,
        contract_type: Option<&str>,
    ) -> bool {
        if !self.load_all && !self.load_ids.contains(&instrument_id) {
            return false;
        }

        matches_filter(&self.symbols, symbol)
            && matches_filter(&self.bases, base)
            && matches_filter(&self.quotes, quote)
            && self
                .contract_types
                .as_ref()
                .is_none_or(|values| contract_type.is_some_and(|value| contains(values, value)))
    }
}

fn filter_values(
    config: &BinanceInstrumentProviderConfig,
    key: &str,
) -> anyhow::Result<Option<AHashSet<String>>> {
    let Some(value) = config.filters.get(key) else {
        return Ok(None);
    };

    let values = match value {
        serde_json::Value::String(value) => vec![value.as_str()],
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("filter {key:?} contains a non-string value"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        _ => anyhow::bail!("filter {key:?} is not a string or array"),
    };

    Ok(Some(
        values
            .into_iter()
            .map(|value| value.trim().to_ascii_uppercase())
            .collect(),
    ))
}

fn matches_filter(values: &Option<AHashSet<String>>, value: &str) -> bool {
    values.as_ref().is_none_or(|values| contains(values, value))
}

fn contains(values: &AHashSet<String>, value: &str) -> bool {
    values.contains(&value.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_selector_load_ids_and_filters_are_both_discriminating() {
        let config = BinanceInstrumentProviderConfig {
            load_all: false,
            load_ids: Some(vec!["ETHUSDT-PERP.BINANCE".to_string()]),
            filters: HashMap::from([
                ("bases".to_string(), serde_json::json!(["eth"])),
                ("quotes".to_string(), serde_json::json!("usdt")),
                (
                    "contract_types".to_string(),
                    serde_json::json!(["PERPETUAL"]),
                ),
            ]),
            ..Default::default()
        };
        let selector = BinanceInstrumentSelector::new(&config).unwrap();

        assert!(selector.includes(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            "ETHUSDT",
            "ETH",
            "USDT",
            Some("PERPETUAL"),
        ));
        assert!(!selector.includes(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            "BTCUSDT",
            "BTC",
            "USDT",
            Some("PERPETUAL"),
        ));
        assert!(!selector.includes(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            "ETHUSDT",
            "ETH",
            "USDT",
            Some("CURRENT_QUARTER"),
        ));
    }

    #[rstest]
    fn test_selector_load_all_still_applies_venue_filters() {
        let config = BinanceInstrumentProviderConfig {
            filters: HashMap::from([("symbols".to_string(), serde_json::json!(["btcusdt"]))]),
            ..Default::default()
        };
        let selector = BinanceInstrumentSelector::new(&config).unwrap();

        assert!(selector.includes(
            InstrumentId::from("BTCUSDT.BINANCE"),
            "BTCUSDT",
            "BTC",
            "USDT",
            None,
        ));
        assert!(!selector.includes(
            InstrumentId::from("ETHUSDT.BINANCE"),
            "ETHUSDT",
            "ETH",
            "USDT",
            None,
        ));
    }
}
