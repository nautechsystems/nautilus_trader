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

use std::path::PathBuf;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    instruments::{InstrumentAny, stubs::audusd_sim},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;

fn main() -> anyhow::Result<()> {
    assert_eq!(std::mem::size_of_val(&Price::from("1.23456").raw), 8);
    let root = PathBuf::from(std::env::args().nth(1).expect("destination"));
    std::fs::create_dir_all(&root)?;
    let catalog = ParquetDataCatalog::new(&root, None, None, None, None);
    let id = InstrumentId::from("AUD/USD.SIM");
    let ts = 1_700_000_000_000_000_123_u64;
    let quotes = [
        QuoteTick::new(
            id,
            Price::from("1.23456"),
            Price::from("1.23478"),
            Quantity::from("123.000001"),
            Quantity::from("456.000002"),
            ts.into(),
            (ts + 1).into(),
        ),
        QuoteTick::new(
            id,
            Price::from("1.34567"),
            Price::from("1.34589"),
            Quantity::from("789.000003"),
            Quantity::from("987.000004"),
            (ts + 2).into(),
            (ts + 3).into(),
        ),
    ];
    catalog.write_to_parquet(
        &quotes,
        Some(UnixNanos::from(ts - 100)),
        Some(UnixNanos::from(ts + 3)),
        None,
    )?;
    let mut instrument = audusd_sim();
    instrument.ts_event = (ts + 10).into();
    instrument.ts_init = (ts + 11).into();
    catalog.write_instruments(vec![InstrumentAny::CurrencyPair(instrument.clone())])?;
    let expected = serde_json::json!({"quotes": quotes, "instrument": instrument});
    std::fs::write(
        root.join("expected.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    Ok(())
}
