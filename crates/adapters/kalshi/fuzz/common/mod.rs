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

//! Shared fixture loaders for Kalshi fuzz targets.

#![allow(dead_code)]

use std::collections::HashMap;

use nautilus_core::UnixNanos;
use nautilus_kalshi::http::{
    models::{Event, Market},
    parse::parse_instrument,
};
use nautilus_model::{identifiers::InstrumentId, instruments::BinaryOption};
use serde_json::Value;

pub(crate) const MAX_INPUT: usize = 4_096;

pub(crate) fn capped(data: &[u8]) -> Option<&[u8]> {
    data.get(..data.len().min(MAX_INPUT))
}

pub(crate) fn instrument(ticker: Option<&str>) -> BinaryOption {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
    let mut market: Market =
        serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();

    if let Some(ticker) = ticker {
        market.ticker = ticker.to_owned();
    }

    let event: Event =
        serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
    parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
}

pub(crate) fn instruments(instrument: BinaryOption) -> HashMap<InstrumentId, BinaryOption> {
    HashMap::from([(instrument.id, instrument)])
}
