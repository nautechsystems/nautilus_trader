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

//! `T-OHLCV_ACTIVE_CONSOLIDATED` files.

use std::path::Path;

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType},
    enums::AggregationSource,
    identifiers::InstrumentId,
    instruments::Instrument,
    types::fixed::FIXED_PRECISION,
};
use rust_decimal::{Decimal, RoundingStrategy};
use ustr::Ustr;

use super::rows::{Precision, Rows, date_time, number, text};
use crate::{
    http::{models::Symbol, parse::parse_instruments},
    websocket::parse::{period_id_to_bar_spec, price_at, quantity_at},
};

/// Loads [`Bar`]s from exchange-wide `T-OHLCV_ACTIVE_CONSOLIDATED` files.
///
/// Each row names its symbol by the numeric `symbol_id`, the REST `symbol_id_int`, so the loader
/// holds the instrument each one maps to and skips rows of unmapped symbols. Infers each
/// instrument's price and size precisions from its values. Captured `sx_sum` values can carry
/// numeric tails, so the loader rounds each volume half to even at the maximum precision.
/// It rejects negative volumes and positive volumes rounded to zero.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.coinapi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")
)]
pub struct CoinApiOhlcvLoader {
    instrument_ids: AHashMap<u64, InstrumentId>,
}

impl CoinApiOhlcvLoader {
    /// Creates a loader mapping each numeric `symbol_id` to an instrument.
    #[must_use]
    pub fn new(instrument_ids: impl IntoIterator<Item = (u64, InstrumentId)>) -> Self {
        Self {
            instrument_ids: instrument_ids.into_iter().collect(),
        }
    }

    /// Creates a loader for the instruments that CoinAPI symbol metadata maps to.
    ///
    /// Skips symbols without a `symbol_id_int` or a native instrument.
    #[must_use]
    pub fn from_symbols(symbols: &[Symbol]) -> Self {
        let (instruments, _) = parse_instruments(symbols, UnixNanos::default());
        let symbol_ids: AHashMap<Ustr, u64> = symbols
            .iter()
            .filter_map(|symbol| Some((symbol.symbol_id, symbol.symbol_id_int?)))
            .collect();

        Self::new(instruments.iter().filter_map(|loaded| {
            let symbol_id = *symbol_ids.get(&loaded.symbol_id)?;
            Some((symbol_id, loaded.instrument.id()))
        }))
    }

    /// Loads the bars of mapped symbols from the file at `path`.
    ///
    /// The bar period comes from `period_id`, such as `1MIN`, or from the file's `TP-` name when
    /// `period_id` is `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the period is unknown, the file cannot be read, a column is missing, or
    /// a mapped row is invalid.
    pub fn load_bars(
        &self,
        path: impl AsRef<Path>,
        period_id: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Bar>> {
        let path = path.as_ref();

        let period_id = match period_id {
            Some(period_id) => period_id,
            None => file_period_id(path).ok_or_else(|| {
                anyhow::anyhow!("no period given and '{}' is not a TP- file", path.display())
            })?,
        };

        let spec = period_id_to_bar_spec(Ustr::from(period_id))
            .ok_or_else(|| anyhow::anyhow!("unsupported period {period_id}"))?;

        let mut rows = Rows::open(path)?;
        let symbol_id = rows.column("symbol_id")?;
        let time_period_end = rows.column("time_period_end")?;
        let px_open = rows.column("px_open")?;
        let px_high = rows.column("px_high")?;
        let px_low = rows.column("px_low")?;
        let px_close = rows.column("px_close")?;
        let sx_sum = rows.column("sx_sum")?;

        let mut bars = Vec::new();
        let mut precisions: AHashMap<InstrumentId, (Precision, Precision)> = AHashMap::new();

        while limit.is_none_or(|limit| bars.len() < limit) {
            let Some(record) = rows.next()? else {
                break;
            };

            let Some(&instrument_id) = text(record, symbol_id)?
                .parse::<u64>()
                .ok()
                .and_then(|id| self.instrument_ids.get(&id))
            else {
                continue;
            };

            let open = number(record, px_open, "px_open")?;
            let high = number(record, px_high, "px_high")?;
            let low = number(record, px_low, "px_low")?;
            let close = number(record, px_close, "px_close")?;
            let source_volume = number(record, sx_sum, "sx_sum")?;
            anyhow::ensure!(
                source_volume >= Decimal::ZERO,
                "sx_sum must be non-negative: {source_volume}"
            );
            let volume = source_volume.round_dp_with_strategy(
                u32::from(FIXED_PRECISION),
                RoundingStrategy::MidpointNearestEven,
            );
            anyhow::ensure!(
                source_volume == Decimal::ZERO || volume > Decimal::ZERO,
                "sx_sum {source_volume} rounds to zero at precision {FIXED_PRECISION}"
            );

            let (price, size) = precisions
                .entry(instrument_id)
                .or_insert((Precision::new(None), Precision::new(None)));

            for value in [open, high, low, close] {
                price.fit(value);
            }

            size.fit(volume);

            let ts_event = date_time(text(record, time_period_end)?, "time_period_end")?;
            bars.push(Bar::new_checked(
                BarType::new(instrument_id, spec, AggregationSource::External),
                price_at(open, price.value, "px_open")?,
                price_at(high, price.value, "px_high")?,
                price_at(low, price.value, "px_low")?,
                price_at(close, price.value, "px_close")?,
                quantity_at(volume, size.value, "sx_sum")?,
                ts_event,
                ts_event,
            )?);
        }

        // Bars converted before a finer value keep their exact raw value at the final precision
        for bar in &mut bars {
            let (price, size) = precisions[&bar.instrument_id()];
            bar.open.precision = price.value;
            bar.high.precision = price.value;
            bar.low.precision = price.value;
            bar.close.precision = price.value;
            bar.volume.precision = size.value;
        }

        Ok(bars)
    }
}

fn file_period_id(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".csv.gz")
        .or_else(|| name.strip_suffix(".csv"))?;
    stem.strip_prefix("TP-")
}
