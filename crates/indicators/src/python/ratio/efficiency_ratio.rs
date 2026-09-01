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
use nautilus_model::{
    data::Bar,
    enums::PriceType,
    types::{Money, Price, Quantity, fixed::MAX_FLOAT_PRECISION},
};
use pyo3::prelude::*;

use crate::{indicator::Indicator, ratio::efficiency_ratio::EfficiencyRatio};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl EfficiencyRatio {
    /// Calculates Kaufman's Efficiency Ratio (ER) across a rolling window.
    ///
    /// The period must be at least `2`.
    ///
    /// For period `n`, the ratio is:
    ///
    /// `ER(t) = |P(t) - P(t - n)| / sum(|P(i) - P(i - 1)|, i = t - n + 1 to t)`
    ///
    /// A full `n`-period window requires `n + 1` prices for `n` price changes. For
    /// finite inputs within the model price range, values range from `0.0` to `1.0`:
    /// lower values indicate more noise, while `1.0` indicates directional price
    /// movement without reversals.
    ///
    /// For compatibility, `initialized` becomes true after `n` inputs, so the first
    /// initialized value covers the `n - 1` available price changes.
    ///
    /// # References
    ///
    /// - Kaufman, P. J. (1995). *Smarter Trading*. McGraw-Hill.
    #[new]
    #[pyo3(signature = (period, price_type=None))]
    fn py_new(period: usize, price_type: Option<PriceType>) -> PyResult<Self> {
        Self::new_checked(period, price_type).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("EfficiencyRatio({})", self.period)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    const fn py_period(&self) -> usize {
        self.period
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

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(
        &mut self,
        #[gen_stub(override_type(type_repr = "float"))] value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let value = extract_update_value(value)?;
        self.update_raw(value);
        Ok(())
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) -> PyResult<()> {
        check_float_precision(bar.close.precision)?;

        self.handle_bar(bar);
        Ok(())
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

fn extract_update_value(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if value.is_instance_of::<Price>() {
        let price = value.extract::<Price>()?;
        check_float_precision(price.precision)?;
        return Ok(price.as_f64());
    }

    if value.is_instance_of::<Quantity>() {
        let quantity = value.extract::<Quantity>()?;
        check_float_precision(quantity.precision)?;
        return Ok(quantity.as_f64());
    }

    if value.is_instance_of::<Money>() {
        let money = value.extract::<Money>()?;
        check_float_precision(money.currency.precision)?;
        return Ok(money.as_f64());
    }

    value.extract()
}

fn check_float_precision(precision: u8) -> PyResult<()> {
    if precision > MAX_FLOAT_PRECISION {
        return Err(to_pyvalue_err(format!(
            "Fixed-point precision {precision} exceeds maximum float precision {MAX_FLOAT_PRECISION}",
        )));
    }

    Ok(())
}
