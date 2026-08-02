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

use nautilus_core::python::{to_pyruntime_err, to_pytype_err};
use nautilus_model::{
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::OrderAny,
    python::{instruments::instrument_any_to_pyobject, orders::order_any_to_pyobject},
    types::Price,
};
use pyo3::prelude::*;

use crate::models::fill::{
    BestPriceFillModel, CompetitionAwareFillModel, DefaultFillModel, FillModel, FillModelAny,
    FillModelHandle, LimitOrderPartialFillModel, MarketHoursFillModel, OneTickSlippageFillModel,
    ProbabilisticFillModel, SizeAwareFillModel, ThreeTierFillModel, TwoTierFillModel,
    VolumeSensitiveFillModel,
};

#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")]
#[pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.execution",
    name = "FillModel",
    subclass,
    unsendable
)]
#[derive(Debug)]
pub struct PyFillModel;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFillModel {
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn is_limit_filled(&mut self) -> bool {
        true
    }

    fn is_slipped(&mut self) -> bool {
        false
    }

    fn fill_limit_inside_spread(&self) -> bool {
        false
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        _instrument: &Bound<'_, PyAny>,
        _order: &Bound<'_, PyAny>,
        _best_bid: Price,
        _best_ask: Price,
    ) -> Option<OrderBook> {
        None
    }
}

#[derive(Debug)]
pub struct PythonFillModel {
    obj: Py<PyAny>,
}

impl PythonFillModel {
    pub fn new(obj: Py<PyAny>) -> Self {
        Self { obj }
    }
}

impl FillModel for PythonFillModel {
    fn is_limit_filled(&mut self) -> anyhow::Result<bool> {
        call_bool_method(&self.obj, "is_limit_filled")
    }

    fn is_slipped(&mut self) -> anyhow::Result<bool> {
        call_bool_method(&self.obj, "is_slipped")
    }

    fn fill_limit_inside_spread(&self) -> anyhow::Result<bool> {
        Python::attach(|py| -> anyhow::Result<bool> {
            let obj = self.obj.bind(py);
            if !obj.hasattr("fill_limit_inside_spread")? {
                return Ok(false);
            }

            obj.call_method0("fill_limit_inside_spread")?
                .extract()
                .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .map_err(|e| anyhow::anyhow!("Python FillModel.fill_limit_inside_spread failed: {e}"))
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> anyhow::Result<Option<OrderBook>> {
        Python::attach(|py| -> anyhow::Result<Option<OrderBook>> {
            let obj = self.obj.bind(py);
            if !obj.hasattr("get_orderbook_for_fill_simulation")? {
                return Ok(None);
            }

            let instrument = instrument_any_to_pyobject(py, instrument.clone())?;
            let order = order_any_to_pyobject(py, order.clone())?;
            obj.call_method1(
                "get_orderbook_for_fill_simulation",
                (instrument, order, best_bid, best_ask),
            )?
            .extract()
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .map_err(|e| {
            anyhow::anyhow!("Python FillModel.get_orderbook_for_fill_simulation failed: {e}")
        })
    }
}

fn call_bool_method(obj: &Py<PyAny>, method_name: &str) -> anyhow::Result<bool> {
    Python::attach(|py| -> anyhow::Result<bool> {
        obj.bind(py)
            .call_method0(method_name)?
            .extract()
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
    .map_err(|e| anyhow::anyhow!("Python FillModel.{method_name} failed: {e}"))
}

/// Extracts a Python fill model object into a Rust [`FillModelAny`].
///
/// # Errors
///
/// Returns an error if `obj` is not a supported built-in fill model binding.
pub fn pyobject_to_fill_model_any(obj: &Bound<'_, PyAny>) -> PyResult<FillModelAny> {
    if let Ok(m) = obj.extract::<DefaultFillModel>() {
        return Ok(FillModelAny::Default(m));
    }

    if let Ok(m) = obj.extract::<BestPriceFillModel>() {
        return Ok(FillModelAny::BestPrice(m));
    }

    if let Ok(m) = obj.extract::<OneTickSlippageFillModel>() {
        return Ok(FillModelAny::OneTickSlippage(m));
    }

    if let Ok(m) = obj.extract::<ProbabilisticFillModel>() {
        return Ok(FillModelAny::Probabilistic(m));
    }

    if let Ok(m) = obj.extract::<TwoTierFillModel>() {
        return Ok(FillModelAny::TwoTier(m));
    }

    if let Ok(m) = obj.extract::<ThreeTierFillModel>() {
        return Ok(FillModelAny::ThreeTier(m));
    }

    if let Ok(m) = obj.extract::<LimitOrderPartialFillModel>() {
        return Ok(FillModelAny::LimitOrderPartialFill(m));
    }

    if let Ok(m) = obj.extract::<SizeAwareFillModel>() {
        return Ok(FillModelAny::SizeAware(m));
    }

    if let Ok(m) = obj.extract::<CompetitionAwareFillModel>() {
        return Ok(FillModelAny::CompetitionAware(m));
    }

    if let Ok(m) = obj.extract::<VolumeSensitiveFillModel>() {
        return Ok(FillModelAny::VolumeSensitive(m));
    }

    if let Ok(m) = obj.extract::<MarketHoursFillModel>() {
        return Ok(FillModelAny::MarketHours(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to FillModel"
    )))
}

/// Extracts a Python fill model object into a runtime [`FillModelHandle`].
///
/// # Errors
///
/// Returns an error if `obj` is neither a supported built-in model nor a Python object with
/// `is_limit_filled` and `is_slipped` methods.
pub fn pyobject_to_fill_model_handle(obj: &Bound<'_, PyAny>) -> PyResult<FillModelHandle> {
    if let Ok(model) = pyobject_to_fill_model_any(obj) {
        return Ok(model.into());
    }

    let has_required_methods = obj.hasattr("is_limit_filled")? && obj.hasattr("is_slipped")?;
    if !has_required_methods {
        let type_name = obj.get_type().name()?;
        return Err(to_pytype_err(format!(
            "Cannot convert {type_name} to FillModel"
        )));
    }

    Ok(FillModelHandle::new(PythonFillModel::new(
        obj.clone().unbind(),
    )))
}

macro_rules! impl_fill_model_pymethods {
    ($type:ty) => {
        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
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
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CompetitionAwareFillModel {
    /// Fill model that reduces available liquidity by a factor to simulate market competition.
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::builder::OrderTestBuilder,
        types::Quantity,
    };
    use pyo3::ffi::c_str;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_python_fill_model_handle_calls_python_methods() {
        Python::initialize();

        Python::attach(|py| {
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFillModel', (), {\
                            'is_limit_filled': lambda self: False, \
                            'is_slipped': lambda self: True, \
                            'fill_limit_inside_spread': lambda self: True\
                        })()"
                    ),
                    None,
                    None,
                )
                .unwrap();
            let mut handle = pyobject_to_fill_model_handle(&model).unwrap();

            assert!(!handle.is_limit_filled().unwrap());
            assert!(handle.is_slipped().unwrap());
            assert!(handle.fill_limit_inside_spread().unwrap());
        });
    }

    #[rstest]
    fn test_python_fill_model_handle_calls_python_liquidity_method() {
        Python::initialize();

        Python::attach(|py| {
            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from(100_000))
                .build();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFillModel', (), {\
                            'is_limit_filled': lambda self: True, \
                            'is_slipped': lambda self: False, \
                            'get_orderbook_for_fill_simulation': \
                                lambda self, instrument, order, best_bid, best_ask: None\
                        })()"
                    ),
                    None,
                    None,
                )
                .unwrap();
            let mut handle = pyobject_to_fill_model_handle(&model).unwrap();

            let book = handle
                .get_orderbook_for_fill_simulation(
                    &instrument,
                    &order,
                    Price::from("0.80000"),
                    Price::from("0.80010"),
                )
                .unwrap();

            assert!(book.is_none());
        });
    }

    #[rstest]
    fn test_python_fill_model_handle_uses_defaults_for_missing_optional_methods() {
        Python::initialize();

        Python::attach(|py| {
            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from(100_000))
                .build();
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFillModel', (), {\
                            'is_limit_filled': lambda self: True, \
                            'is_slipped': lambda self: False\
                        })()"
                    ),
                    None,
                    None,
                )
                .unwrap();
            let mut handle = pyobject_to_fill_model_handle(&model).unwrap();

            let book = handle
                .get_orderbook_for_fill_simulation(
                    &instrument,
                    &order,
                    Price::from("0.80000"),
                    Price::from("0.80010"),
                )
                .unwrap();

            assert!(!handle.fill_limit_inside_spread().unwrap());
            assert!(book.is_none());
        });
    }

    #[rstest]
    fn test_python_fill_model_handle_propagates_python_error() {
        Python::initialize();

        Python::attach(|py| {
            let model = py
                .eval(
                    c_str!(
                        "type('CustomFillModel', (), {\
                            'is_limit_filled': lambda self: \
                                (_ for _ in ()).throw(RuntimeError('boom')), \
                            'is_slipped': lambda self: False\
                        })()"
                    ),
                    None,
                    None,
                )
                .unwrap();
            let mut handle = pyobject_to_fill_model_handle(&model).unwrap();
            let error = handle.is_limit_filled().unwrap_err().to_string();

            assert!(error.contains("Python FillModel.is_limit_filled failed"));
            assert!(error.contains("boom"));
        });
    }
}
