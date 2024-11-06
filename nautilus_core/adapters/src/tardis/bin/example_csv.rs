// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::path::Path;

use nautilus_adapters::tardis;
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() {
    // You must specify precisions and the CSV filepath
    let price_precision = 1;
    let size_precision = 0;
    let filepath = Path::new("YOUR_CSV_DATA_PATH");

    // Optionally specify an instrument ID and/or limit
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let limit = None;

    // Consider propagating any parsing error depending on your workflow
    let _deltas = tardis::csv::load_deltas(
        filepath,
        price_precision,
        size_precision,
        Some(instrument_id),
        limit,
    )
    .unwrap();
}
