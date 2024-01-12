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

use pyo3::prelude::*;

pub mod backend;
pub mod wranglers;

/// Loaded as nautilus_pyo3.persistence
#[pymodule]
pub fn persistence(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<crate::backend::session::DataBackendSession>()?;
    m.add_class::<crate::backend::session::DataQueryResult>()?;
    m.add_class::<backend::session::NautilusDataType>()?;
    m.add_class::<backend::transformer::DataTransformer>()?;
    m.add_class::<wranglers::bar::BarDataWrangler>()?;
    m.add_class::<wranglers::delta::OrderBookDeltaDataWrangler>()?;
    m.add_class::<wranglers::quote::QuoteTickDataWrangler>()?;
    m.add_class::<wranglers::trade::TradeTickDataWrangler>()?;
    Ok(())
}
