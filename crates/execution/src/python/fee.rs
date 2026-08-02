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

use nautilus_core::python::{to_pynotimplemented_err, to_pyruntime_err, to_pytype_err};
use nautilus_model::{
    instruments::InstrumentAny,
    orders::OrderAny,
    python::{instruments::instrument_any_to_pyobject, orders::order_any_to_pyobject},
    types::{Money, Price, Quantity},
};
use pyo3::{IntoPyObject, IntoPyObjectExt, prelude::*};
use rust_decimal::Decimal;

use crate::models::fee::{
    CappedOptionFeeModel, FeeModel, FeeModelAny, FeeModelHandle, FixedFeeModel, MakerTakerFeeModel,
    PerContractFeeModel, ProbabilityPriceFeeModel, TieredNotionalOptionFeeModel,
};

#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")]
#[pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.execution",
    name = "FeeModel",
    subclass,
    unsendable
)]
#[derive(Debug)]
pub struct PyFeeModel;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFeeModel {
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn get_commission(
        &self,
        _order: &Bound<'_, PyAny>,
        _fill_quantity: Quantity,
        _fill_px: Price,
        _instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        Err(to_pynotimplemented_err(
            "Method 'get_commission' must be implemented in a subclass.",
        ))
    }

    fn get_commission_with_context(
        slf: PyRef<'_, Self>,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
        _underlying_px: Option<Price>,
    ) -> PyResult<Money> {
        let py = slf.py();
        let obj = match slf.into_pyobject(py) {
            Ok(obj) => obj,
            Err(e) => match e {},
        };
        obj.as_any()
            .call_method1(
                "get_commission",
                (order.clone(), fill_quantity, fill_px, instrument.clone()),
            )?
            .extract()
            .map_err(to_pyruntime_err)
    }
}

#[derive(Debug)]
pub struct PythonFeeModel {
    obj: Py<PyAny>,
}

impl PythonFeeModel {
    pub fn new(obj: Py<PyAny>) -> Self {
        Self { obj }
    }
}

impl FeeModel for PythonFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        Python::attach(|py| -> anyhow::Result<Money> {
            let order = order_any_to_pyobject(py, order.clone())?;
            let instrument = instrument_any_to_pyobject(py, instrument.clone())?;
            self.obj
                .bind(py)
                .call_method1(
                    "get_commission",
                    (order, fill_quantity, fill_px, instrument),
                )?
                .extract()
                .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .map_err(|e| anyhow::anyhow!("Python FeeModel.get_commission failed: {e}"))
    }

    fn get_commission_with_context(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
        underlying_px: Option<Price>,
    ) -> anyhow::Result<Money> {
        Python::attach(|py| -> anyhow::Result<Money> {
            let obj = self.obj.bind(py);
            if !has_method_override_before_base(py, obj, "get_commission_with_context")? {
                let order = order_any_to_pyobject(py, order.clone())?;
                let instrument = instrument_any_to_pyobject(py, instrument.clone())?;
                return obj
                    .call_method1(
                        "get_commission",
                        (order, fill_quantity, fill_px, instrument),
                    )?
                    .extract()
                    .map_err(|e| anyhow::anyhow!("{e}"));
            }

            let order = order_any_to_pyobject(py, order.clone())?;
            let instrument = instrument_any_to_pyobject(py, instrument.clone())?;
            obj.call_method1(
                "get_commission_with_context",
                (order, fill_quantity, fill_px, instrument, underlying_px),
            )?
            .extract()
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .map_err(|e| anyhow::anyhow!("Python FeeModel.get_commission_with_context failed: {e}"))
    }
}

fn has_method_override_before_base(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    method_name: &str,
) -> PyResult<bool> {
    let base_type = py.get_type::<PyFeeModel>();
    for cls in obj.get_type().getattr("__mro__")?.try_iter()? {
        let cls = cls?;
        if cls.is(base_type.as_any()) {
            return Ok(false);
        }

        if cls.getattr("__dict__")?.contains(method_name)? {
            return Ok(true);
        }
    }

    Ok(false)
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl FixedFeeModel {
    /// Creates a new `FixedFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
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
    /// Fee model for probability-priced outcome shares.
    ///
    /// Applies `qty * fee_rate * p * (1 - p)` using the instrument's maker or
    /// taker fee rate. This matches venues that represent outcome shares as
    /// `InstrumentAny.BinaryOption` instruments quoted on a `[0, 1]`
    /// probability scale.
    ///
    /// This model covers quote-currency match-time exchange fees only.
    /// Venue-specific rebate programs or non-quote fee assets remain outside the
    /// core execution layer.
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
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
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

/// Extracts a Python fee model object into a runtime [`FeeModelHandle`].
///
/// # Errors
///
/// Returns an error if `obj` is neither a supported built-in model nor a Python object with
/// a `get_commission` method.
pub fn pyobject_to_fee_model_handle(obj: &Bound<'_, PyAny>) -> PyResult<FeeModelHandle> {
    if let Ok(model) = pyobject_to_fee_model_any(obj) {
        return Ok(model.into());
    }

    if !obj.hasattr("get_commission")? {
        let type_name = obj.get_type().name()?;
        return Err(to_pytype_err(format!(
            "Cannot convert {type_name} to FeeModel"
        )));
    }

    Ok(FeeModelHandle::new(PythonFeeModel::new(
        obj.clone().unbind(),
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{OrderAny, builder::OrderTestBuilder},
    };
    use pyo3::{IntoPyObjectExt, ffi::c_str, types::PyDict};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_python_fee_model_handle_calls_python_method() {
        Python::initialize();

        Python::attach(|py| {
            let expected_commission = Money::from("1.23 USD");
            let locals = PyDict::new(py);
            locals
                .set_item("FeeModel", py.get_type::<PyFeeModel>())
                .unwrap();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFeeModel', (FeeModel,), {\
                            'get_commission': \
                                lambda self, order, fill_quantity, fill_px, instrument: self.commission\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            model
                .setattr("commission", expected_commission.into_py_any(py).unwrap())
                .unwrap();

            let handle = pyobject_to_fee_model_handle(&model).unwrap();
            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from(100_000))
                .build();
            let commission = handle
                .get_commission(
                    &order,
                    Quantity::from(100_000),
                    Price::from("0.80000"),
                    &instrument,
                )
                .unwrap();

            assert_eq!(commission, expected_commission);
        });
    }

    #[rstest]
    fn test_python_fee_model_context_falls_back_to_get_commission() {
        Python::initialize();

        Python::attach(|py| {
            let expected_commission = Money::from("1.23 USD");
            let locals = PyDict::new(py);
            locals
                .set_item("FeeModel", py.get_type::<PyFeeModel>())
                .unwrap();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFeeModel', (FeeModel,), {\
                            'get_commission': \
                                lambda self, order, fill_quantity, fill_px, instrument: self.commission\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            model
                .setattr("commission", expected_commission.into_py_any(py).unwrap())
                .unwrap();

            let handle = pyobject_to_fee_model_handle(&model).unwrap();
            let (instrument, order) = commission_inputs();
            let commission = handle
                .get_commission_with_context(
                    &order,
                    Quantity::from(100_000),
                    Price::from("0.80000"),
                    &instrument,
                    Some(Price::from("0.70000")),
                )
                .unwrap();

            assert_eq!(commission, expected_commission);
        });
    }

    #[rstest]
    fn test_python_fee_model_context_calls_python_override() {
        Python::initialize();

        Python::attach(|py| {
            let expected_commission = Money::from("2.34 USD");
            let locals = PyDict::new(py);
            locals
                .set_item("FeeModel", py.get_type::<PyFeeModel>())
                .unwrap();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFeeModel', (FeeModel,), {\
                            'get_commission': \
                                lambda self, order, fill_quantity, fill_px, instrument: self.base_commission, \
                            'get_commission_with_context': \
                                lambda self, order, fill_quantity, fill_px, instrument, underlying_px=None: self.context_commission\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            model
                .setattr(
                    "base_commission",
                    Money::from("1.23 USD").into_py_any(py).unwrap(),
                )
                .unwrap();
            model
                .setattr(
                    "context_commission",
                    expected_commission.into_py_any(py).unwrap(),
                )
                .unwrap();

            let handle = pyobject_to_fee_model_handle(&model).unwrap();
            let (instrument, order) = commission_inputs();
            let commission = handle
                .get_commission_with_context(
                    &order,
                    Quantity::from(100_000),
                    Price::from("0.80000"),
                    &instrument,
                    Some(Price::from("0.70000")),
                )
                .unwrap();

            assert_eq!(commission, expected_commission);
        });
    }

    #[rstest]
    fn test_python_fee_model_context_propagates_python_error() {
        Python::initialize();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("FeeModel", py.get_type::<PyFeeModel>())
                .unwrap();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFeeModel', (FeeModel,), {\
                            'get_commission': lambda self, order, fill_quantity, fill_px, instrument: \
                                (_ for _ in ()).throw(RuntimeError('boom'))\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();

            let handle = pyobject_to_fee_model_handle(&model).unwrap();
            let (instrument, order) = commission_inputs();
            let error = handle
                .get_commission_with_context(
                    &order,
                    Quantity::from(100_000),
                    Price::from("0.80000"),
                    &instrument,
                    None,
                )
                .unwrap_err();
            let error = error.to_string();

            assert!(error.contains("Python FeeModel.get_commission_with_context failed"));
            assert!(error.contains("boom"));
        });
    }

    fn commission_inputs() -> (InstrumentAny, OrderAny) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        (instrument, order)
    }
}
