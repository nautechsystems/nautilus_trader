// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

#![recursion_limit = "256"]
#[macro_use]
extern crate lazy_static;

use crate::enums::PriceType;
use pyo3::prelude::*;
use pyo3::{PyResult, Python};

pub mod currencies;
pub mod data;
pub mod enums;
pub mod events;
pub mod identifiers;
pub mod instruments;
pub mod orderbook;
pub mod orders;
pub mod position;
pub mod types;

/// Loaded as nautilus_model
#[pymodule]
pub fn model(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<PriceType>()?;
    Ok(())
}
