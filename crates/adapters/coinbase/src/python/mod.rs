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

//! Python bindings from `pyo3`.

pub mod config;
pub mod enums;

use pyo3::prelude::*;

use crate::{
    common::consts::COINBASE,
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
};

/// Loaded as `nautilus_pyo3.coinbase`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn coinbase(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(COINBASE), COINBASE)?;
    m.add_class::<crate::common::enums::CoinbaseEnvironment>()?;
    m.add_class::<crate::common::enums::CoinbaseMarginType>()?;
    m.add_class::<CoinbaseDataClientConfig>()?;
    m.add_class::<CoinbaseExecClientConfig>()?;

    Ok(())
}
