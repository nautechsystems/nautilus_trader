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

use std::{collections::HashMap, sync::Arc};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{HasTsInit, bar::BarType, custom::CustomDataTrait},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Represents a Binance bar (kline/candlestick) with additional Binance-specific fields.
///
/// Extends the core `Bar` fields with `quote_volume`, `count`,
/// `taker_buy_base_volume`, and `taker_buy_quote_volume`.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.binance")
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BinanceBar {
    /// The bar type for this bar.
    pub bar_type: BarType,
    /// The bars open price.
    pub open: Price,
    /// The bars high price.
    pub high: Price,
    /// The bars low price.
    pub low: Price,
    /// The bars close price.
    pub close: Price,
    /// The bars volume.
    pub volume: Quantity,
    /// The quote asset volume.
    pub quote_volume: Decimal,
    /// The number of trades.
    pub count: u64,
    /// Taker buy base asset volume.
    pub taker_buy_base_volume: Decimal,
    /// Taker buy quote asset volume.
    pub taker_buy_quote_volume: Decimal,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl BinanceBar {
    /// Creates a new [`BinanceBar`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        quote_volume: Decimal,
        count: u64,
        taker_buy_base_volume: Decimal,
        taker_buy_quote_volume: Decimal,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            quote_volume,
            count,
            taker_buy_base_volume,
            taker_buy_quote_volume,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(bar_type: &BarType) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("bar_type".to_string(), bar_type.to_string());
        metadata.insert(
            "instrument_id".to_string(),
            bar_type.instrument_id().to_string(),
        );
        metadata
    }

    /// Returns the taker sell base asset volume.
    #[must_use]
    pub fn taker_sell_base_volume(&self) -> Decimal {
        Decimal::from(self.volume.raw) / Decimal::new(10i64.pow(self.volume.precision.into()), 0)
            - self.taker_buy_base_volume
    }

    /// Returns the taker sell quote asset volume.
    #[must_use]
    pub fn taker_sell_quote_volume(&self) -> Decimal {
        self.quote_volume - self.taker_buy_quote_volume
    }
}

impl HasTsInit for BinanceBar {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceBar {
    fn type_name(&self) -> &'static str {
        "BinanceBar"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceBar"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        // Price/Quantity deserialize from borrowed &str, so we must go through
        // a string representation rather than serde_json::from_value which
        // produces owned strings.
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn stub_binance_bar() -> BinanceBar {
        BinanceBar::new(
            BarType::from("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("0.01634790"),
            Price::from("0.01640000"),
            Price::from("0.01575800"),
            Price::from("0.01577100"),
            Quantity::from("148976.11427815"),
            dec!(2434.19055334),
            100,
            dec!(1756.87402397),
            dec!(28.46694368),
            UnixNanos::from(1_650_000_000_000_000_000u64),
            UnixNanos::from(1_650_000_000_000_000_000u64),
        )
    }

    #[rstest]
    fn test_type_name() {
        let bar = stub_binance_bar();
        assert_eq!(bar.type_name(), "BinanceBar");
        assert_eq!(BinanceBar::type_name_static(), "BinanceBar");
    }

    #[rstest]
    fn test_taker_sell_quote_volume() {
        let bar = stub_binance_bar();
        assert_eq!(bar.taker_sell_quote_volume(), dec!(2405.72360966));
    }

    #[rstest]
    fn test_json_round_trip() {
        let bar = stub_binance_bar();
        let json = bar.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let restored = BinanceBar::from_json(value).unwrap();
        let restored_bar = restored.as_any().downcast_ref::<BinanceBar>().unwrap();
        assert_eq!(restored_bar, &bar);
    }
}
