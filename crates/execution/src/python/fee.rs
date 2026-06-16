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

//! Python bindings for fee model types.

use nautilus_core::python::{to_pyruntime_err, to_pytype_err};
use nautilus_model::types::Money;
use pyo3::{IntoPyObjectExt, prelude::*};
use rust_decimal::Decimal;

use crate::models::fee::{
    CappedOptionFeeModel, FeeModelAny, FixedFeeModel, MakerTakerFeeModel, PerContractFeeModel,
    ProbabilityPriceFeeModel, TieredNotionalOptionFeeModel,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl FixedFeeModel {
    /// Creates a new `FixedFeeModel` instance.
    #[new]
    #[pyo3(signature = (commission, charge_commission_once=None, change_commission_once=None))]
    fn py_new(
        commission: Money,
        charge_commission_once: Option<bool>,
        change_commission_once: Option<bool>,
    ) -> PyResult<Self> {
        let charge_commission_once = resolve_fixed_fee_charge_commission_once(
            charge_commission_once,
            change_commission_once,
        )?;
        Self::new(commission, charge_commission_once).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

fn resolve_fixed_fee_charge_commission_once(
    charge_commission_once: Option<bool>,
    change_commission_once: Option<bool>,
) -> PyResult<Option<bool>> {
    if charge_commission_once.is_some() && change_commission_once.is_some() {
        return Err(to_pytype_err(
            "Provide only one of `charge_commission_once` or `change_commission_once`",
        ));
    }

    Ok(charge_commission_once.or(change_commission_once))
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl MakerTakerFeeModel {
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
impl PerContractFeeModel {
    /// Creates a new `PerContractFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    #[new]
    fn py_new(commission: Money) -> PyResult<Self> {
        Self::new(commission).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ProbabilityPriceFeeModel {
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
impl CappedOptionFeeModel {
    /// Creates a new `CappedOptionFeeModel` instance.
    #[new]
    #[pyo3(signature = (maker_rate=None, taker_rate=None, cap_rate=None))]
    fn py_new(
        maker_rate: Option<Decimal>,
        taker_rate: Option<Decimal>,
        cap_rate: Option<Decimal>,
    ) -> PyResult<Self> {
        Self::new(maker_rate, taker_rate, cap_rate).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl TieredNotionalOptionFeeModel {
    /// Creates a new `TieredNotionalOptionFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
    #[new]
    #[pyo3(signature = (maker_rate=None, taker_rate=None))]
    fn py_new(maker_rate: Option<Decimal>, taker_rate: Option<Decimal>) -> PyResult<Self> {
        Self::new(maker_rate, taker_rate).map_err(to_pyruntime_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Extracts a Python fee model object into a Rust [`FeeModelAny`].
///
/// # Errors
///
/// Returns an error if `obj` is not a supported fee model binding.
pub fn pyobject_to_fee_model_any(obj: &Bound<'_, PyAny>) -> PyResult<FeeModelAny> {
    if let Ok(m) = obj.extract::<FixedFeeModel>() {
        return Ok(FeeModelAny::Fixed(m));
    }

    if let Ok(m) = obj.extract::<MakerTakerFeeModel>() {
        return Ok(FeeModelAny::MakerTaker(m));
    }

    if let Ok(m) = obj.extract::<PerContractFeeModel>() {
        return Ok(FeeModelAny::PerContract(m));
    }

    if let Ok(m) = obj.extract::<ProbabilityPriceFeeModel>() {
        return Ok(FeeModelAny::ProbabilityPrice(m));
    }

    if let Ok(m) = obj.extract::<CappedOptionFeeModel>() {
        return Ok(FeeModelAny::CappedOption(m));
    }

    if let Ok(m) = obj.extract::<TieredNotionalOptionFeeModel>() {
        return Ok(FeeModelAny::TieredNotionalOption(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to FeeModel"
    )))
}

/// Converts a Rust [`FeeModelAny`] into its Python binding object.
///
/// # Errors
///
/// Returns an error if conversion to a Python object fails.
pub fn fee_model_any_to_pyobject(py: Python<'_>, model: &FeeModelAny) -> PyResult<Py<PyAny>> {
    match model {
        FeeModelAny::Fixed(model) => model.clone().into_py_any(py),
        FeeModelAny::MakerTaker(model) => model.clone().into_py_any(py),
        FeeModelAny::PerContract(model) => model.clone().into_py_any(py),
        FeeModelAny::ProbabilityPrice(model) => model.clone().into_py_any(py),
        FeeModelAny::CappedOption(model) => model.clone().into_py_any(py),
        FeeModelAny::TieredNotionalOption(model) => model.clone().into_py_any(py),
    }
}
