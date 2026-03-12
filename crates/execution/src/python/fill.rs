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

//! Python bindings for fill model types.

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;

use crate::models::fill::{
    BestPriceFillModel, CompetitionAwareFillModel, DefaultFillModel, LimitOrderPartialFillModel,
    MarketHoursFillModel, OneTickSlippageFillModel, ProbabilisticFillModel, SizeAwareFillModel,
    ThreeTierFillModel, TwoTierFillModel, VolumeSensitiveFillModel,
};

macro_rules! impl_fill_model_pymethods {
    ($type:ty) => {
        #[pymethods]
        impl $type {
            #[new]
            #[pyo3(signature = (prob_fill_on_limit=1.0, prob_slippage=0.0, random_seed=None))]
            fn py_new(
                prob_fill_on_limit: f64,
                prob_slippage: f64,
                random_seed: Option<u64>,
            ) -> PyResult<Self> {
                Self::new(prob_fill_on_limit, prob_slippage, random_seed).map_err(to_pyruntime_err)
            }

            fn __repr__(&self) -> String {
                format!("{self:?}")
            }
        }
    };
}

impl_fill_model_pymethods!(DefaultFillModel);
impl_fill_model_pymethods!(BestPriceFillModel);
impl_fill_model_pymethods!(OneTickSlippageFillModel);
impl_fill_model_pymethods!(ProbabilisticFillModel);
impl_fill_model_pymethods!(TwoTierFillModel);
impl_fill_model_pymethods!(ThreeTierFillModel);
impl_fill_model_pymethods!(LimitOrderPartialFillModel);
impl_fill_model_pymethods!(SizeAwareFillModel);
impl_fill_model_pymethods!(VolumeSensitiveFillModel);
impl_fill_model_pymethods!(MarketHoursFillModel);

#[pymethods]
impl CompetitionAwareFillModel {
    #[new]
    #[pyo3(signature = (
        prob_fill_on_limit=1.0,
        prob_slippage=0.0,
        random_seed=None,
        liquidity_factor=0.3,
    ))]
    fn py_new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
        liquidity_factor: f64,
    ) -> PyResult<Self> {
        Self::new(
            prob_fill_on_limit,
            prob_slippage,
            random_seed,
            liquidity_factor,
        )
        .map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
