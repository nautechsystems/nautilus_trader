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

//! Python bindings from [PyO3](https://pyo3.rs).

use pyo3::pymethods;

use crate::config::PortfolioConfig;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PortfolioConfig {
    /// Configuration for `Portfolio` instances.
    #[new]
    #[pyo3(signature = (use_mark_prices=None, use_mark_xrates=None, bar_updates=None, convert_to_account_base_currency=None, min_account_state_logging_interval_ms=None, debug=None))]
    fn py_new(
        use_mark_prices: Option<bool>,
        use_mark_xrates: Option<bool>,
        bar_updates: Option<bool>,
        convert_to_account_base_currency: Option<bool>,
        min_account_state_logging_interval_ms: Option<u64>,
        debug: Option<bool>,
    ) -> Self {
        let default = Self::default();
        Self {
            use_mark_prices: use_mark_prices.unwrap_or(default.use_mark_prices),
            use_mark_xrates: use_mark_xrates.unwrap_or(default.use_mark_xrates),
            bar_updates: bar_updates.unwrap_or(default.bar_updates),
            convert_to_account_base_currency: convert_to_account_base_currency
                .unwrap_or(default.convert_to_account_base_currency),
            min_account_state_logging_interval_ms,
            debug: debug.unwrap_or(default.debug),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn use_mark_prices(&self) -> bool {
        self.use_mark_prices
    }

    #[getter]
    fn use_mark_xrates(&self) -> bool {
        self.use_mark_xrates
    }

    #[getter]
    fn bar_updates(&self) -> bool {
        self.bar_updates
    }

    #[getter]
    fn convert_to_account_base_currency(&self) -> bool {
        self.convert_to_account_base_currency
    }

    #[getter]
    fn min_account_state_logging_interval_ms(&self) -> Option<u64> {
        self.min_account_state_logging_interval_ms
    }

    #[getter]
    fn debug(&self) -> bool {
        self.debug
    }
}
