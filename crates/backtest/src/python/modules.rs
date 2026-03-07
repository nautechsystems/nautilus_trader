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

//! Python bindings for simulation module types.

use pyo3::prelude::*;

use crate::modules::fx_rollover::{FXRolloverInterestModule, InterestRateRecord};

#[pymethods]
impl InterestRateRecord {
    #[new]
    fn py_new(location: String, time: String, value: f64) -> Self {
        Self {
            location,
            time,
            value,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl FXRolloverInterestModule {
    #[new]
    fn py_new(records: Vec<InterestRateRecord>) -> Self {
        Self::new(records)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
