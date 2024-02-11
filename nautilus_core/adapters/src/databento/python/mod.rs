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

pub mod decode;
pub mod historical;
pub mod live;
pub mod loader;

use pyo3::prelude::*;

/// Loaded as nautilus_pyo3.databento
#[pymodule]
pub fn databento(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<super::types::DatabentoPublisher>()?;
    m.add_class::<super::loader::DatabentoDataLoader>()?;
    m.add_class::<live::DatabentoLiveClient>()?;
    m.add_class::<historical::DatabentoHistoricalClient>()?;
    m.add_function(wrap_pyfunction!(decode::py_decode_equity, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_futures_contract, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_options_contract, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_mbo_msg, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_trade_msg, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_mbp1_msg, m)?)?;
    m.add_function(wrap_pyfunction!(decode::py_decode_mbp10_msg, m)?)?;

    Ok(())
}
