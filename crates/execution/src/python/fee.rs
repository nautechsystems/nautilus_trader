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

use nautilus_core::python::{
    clone_py_object, to_pynotimplemented_err, to_pyruntime_err, to_pytype_err,
};
use nautilus_model::{
    instruments::InstrumentAny,
    orders::OrderAny,
    python::{
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{order_any_to_pyobject, pyobject_to_order_any},
    },
    types::{Money, Price, Quantity},
};
use pyo3::{
    IntoPyObject, PyClass,
    prelude::*,
    types::{PyDict, PyTuple},
};
use rust_decimal::Decimal;

use crate::models::fee::{
    CappedOptionFeeModel, FeeModel, FeeModelAny, FeeModelHandle, FixedFeeModel, MakerTakerFeeModel,
    PerContractFeeModel, ProbabilityPriceFeeModel, TieredNotionalOptionFeeModel,
};

#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")]
#[pyclass(
    module = "nautilus_trader.execution",
    name = "FeeModel",
    subclass,
    unsendable
)]
#[derive(Debug)]
pub struct PyFeeModel;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyFeeModel {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
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

    #[pyo3(signature = (order, fill_quantity, fill_px, instrument, _underlying_px = None))]
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

fn fee_args_to_any(
    py: Python<'_>,
    order: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
) -> PyResult<(OrderAny, InstrumentAny)> {
    let instrument_any =
        pyobject_to_instrument_any(py, instrument.clone().unbind()).map_err(|_| {
            let type_name = instrument
                .get_type()
                .name()
                .map_or_else(|_| "unknown".to_string(), |name| name.to_string());
            to_pytype_err(format!(
                "`instrument` must be an `Instrument`, was `{type_name}`"
            ))
        })?;
    let order_any = pyobject_to_order_any(py, order.clone().unbind()).map_err(|_| {
        let type_name = order
            .get_type()
            .name()
            .map_or_else(|_| "unknown".to_string(), |name| name.to_string());
        to_pytype_err(format!("`order` must be an `Order`, was `{type_name}`"))
    })?;
    Ok((order_any, instrument_any))
}

fn call_fee_get_commission<M: FeeModel>(
    model: &M,
    py: Python<'_>,
    order: &Bound<'_, PyAny>,
    fill_quantity: Quantity,
    fill_px: Price,
    instrument: &Bound<'_, PyAny>,
) -> PyResult<Money> {
    let (order_any, instrument_any) = fee_args_to_any(py, order, instrument)?;
    model
        .get_commission(&order_any, fill_quantity, fill_px, &instrument_any)
        .map_err(to_pyruntime_err)
}

fn call_fee_get_commission_with_context<M: FeeModel>(
    model: &M,
    py: Python<'_>,
    order: &Bound<'_, PyAny>,
    fill_quantity: Quantity,
    fill_px: Price,
    instrument: &Bound<'_, PyAny>,
    underlying_px: Option<Price>,
) -> PyResult<Money> {
    let (order_any, instrument_any) = fee_args_to_any(py, order, instrument)?;
    model
        .get_commission_with_context(
            &order_any,
            fill_quantity,
            fill_px,
            &instrument_any,
            underlying_px,
        )
        .map_err(to_pyruntime_err)
}

#[derive(Debug)]
pub struct PythonFeeModel {
    obj: Py<PyAny>,
}

impl Clone for PythonFeeModel {
    fn clone(&self) -> Self {
        Self::new(clone_py_object(&self.obj))
    }
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

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl FixedFeeModel {
    /// Creates a new `FixedFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (commission, charge_commission_once=None, change_commission_once=None))]
    fn py_new(
        commission: Money,
        charge_commission_once: Option<bool>,
        change_commission_once: Option<bool>,
    ) -> PyResult<PyClassInitializer<FixedFeeModel>> {
        let charge_commission_once = resolve_fixed_fee_charge_commission_once(
            charge_commission_once,
            change_commission_once,
        )?;
        let model = Self::new(commission, charge_commission_once).map_err(to_pyruntime_err)?;
        Ok(PyClassInitializer::from(PyFeeModel).add_subclass(model))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
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

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl MakerTakerFeeModel {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new() -> PyClassInitializer<MakerTakerFeeModel> {
        PyClassInitializer::from(PyFeeModel).add_subclass(MakerTakerFeeModel)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl PerContractFeeModel {
    /// Creates a new `PerContractFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new(commission: Money) -> PyResult<PyClassInitializer<PerContractFeeModel>> {
        let model = Self::new(commission).map_err(to_pyruntime_err)?;
        Ok(PyClassInitializer::from(PyFeeModel).add_subclass(model))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
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
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new() -> PyClassInitializer<ProbabilityPriceFeeModel> {
        PyClassInitializer::from(PyFeeModel).add_subclass(ProbabilityPriceFeeModel)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl CappedOptionFeeModel {
    /// Creates a new `CappedOptionFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (maker_rate=None, taker_rate=None, cap_rate=None))]
    fn py_new(
        maker_rate: Option<Decimal>,
        taker_rate: Option<Decimal>,
        cap_rate: Option<Decimal>,
    ) -> PyResult<PyClassInitializer<CappedOptionFeeModel>> {
        let model = Self::new(maker_rate, taker_rate, cap_rate).map_err(to_pyruntime_err)?;
        Ok(PyClassInitializer::from(PyFeeModel).add_subclass(model))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
    }

    #[pyo3(signature = (order, fill_quantity, fill_px, instrument, underlying_px = None))]
    fn get_commission_with_context(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
        underlying_px: Option<Price>,
    ) -> PyResult<Money> {
        call_fee_get_commission_with_context(
            self,
            order.py(),
            order,
            fill_quantity,
            fill_px,
            instrument,
            underlying_px,
        )
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[expect(
    clippy::use_self,
    reason = "`Self` breaks pyo3-stub-gen derive for subclass pyclasses"
)]
impl TieredNotionalOptionFeeModel {
    /// Creates a new `TieredNotionalOptionFeeModel` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (maker_rate=None, taker_rate=None))]
    fn py_new(
        maker_rate: Option<Decimal>,
        taker_rate: Option<Decimal>,
    ) -> PyResult<PyClassInitializer<TieredNotionalOptionFeeModel>> {
        let model = Self::new(maker_rate, taker_rate).map_err(to_pyruntime_err)?;
        Ok(PyClassInitializer::from(PyFeeModel).add_subclass(model))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn get_commission(
        &self,
        order: &Bound<'_, PyAny>,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &Bound<'_, PyAny>,
    ) -> PyResult<Money> {
        call_fee_get_commission(self, order.py(), order, fill_quantity, fill_px, instrument)
    }
}

/// Extracts a Python fee model object into a Rust [`FeeModelAny`].
///
/// # Errors
///
/// Returns an error if `obj` is neither a supported built-in model nor a Python object with
/// a `get_commission` method.
pub fn pyobject_to_fee_model_any(obj: &Bound<'_, PyAny>) -> PyResult<FeeModelAny> {
    if let Ok(m) = obj.extract::<PyRef<'_, FixedFeeModel>>() {
        return Ok(FeeModelAny::Fixed((*m).clone()));
    }

    if let Ok(m) = obj.extract::<PyRef<'_, MakerTakerFeeModel>>() {
        return Ok(FeeModelAny::MakerTaker((*m).clone()));
    }

    if let Ok(m) = obj.extract::<PyRef<'_, PerContractFeeModel>>() {
        return Ok(FeeModelAny::PerContract((*m).clone()));
    }

    if let Ok(m) = obj.extract::<PyRef<'_, ProbabilityPriceFeeModel>>() {
        return Ok(FeeModelAny::ProbabilityPrice((*m).clone()));
    }

    if let Ok(m) = obj.extract::<PyRef<'_, CappedOptionFeeModel>>() {
        return Ok(FeeModelAny::CappedOption((*m).clone()));
    }

    if let Ok(m) = obj.extract::<PyRef<'_, TieredNotionalOptionFeeModel>>() {
        return Ok(FeeModelAny::TieredNotionalOption((*m).clone()));
    }

    if !obj.hasattr("get_commission")? {
        let type_name = obj.get_type().name()?;
        return Err(to_pytype_err(format!(
            "Cannot convert {type_name} to FeeModel"
        )));
    }

    Ok(FeeModelAny::Python(PythonFeeModel::new(
        obj.clone().unbind(),
    )))
}

/// Extracts a Python fee model object into a runtime [`FeeModelHandle`].
///
/// # Errors
///
/// Returns an error if `obj` is neither a supported built-in model nor a Python object with
/// a `get_commission` method.
pub fn pyobject_to_fee_model_handle(obj: &Bound<'_, PyAny>) -> PyResult<FeeModelHandle> {
    pyobject_to_fee_model_any(obj).map(Into::into)
}

fn fee_model_into_py<T>(py: Python<'_>, model: T) -> PyResult<Py<PyAny>>
where
    T: PyClass<BaseType = PyFeeModel>,
{
    Ok(Py::new(py, PyClassInitializer::from(PyFeeModel).add_subclass(model))?.into_any())
}

/// Converts a Rust [`FeeModelAny`] into its Python binding object.
///
/// # Errors
///
/// Returns an error if conversion to a Python object fails.
pub fn fee_model_any_to_pyobject(py: Python<'_>, model: &FeeModelAny) -> PyResult<Py<PyAny>> {
    match model {
        FeeModelAny::Fixed(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::MakerTaker(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::PerContract(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::ProbabilityPrice(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::CappedOption(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::TieredNotionalOption(model) => fee_model_into_py(py, model.clone()),
        FeeModelAny::Python(model) => Ok(model.obj.clone_ref(py)),
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
            let model = fee_model_with_commission(py, expected_commission);

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
    fn test_python_fee_model_any_clones_and_retains_python_model() {
        Python::initialize();

        Python::attach(|py| {
            let expected_commission = Money::from("1.23 USD");
            let model = fee_model_with_commission(py, expected_commission);
            let fee_model = pyobject_to_fee_model_any(&model).unwrap();
            let cloned_fee_model = fee_model.clone();
            let original = fee_model_any_to_pyobject(py, &fee_model).unwrap();
            let retained = fee_model_any_to_pyobject(py, &cloned_fee_model).unwrap();
            let (instrument, order) = commission_inputs();
            let commission = cloned_fee_model
                .get_commission(
                    &order,
                    Quantity::from(100_000),
                    Price::from("0.80000"),
                    &instrument,
                )
                .unwrap();

            assert!(original.bind(py).is(&model));
            assert!(retained.bind(py).is(&model));
            assert_eq!(commission, expected_commission);
        });
    }

    #[rstest]
    fn test_python_fee_model_any_rejects_object_without_get_commission() {
        Python::initialize();

        Python::attach(|py| {
            let model = PyDict::new(py);
            let error = pyobject_to_fee_model_any(model.as_any()).unwrap_err();

            assert_eq!(
                error.to_string(),
                "TypeError: Cannot convert dict to FeeModel"
            );
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

    fn fee_model_with_commission(py: Python<'_>, commission: Money) -> Bound<'_, PyAny> {
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
            .setattr("commission", commission.into_py_any(py).unwrap())
            .unwrap();

        model
    }
}
