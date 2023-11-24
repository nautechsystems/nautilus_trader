// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyList},
    PyResult, Python,
};
use serde_json::Value;
use strum::IntoEnumIterator;

use crate::enums;

pub mod data;
pub mod events;
pub mod identifiers;
pub mod instruments;
pub mod macros;
pub mod orders;
pub mod types;

pub const PY_MODULE_MODEL: &str = "nautilus_trader.core.nautilus_pyo3.model";

/// Python iterator over the variants of an enum.
#[pyclass]
pub struct EnumIterator {
    // Type erasure for code reuse. Generic types can't be exposed to Python.
    iter: Box<dyn Iterator<Item = PyObject> + Send>,
}

#[pymethods]
impl EnumIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.iter.next()
    }
}

impl EnumIterator {
    pub fn new<E>(py: Python<'_>) -> Self
    where
        E: strum::IntoEnumIterator + IntoPy<Py<PyAny>>,
        <E as IntoEnumIterator>::Iterator: Send,
    {
        Self {
            iter: Box::new(
                E::iter()
                    .map(|var| var.into_py(py))
                    // Force eager evaluation because `py` isn't `Send`
                    .collect::<Vec<_>>()
                    .into_iter(),
            ),
        }
    }
}

pub fn value_to_pydict(py: Python<'_>, val: &Value) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);

    match val {
        Value::Object(map) => {
            for (key, value) in map.iter() {
                let py_value = value_to_pyobject(py, value)?;
                dict.set_item(key, py_value)?;
            }
        }
        // This shouldn't be reached in this function, but we include it for completeness
        _ => return Err(PyValueError::new_err("Expected JSON object")),
    }

    Ok(dict.into_py(py))
}

pub fn value_to_pyobject(py: Python<'_>, val: &Value) -> PyResult<PyObject> {
    match val {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok(b.into_py(py)),
        Value::String(s) => Ok(s.into_py(py)),
        Value::Number(n) => {
            if n.is_i64() {
                Ok(n.as_i64().unwrap().into_py(py))
            } else if n.is_f64() {
                Ok(n.as_f64().unwrap().into_py(py))
            } else {
                Err(PyValueError::new_err("Unsupported JSON number type"))
            }
        }
        Value::Array(arr) => {
            let py_list = PyList::new(py, &[] as &[PyObject]);
            for item in arr.iter() {
                let py_item = value_to_pyobject(py, item)?;
                py_list.append(py_item)?;
            }
            Ok(py_list.into())
        }
        Value::Object(_) => {
            let py_dict = value_to_pydict(py, val)?;
            Ok(py_dict.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        prelude::*,
        types::{PyBool, PyInt, PyList, PyString},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    #[rstest]
    fn test_value_to_pydict() {
        Python::with_gil(|py| {
            let json_str = r#"
        {
            "type": "OrderAccepted",
            "ts_event": 42,
            "is_reconciliation": false
        }
        "#;

            let val: Value = serde_json::from_str(json_str).unwrap();
            let py_dict_ref = value_to_pydict(py, &val).unwrap();
            let py_dict = py_dict_ref.as_ref(py);

            assert_eq!(
                py_dict
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .downcast::<PyString>()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "OrderAccepted"
            );
            assert_eq!(
                py_dict
                    .get_item("ts_event")
                    .unwrap()
                    .unwrap()
                    .downcast::<PyInt>()
                    .unwrap()
                    .extract::<i64>()
                    .unwrap(),
                42
            );
            assert_eq!(
                py_dict
                    .get_item("is_reconciliation")
                    .unwrap()
                    .unwrap()
                    .downcast::<PyBool>()
                    .unwrap()
                    .is_true(),
                false
            );
        });
    }

    #[rstest]
    fn test_value_to_pyobject_string() {
        Python::with_gil(|py| {
            let val = Value::String("Hello, world!".to_string());
            let py_obj = value_to_pyobject(py, &val).unwrap();

            assert_eq!(py_obj.extract::<&str>(py).unwrap(), "Hello, world!");
        });
    }

    #[rstest]
    fn test_value_to_pyobject_bool() {
        Python::with_gil(|py| {
            let val = Value::Bool(true);
            let py_obj = value_to_pyobject(py, &val).unwrap();

            assert_eq!(py_obj.extract::<bool>(py).unwrap(), true);
        });
    }

    #[rstest]
    fn test_value_to_pyobject_array() {
        Python::with_gil(|py| {
            let val = Value::Array(vec![
                Value::String("item1".to_string()),
                Value::String("item2".to_string()),
            ]);
            let binding = value_to_pyobject(py, &val).unwrap();
            let py_list = binding.downcast::<PyList>(py).unwrap();

            assert_eq!(py_list.len(), 2);
            assert_eq!(
                py_list.get_item(0).unwrap().extract::<&str>().unwrap(),
                "item1"
            );
            assert_eq!(
                py_list.get_item(1).unwrap().extract::<&str>().unwrap(),
                "item2"
            );
        });
    }
}

/// Loaded as nautilus_pyo3.model
#[pymodule]
pub fn model(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    // data
    m.add_class::<crate::data::bar::BarSpecification>()?;
    m.add_class::<crate::data::bar::BarType>()?;
    m.add_class::<crate::data::bar::Bar>()?;
    m.add_class::<crate::data::order::BookOrder>()?;
    m.add_class::<crate::data::delta::OrderBookDelta>()?;
    m.add_class::<crate::data::quote::QuoteTick>()?;
    m.add_class::<crate::data::trade::TradeTick>()?;
    // enums
    m.add_class::<enums::AccountType>()?;
    m.add_class::<enums::AggregationSource>()?;
    m.add_class::<enums::AggressorSide>()?;
    m.add_class::<enums::AssetClass>()?;
    m.add_class::<enums::AssetType>()?;
    m.add_class::<enums::BarAggregation>()?;
    m.add_class::<enums::BookAction>()?;
    m.add_class::<enums::BookType>()?;
    m.add_class::<enums::ContingencyType>()?;
    m.add_class::<enums::CurrencyType>()?;
    m.add_class::<enums::InstrumentCloseType>()?;
    m.add_class::<enums::LiquiditySide>()?;
    m.add_class::<enums::MarketStatus>()?;
    m.add_class::<enums::OmsType>()?;
    m.add_class::<enums::OptionKind>()?;
    m.add_class::<enums::OrderSide>()?;
    m.add_class::<enums::OrderStatus>()?;
    m.add_class::<enums::OrderType>()?;
    m.add_class::<enums::PositionSide>()?;
    m.add_class::<enums::PriceType>()?;
    m.add_class::<enums::TimeInForce>()?;
    m.add_class::<enums::TradingState>()?;
    m.add_class::<enums::TrailingOffsetType>()?;
    m.add_class::<enums::TriggerType>()?;
    // identifiers
    m.add_class::<crate::identifiers::account_id::AccountId>()?;
    m.add_class::<crate::identifiers::client_id::ClientId>()?;
    m.add_class::<crate::identifiers::client_order_id::ClientOrderId>()?;
    m.add_class::<crate::identifiers::component_id::ComponentId>()?;
    m.add_class::<crate::identifiers::exec_algorithm_id::ExecAlgorithmId>()?;
    m.add_class::<crate::identifiers::instrument_id::InstrumentId>()?;
    m.add_class::<crate::identifiers::order_list_id::OrderListId>()?;
    m.add_class::<crate::identifiers::position_id::PositionId>()?;
    m.add_class::<crate::identifiers::strategy_id::StrategyId>()?;
    m.add_class::<crate::identifiers::symbol::Symbol>()?;
    m.add_class::<crate::identifiers::trade_id::TradeId>()?;
    m.add_class::<crate::identifiers::trader_id::TraderId>()?;
    m.add_class::<crate::identifiers::venue::Venue>()?;
    m.add_class::<crate::identifiers::venue_order_id::VenueOrderId>()?;
    // orders
    m.add_class::<crate::orders::limit::LimitOrder>()?;
    m.add_class::<crate::orders::limit_if_touched::LimitIfTouchedOrder>()?;
    m.add_class::<crate::orders::market::MarketOrder>()?;
    m.add_class::<crate::orders::market_to_limit::MarketToLimitOrder>()?;
    m.add_class::<crate::orders::stop_limit::StopLimitOrder>()?;
    m.add_class::<crate::orders::stop_market::StopMarketOrder>()?;
    m.add_class::<crate::orders::trailing_stop_limit::TrailingStopLimitOrder>()?;
    m.add_class::<crate::orders::trailing_stop_market::TrailingStopMarketOrder>()?;
    m.add_class::<crate::types::currency::Currency>()?;
    m.add_class::<crate::types::money::Money>()?;
    m.add_class::<crate::types::price::Price>()?;
    m.add_class::<crate::types::quantity::Quantity>()?;
    // instruments
    m.add_class::<crate::instruments::crypto_future::CryptoFuture>()?;
    m.add_class::<crate::instruments::crypto_perpetual::CryptoPerpetual>()?;
    m.add_class::<crate::instruments::currency_pair::CurrencyPair>()?;
    m.add_class::<crate::instruments::equity::Equity>()?;
    m.add_class::<crate::instruments::futures_contract::FuturesContract>()?;
    m.add_class::<crate::instruments::options_contract::OptionsContract>()?;
    m.add_class::<crate::instruments::synthetic::SyntheticInstrument>()?;
    // events
    m.add_class::<crate::events::order::OrderDenied>()?;
    Ok(())
}
