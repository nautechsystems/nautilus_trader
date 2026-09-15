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

use std::{path::PathBuf, sync::Arc};
use nautilus_core::UnixNanos;
use nautilus_model::{data::{CustomData, DataType, QuoteTick, stubs::{stub_bar, stub_depth10, stub_trade_ethusdt_buy}}, identifiers::InstrumentId, instruments::{InstrumentAny, stubs::audusd_sim}, types::{Price, Quantity}};
use nautilus_persistence::{backend::catalog::ParquetDataCatalog, test_data::RustTestCustomData};
use nautilus_serialization::arrow::custom::ensure_custom_data_registered;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).expect("destination"));
    std::fs::create_dir_all(&root)?;
    let catalog = ParquetDataCatalog::new(&root, None, None, None, None);
    let id = InstrumentId::from("AUD/USD.SIM");
    let ts = 1_700_000_000_000_000_123_u64;
    let quotes = [
        QuoteTick::new(id, Price::from("1.23456"), Price::from("1.23478"), Quantity::from("123.000001"), Quantity::from("456.000002"), UnixNanos::from(ts), UnixNanos::from(ts + 1)),
        QuoteTick::new(id, Price::from("1.34567"), Price::from("1.34589"), Quantity::from("789.000003"), Quantity::from("987.000004"), UnixNanos::from(ts + 2), UnixNanos::from(ts + 3)),
    ];
    catalog.write_to_parquet(&quotes, None, None, None)?;
    let mut trade = stub_trade_ethusdt_buy();
    trade.ts_event = UnixNanos::from(ts + 10); trade.ts_init = UnixNanos::from(ts + 11);
    catalog.write_to_parquet(&[trade], None, None, None)?;
    let mut bar = stub_bar();
    bar.ts_event = UnixNanos::from(ts + 20); bar.ts_init = UnixNanos::from(ts + 21);
    catalog.write_to_parquet(&[bar], None, None, None)?;
    let mut depth = stub_depth10();
    depth.ts_event = UnixNanos::from(ts + 30); depth.ts_init = UnixNanos::from(ts + 31);
    catalog.write_to_parquet(&[depth], None, None, None)?;
    let mut currency_pair = audusd_sim();
    currency_pair.ts_event = UnixNanos::from(ts + 50);
    currency_pair.ts_init = UnixNanos::from(ts + 51);
    let instrument = InstrumentAny::CurrencyPair(currency_pair);
    catalog.write_instruments(vec![instrument.clone()])?;
    ensure_custom_data_registered::<RustTestCustomData>();
    let custom = RustTestCustomData::new(id, 1.25, true, UnixNanos::from(ts + 40), UnixNanos::from(ts + 41));
    let wrapped = CustomData::new(Arc::new(custom.clone()), DataType::new("RustTestCustomData", None, Some(id.to_string())));
    catalog.write_custom_data_batch(vec![wrapped], None, None, None)?;
    let mut expected = serde_json::json!({"quotes": quotes, "trade": trade, "bar": bar, "depth": depth, "instrument": instrument, "custom": custom});
    let mut account = nautilus_model::events::account::stubs::cash_account_state();
    account.ts_event = UnixNanos::from(ts + 60);
    account.ts_init = UnixNanos::from(ts + 61);
    catalog.write_to_parquet(&[account.clone()], None, None, None)?;
    expected["account_state"] = serde_json::to_value(&account)?;
    std::fs::write(root.join("expected.json"), serde_json::to_vec_pretty(&expected)?)?;
    Ok(())
}
