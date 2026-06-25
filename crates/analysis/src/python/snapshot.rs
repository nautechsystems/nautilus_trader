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

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::snapshot::PortfolioStatistics;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioStatistics {
    #[getter]
    fn pnls(&self) -> HashMap<String, HashMap<String, f64>> {
        self.pnls
            .iter()
            .map(|(currency, stats)| {
                (
                    currency.clone(),
                    stats.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                )
            })
            .collect()
    }

    #[getter]
    fn returns(&self) -> HashMap<String, f64> {
        self.returns.clone().into_iter().collect()
    }

    #[getter]
    fn general(&self) -> HashMap<String, f64> {
        self.general.clone().into_iter().collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PortfolioStatistics(currencies={}, returns={}, general={})",
            self.pnls.len(),
            self.returns.len(),
            self.general.len(),
        )
    }
}
