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

use pyo3::{prelude::*, pymodule};

pub mod cash;
pub mod margin;
pub mod transformer;

#[pymodule]
pub fn accounting(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<crate::account::cash::CashAccount>()?;
    m.add_class::<crate::account::margin::MarginAccount>()?;
    m.add_function(wrap_pyfunction!(
        crate::python::transformer::cash_account_from_account_events,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        crate::python::transformer::margin_account_from_account_events,
        m
    )?)?;
    Ok(())
}
