// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::prelude::*;
use strum::IntoEnumIterator;

use crate::enums::TardisExchange;

#[must_use]
#[pyfunction(name = "tardis_exchanges")]
pub fn py_tardis_exchanges() -> Vec<String> {
    TardisExchange::iter().map(|e| e.to_string()).collect()
}

#[must_use]
#[pyfunction(name = "tardis_exchange_from_venue_str")]
pub fn py_tardis_exchange_from_venue_str(venue_str: &str) -> Vec<String> {
    TardisExchange::from_venue_str(venue_str)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[must_use]
#[pyfunction(name = "tardis_exchange_to_venue_str")]
pub fn py_tardis_exchange_to_venue_str(exchange_str: &str) -> String {
    match exchange_str.parse::<TardisExchange>() {
        Ok(exchange) => exchange.as_venue_str().to_string(),
        Err(_) => String::new(),
    }
}

#[must_use]
#[pyfunction(name = "tardis_exchange_is_option_exchange")]
pub fn py_tardis_exchange_is_option_exchange(exchange_str: &str) -> bool {
    match exchange_str.parse::<TardisExchange>() {
        Ok(exchange) => exchange.is_option_exchange(),
        Err(_) => false,
    }
}
