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

//! Python bindings for position sizing.

use nautilus_core::{
    correctness::{check_equal, check_positive_i64},
    python::{
        correctness_error_to_pyvalue_err, to_pynotimplemented_err, to_pytype_err, to_pyvalue_err,
    },
};
use nautilus_model::{
    instruments::{Instrument, InstrumentAny},
    python::instruments::pyobject_to_instrument_any,
    types::{Money, Price, Quantity},
};
use pyo3::{
    prelude::*,
    sync::PyOnceLock,
    types::{PyAny, PyType},
};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};
use rust_decimal::Decimal;

use crate::sizing::calculate_fixed_risk_position_size;

/// Base class for position sizers.
#[allow(missing_debug_implementations)]
#[gen_stub_pyclass(module = "nautilus_trader.risk")]
#[pyclass(module = "nautilus_trader.risk", subclass)]
pub struct PositionSizer {
    instrument: Py<PyAny>,
    instrument_any: InstrumentAny,
}

#[gen_stub_pymethods]
#[pymethods]
impl PositionSizer {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new(py: Python<'_>, instrument: Py<PyAny>) -> PyResult<Self> {
        Self::from_instrument(py, instrument)
    }

    /// Returns the instrument used for position sizing.
    #[getter]
    fn instrument<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.instrument.bind(py).clone()
    }

    /// Updates the instrument used for position sizing.
    ///
    /// # Errors
    ///
    /// Returns an error if `instrument` is invalid or its ID differs from the
    /// current instrument ID.
    fn update_instrument(&mut self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let updated = Self::from_instrument(py, instrument)?;
        check_equal(
            &self.instrument_any.id(),
            &updated.instrument_any.id(),
            "instrument.id",
            "instrument.id",
        )
        .map_err(correctness_error_to_pyvalue_err)?;
        *self = updated;
        Ok(())
    }

    /// Calculates the position size quantity for the given risk parameters.
    ///
    /// # Errors
    ///
    /// Always returns `NotImplementedError`; subclasses override this method.
    #[pyo3(signature = (
        entry,
        stop_loss,
        equity,
        risk,
        commission_rate = Decimal::ZERO,
        exchange_rate = Decimal::ONE,
        hard_limit = None,
        unit_batch_size = Decimal::ONE,
        units = 1
    ))]
    #[expect(
        clippy::too_many_arguments,
        reason = "position sizing API takes fixed-risk inputs used by callers"
    )]
    #[allow(unused_variables, clippy::unused_self)]
    fn calculate(
        &self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        #[pyo3(from_py_with = extract_decimal)] risk: Decimal,
        #[pyo3(from_py_with = extract_decimal)] commission_rate: Decimal,
        #[pyo3(from_py_with = extract_decimal)] exchange_rate: Decimal,
        #[pyo3(from_py_with = extract_optional_decimal)] hard_limit: Option<Decimal>,
        #[pyo3(from_py_with = extract_decimal)] unit_batch_size: Decimal,
        units: i64,
    ) -> PyResult<Quantity> {
        Err(to_pynotimplemented_err(
            "PositionSizer subclasses must implement `calculate`",
        ))
    }
}

/// Fixed-risk position sizer.
#[allow(missing_debug_implementations)]
#[gen_stub_pyclass(module = "nautilus_trader.risk")]
#[pyclass(module = "nautilus_trader.risk", extends = PositionSizer)]
pub struct FixedRiskSizer;

#[gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl FixedRiskSizer {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new(
        py: Python<'_>,
        instrument: Py<PyAny>,
    ) -> PyResult<PyClassInitializer<FixedRiskSizer>> {
        Ok(
            PyClassInitializer::from(PositionSizer::from_instrument(py, instrument)?)
                .add_subclass(FixedRiskSizer),
        )
    }

    /// Calculates the position size quantity for the given risk parameters.
    ///
    /// Returns zero when no position is riskable, including for zero exchange
    /// rates and equal entry and stop-loss prices.
    ///
    /// # Parameters
    ///
    /// - `entry`: The entry price.
    /// - `stop_loss`: The stop-loss price.
    /// - `equity`: The account equity.
    /// - `risk`: The positive risk fraction.
    /// - `commission_rate`: The non-negative commission rate.
    /// - `exchange_rate`: The non-negative exchange rate between the instrument
    ///   quote currency and the account currency.
    /// - `hard_limit`: The optional positive limit for the total quantity.
    /// - `unit_batch_size`: The non-negative unit batch size.
    /// - `units`: The positive number of units to divide the position into.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A decimal argument is not a `decimal.Decimal`.
    /// - A parameter violates the constraints above.
    /// - Decimal arithmetic overflows.
    /// - The final size rounds to zero or cannot be represented as a `Quantity`.
    #[pyo3(signature = (
        entry,
        stop_loss,
        equity,
        risk,
        commission_rate = Decimal::ZERO,
        exchange_rate = Decimal::ONE,
        hard_limit = None,
        unit_batch_size = Decimal::ONE,
        units = 1
    ))]
    #[expect(
        clippy::too_many_arguments,
        reason = "position sizing API takes fixed-risk inputs used by callers"
    )]
    fn calculate(
        slf: PyRef<'_, Self>,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        #[pyo3(from_py_with = extract_decimal)] risk: Decimal,
        #[pyo3(from_py_with = extract_decimal)] commission_rate: Decimal,
        #[pyo3(from_py_with = extract_decimal)] exchange_rate: Decimal,
        #[pyo3(from_py_with = extract_optional_decimal)] hard_limit: Option<Decimal>,
        #[pyo3(from_py_with = extract_decimal)] unit_batch_size: Decimal,
        units: i64,
    ) -> PyResult<Quantity> {
        check_positive_i64(units, "units").map_err(correctness_error_to_pyvalue_err)?;
        let units = usize::try_from(units).map_err(to_pyvalue_err)?;

        let base = slf.into_super();
        calculate_fixed_risk_position_size(
            &base.instrument_any,
            entry,
            stop_loss,
            equity,
            risk,
            commission_rate,
            exchange_rate,
            hard_limit,
            unit_batch_size,
            units,
        )
        .map_err(correctness_error_to_pyvalue_err)
    }
}

impl PositionSizer {
    fn from_instrument(py: Python<'_>, instrument: Py<PyAny>) -> PyResult<Self> {
        let instrument_any =
            pyobject_to_instrument_any(py, instrument.clone_ref(py)).map_err(|_| {
                let type_name = instrument
                    .bind(py)
                    .get_type()
                    .name()
                    .map_or_else(|_| "unknown".to_string(), |name| name.to_string());
                to_pytype_err(format!(
                    "`instrument` must be an `Instrument`, was `{type_name}`"
                ))
            })?;
        Ok(Self {
            instrument,
            instrument_any,
        })
    }
}

static DECIMAL_TYPE: PyOnceLock<Py<PyType>> = PyOnceLock::new();

fn extract_decimal(value: &Bound<'_, PyAny>) -> PyResult<Decimal> {
    let decimal_type = DECIMAL_TYPE.import(value.py(), "decimal", "Decimal")?;
    if !value.is_instance(decimal_type)? {
        return Err(to_pytype_err(format!(
            "expected decimal.Decimal, was {}",
            value.get_type().name()?
        )));
    }
    value.extract()
}

fn extract_optional_decimal(value: &Bound<'_, PyAny>) -> PyResult<Option<Decimal>> {
    if value.is_none() {
        Ok(None)
    } else {
        extract_decimal(value).map(Some)
    }
}
