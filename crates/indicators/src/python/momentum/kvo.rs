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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{
    average::MovingAverageType, indicator::Indicator, momentum::kvo::KlingerVolumeOscillator,
    python::float_precision,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KlingerVolumeOscillator {
    /// Stephen J. Klinger's Volume Oscillator: a fast/slow moving-average
    /// difference over the per-bar "volume force".
    ///
    /// ```text
    /// dm_t   = high_t + low_t + close_t                 (the daily measurement)
    /// trend  = sign(dm_t - dm_{t-1}), carried over when equal
    /// cm_t   = cm_{t-1} + dm_t        while the trend holds
    /// cm_t   = dm_{t-1} + dm_t        when the trend flips
    /// vf_t   = volume_t * |2 * (dm_t / cm_t - 1)| * trend * 100
    /// KVO_t  = MA(vf, fast) - MA(vf, slow)
    /// ```
    ///
    /// Klinger's textbook configuration is `fast = 34, slow = 55` with exponential
    /// averages, which is the default `ma_type`.
    #[new]
    #[pyo3(signature = (fast_period, slow_period, ma_type=None))]
    pub fn py_new(
        fast_period: usize,
        slow_period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> PyResult<Self> {
        Self::new_checked(fast_period, slow_period, ma_type).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "KlingerVolumeOscillator({},{},{})",
            self.fast_period, self.slow_period, self.ma_type
        )
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "fast_period")]
    const fn py_fast_period(&self) -> usize {
        self.fast_period
    }

    #[getter]
    #[pyo3(name = "slow_period")]
    const fn py_slow_period(&self) -> usize {
        self.slow_period
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, close: f64, volume: f64) {
        self.update_raw(high, low, close, volume);
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) -> PyResult<()> {
        float_precision::check_bar(bar)?;
        float_precision::check_bar_volume(bar)?;
        self.handle_bar(bar);
        Ok(())
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
