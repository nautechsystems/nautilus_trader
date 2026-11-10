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

//! Decoding for consolidated OHLCV files.

use std::{collections::HashMap, path::Path};

use ahash::AHashMap;
use nautilus_model::{
    data::{Bar, BarType},
    enums::AggregationSource,
    identifiers::InstrumentId,
    types::fixed::FIXED_PRECISION,
};
use rust_decimal::{Decimal, RoundingStrategy};

use super::rows::{Precision, Rows, date_time, number, text};
use crate::{
    common::enums::OhlcvPeriod,
    websocket::parse::{period_id_to_bar_spec, price_at, quantity_at},
};

pub(crate) fn load_bars(
    path: impl AsRef<Path>,
    instrument_ids: &HashMap<u64, (InstrumentId, u8)>,
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

    let spec = period_id_to_bar_spec(OhlcvPeriod::from(period_id))
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
    let mut precisions: AHashMap<InstrumentId, Precision> = AHashMap::new();

    while limit.is_none_or(|limit| bars.len() < limit) {
        let Some(record) = rows.next()? else {
            break;
        };

        let Some(&(instrument_id, price_precision)) = text(record, symbol_id)?
            .parse::<u64>()
            .ok()
            .and_then(|id| instrument_ids.get(&id))
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

        let size = precisions
            .entry(instrument_id)
            .or_insert(Precision::new(None));
        size.fit(volume);

        let ts_event = date_time(text(record, time_period_end)?, "time_period_end")?;
        bars.push(Bar::new_checked(
            BarType::new(instrument_id, spec, AggregationSource::External),
            price_at(open, price_precision, "px_open")?,
            price_at(high, price_precision, "px_high")?,
            price_at(low, price_precision, "px_low")?,
            price_at(close, price_precision, "px_close")?,
            quantity_at(volume, size.value, "sx_sum")?,
            ts_event,
            ts_event,
        )?);
    }

    // Volumes can be finer than the trading lot increment and retain their observed precision
    for bar in &mut bars {
        let size = precisions[&bar.instrument_id()];
        bar.volume.precision = size.value;
    }

    Ok(bars)
}

fn file_period_id(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".csv.gz")
        .or_else(|| name.strip_suffix(".csv"))?;
    stem.strip_prefix("TP-")
}
