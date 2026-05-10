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

//! Python bindings for margin model types.

use pyo3::prelude::*;

use crate::accounts::margin_model::{LeveragedMarginModel, StandardMarginModel};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StandardMarginModel {
    /// Uses fixed margin percentages without leverage division.
    ///
    /// Margin is calculated as `notional_value * margin_rate`, ignoring the
    /// account leverage. Appropriate for traditional brokers where margin
    /// requirements are fixed percentages of notional value.
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl LeveragedMarginModel {
    /// Divides notional value by leverage before applying margin rates.
    ///
    /// Margin is calculated as `(notional_value / leverage) * margin_rate`.
    /// This is the default model, appropriate for crypto exchanges and venues
    /// where leverage directly reduces margin requirements.
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
