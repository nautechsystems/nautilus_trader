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

//! Coinbase enumerations Python bindings.

use pyo3::prelude::*;

use crate::common::enums::CoinbaseEnvironment;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinbaseEnvironment {
    /// Coinbase environment selection (live vs sandbox).
    #[new]
    fn py_new() -> Self {
        Self::default()
    }

    const fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __str__(&self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Sandbox => "SANDBOX",
        }
    }

    fn __repr__(&self) -> String {
        format!("CoinbaseEnvironment.{}", self.__str__())
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.__str__().to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }
}
