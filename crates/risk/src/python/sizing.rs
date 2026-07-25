// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings for position sizing.

use nautilus_core::correctness::{
    check_equal, check_non_negative_decimal, check_positive_decimal, check_positive_usize,
};
use nautilus_core::python::{
    correctness_error_to_pyvalue_err, decimal, py_to_decimal, to_pynotimplemented_err,
    to_pytype_err,
};
use nautilus_model::instruments::Instrument;
use nautilus_model::python::instruments::pyobject_to_instrument_any;
use nautilus_model::types::{Money, Price, Quantity};
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::sizing::calculate_fixed_risk_position_size;

/// Validates that `instrument` is a supported `Instrument`.
///
/// # Errors
///
/// Returns a `TypeError` if the value is not a valid `Instrument`.
fn check_instrument(py: Python<'_>, instrument: &Py<PyAny>) -> PyResult<()> {
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
    Ok(())
}

/// Base class for position sizers.
#[allow(missing_debug_implementations)]
#[gen_stub_pyclass(module = "nautilus_trader.risk")]
#[pyclass(module = "nautilus_trader.risk", subclass)]
pub struct PositionSizer {
    instrument: Py<PyAny>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PositionSizer {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new(py: Python<'_>, instrument: Py<PyAny>) -> PyResult<Self> {
        check_instrument(py, &instrument)?;
        Ok(Self { instrument })
    }

    /// The instrument used for position sizing.
    #[getter]
    fn instrument<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.instrument.bind(py).clone()
    }

    /// Replace the instrument used for position sizing.
    fn update_instrument(&mut self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let current = pyobject_to_instrument_any(py, self.instrument.clone_ref(py))?;
        let new = pyobject_to_instrument_any(py, instrument.clone_ref(py))?;
        check_equal(&current.id(), &new.id(), "instrument.id", "instrument.id")
            .map_err(correctness_error_to_pyvalue_err)?;
        self.instrument = instrument;
        Ok(())
    }

    /// Calculate the position size quantity for the given risk parameters.
    ///
    /// Raises `NotImplementedError`; subclasses override this method.
    #[pyo3(signature = (
        entry,
        stop_loss,
        equity,
        risk,
        commission_rate = decimal.Decimal("0"),
        exchange_rate = decimal.Decimal("1"),
        hard_limit = None,
        unit_batch_size = decimal.Decimal("1"),
        units = 1
    ))]
    #[expect(
        clippy::too_many_arguments,
        reason = "position sizing API takes fixed-risk inputs used by callers"
    )]
    #[expect(clippy::needless_pass_by_value)]
    #[allow(unused_variables, clippy::unused_self)]
    fn calculate(
        &self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))] risk: Py<
            PyAny,
        >,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        commission_rate: Py<PyAny>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        exchange_rate: Py<PyAny>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal | None", imports = ("decimal",)))]
        hard_limit: Option<Py<PyAny>>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        unit_batch_size: Py<PyAny>,
        units: usize,
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
        check_instrument(py, &instrument)?;
        Ok(PyClassInitializer::from(PositionSizer { instrument }).add_subclass(FixedRiskSizer))
    }

    /// Calculate the position size quantity for the given risk parameters.
    #[pyo3(signature = (
        entry,
        stop_loss,
        equity,
        risk,
        commission_rate = decimal.Decimal("0"),
        exchange_rate = decimal.Decimal("1"),
        hard_limit = None,
        unit_batch_size = decimal.Decimal("1"),
        units = 1
    ))]
    #[expect(
        clippy::too_many_arguments,
        reason = "position sizing API takes fixed-risk inputs used by callers"
    )]
    #[expect(clippy::needless_pass_by_value)]
    fn calculate(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))] risk: Py<
            PyAny,
        >,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        commission_rate: Py<PyAny>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        exchange_rate: Py<PyAny>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal | None", imports = ("decimal",)))]
        hard_limit: Option<Py<PyAny>>,
        #[gen_stub(override_type(type_repr = "decimal.Decimal", imports = ("decimal",)))]
        unit_batch_size: Py<PyAny>,
        units: usize,
    ) -> PyResult<Quantity> {
        let risk = py_to_decimal(risk.bind(py))?;
        check_positive_decimal(risk, "risk").map_err(correctness_error_to_pyvalue_err)?;
        let commission_rate = py_to_decimal(commission_rate.bind(py))?;
        check_non_negative_decimal(commission_rate, "commission_rate")
            .map_err(correctness_error_to_pyvalue_err)?;
        let exchange_rate = py_to_decimal(exchange_rate.bind(py))?;
        check_non_negative_decimal(exchange_rate, "exchange_rate")
            .map_err(correctness_error_to_pyvalue_err)?;
        let hard_limit = match hard_limit {
            Some(value) => {
                let parsed = py_to_decimal(value.bind(py))?;
                check_positive_decimal(parsed, "hard_limit")
                    .map_err(correctness_error_to_pyvalue_err)?;
                Some(parsed)
            }
            None => None,
        };
        let unit_batch_size = py_to_decimal(unit_batch_size.bind(py))?;
        check_non_negative_decimal(unit_batch_size, "unit_batch_size")
            .map_err(correctness_error_to_pyvalue_err)?;

        check_positive_usize(units, "units").map_err(correctness_error_to_pyvalue_err)?;

        let base = slf.into_super();
        let instrument = pyobject_to_instrument_any(py, base.instrument.clone_ref(py))?;
        calculate_fixed_risk_position_size(
            &instrument,
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
