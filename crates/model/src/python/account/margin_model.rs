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

//! Python bindings for margin model types.

use std::any::Any;

use nautilus_core::python::{clone_py_object, to_pynotimplemented_err, to_pytype_err};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyTuple},
};
use rust_decimal::Decimal;

use crate::{
    accounts::margin_model::{
        LeveragedMarginModel, MarginModel, MarginModelAny, StandardMarginModel,
    },
    instruments::{
        BettingInstrument, BinaryOption, Cfd, Commodity, CryptoFuture, CryptoFuturesSpread,
        CryptoOption, CryptoOptionSpread, CryptoPerpetual, CurrencyPair, Equity, FuturesContract,
        FuturesSpread, IndexInstrument, Instrument, InstrumentAny, OptionContract, OptionSpread,
        PerpetualContract, TokenizedAsset,
    },
    python::instruments::instrument_any_to_pyobject,
    types::{Money, Price, Quantity},
};

/// Base class for Python-defined margin models.
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
#[pyclass(module = "nautilus_trader.model", name = "MarginModel", subclass)]
#[derive(Debug)]
pub struct PyMarginModel;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyMarginModel {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    /// Returns the initial margin requirement as `Money`.
    #[pyo3(signature = (instrument, quantity, price, leverage, use_quote_for_inverse=None))]
    fn calculate_initial_margin(
        &self,
        instrument: &Bound<'_, PyAny>,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> PyResult<Money> {
        let _ = (instrument, quantity, price, leverage, use_quote_for_inverse);
        Err(to_pynotimplemented_err(
            "Method 'calculate_initial_margin' must be implemented in a subclass.",
        ))
    }

    /// Returns the maintenance margin requirement as `Money`.
    #[pyo3(signature = (instrument, quantity, price, leverage, use_quote_for_inverse=None))]
    fn calculate_maintenance_margin(
        &self,
        instrument: &Bound<'_, PyAny>,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> PyResult<Money> {
        let _ = (instrument, quantity, price, leverage, use_quote_for_inverse);
        Err(to_pynotimplemented_err(
            "Method 'calculate_maintenance_margin' must be implemented in a subclass.",
        ))
    }
}

/// Retains the Python instance used by margin accounts and configuration clones.
#[derive(Debug)]
pub struct PythonMarginModel {
    obj: Py<PyAny>,
}

impl Clone for PythonMarginModel {
    fn clone(&self) -> Self {
        Self {
            obj: clone_py_object(&self.obj),
        }
    }
}

impl MarginModel for PythonMarginModel {
    fn name(&self) -> &'static str {
        "python"
    }

    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.calculate(
            "calculate_initial_margin",
            instrument,
            quantity,
            price,
            leverage,
            use_quote_for_inverse,
        )
    }

    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.calculate(
            "calculate_maintenance_margin",
            instrument,
            quantity,
            price,
            leverage,
            use_quote_for_inverse,
        )
    }
}

impl PythonMarginModel {
    fn calculate(
        &self,
        method: &str,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        Python::attach(|py| -> PyResult<Money> {
            let instrument = instrument_to_pyobject(py, instrument)?;
            self.obj
                .bind(py)
                .call_method1(
                    method,
                    (instrument, quantity, price, leverage, use_quote_for_inverse),
                )?
                .extract::<Money>()
                .map_err(Into::into)
        })
        .map_err(|e| anyhow::anyhow!("Python MarginModel.{method} failed: {e}"))
    }
}

fn instrument_to_pyobject(py: Python<'_>, instrument: &dyn Instrument) -> PyResult<Py<PyAny>> {
    let instrument = instrument as &dyn Any;
    if let Some(instrument) = instrument.downcast_ref::<InstrumentAny>() {
        return instrument_any_to_pyobject(py, instrument.clone());
    }

    macro_rules! convert {
        ($($kind:ty),+ $(,)?) => {
            $(if let Some(instrument) = instrument.downcast_ref::<$kind>() {
                return instrument.clone().into_py_any(py);
            })+
        };
    }

    convert!(
        BettingInstrument,
        BinaryOption,
        Cfd,
        Commodity,
        CryptoFuture,
        CryptoFuturesSpread,
        CryptoOption,
        CryptoOptionSpread,
        CryptoPerpetual,
        CurrencyPair,
        Equity,
        FuturesContract,
        FuturesSpread,
        IndexInstrument,
        OptionContract,
        OptionSpread,
        PerpetualContract,
        TokenizedAsset,
    );
    Err(to_pytype_err(
        "Unsupported instrument type for Python MarginModel",
    ))
}

/// Extracts a built-in or Python margin model, retaining Python instance state.
///
/// # Errors
///
/// Returns an error if either margin calculation method is missing or not callable.
pub fn pyobject_to_margin_model_any(obj: &Bound<'_, PyAny>) -> PyResult<MarginModelAny> {
    if let Ok(model) = obj.extract::<StandardMarginModel>() {
        return Ok(MarginModelAny::Standard(model));
    }

    if let Ok(model) = obj.extract::<LeveragedMarginModel>() {
        return Ok(MarginModelAny::Leveraged(model));
    }

    for method in ["calculate_initial_margin", "calculate_maintenance_margin"] {
        if !obj.hasattr(method)? || !obj.getattr(method)?.is_callable() {
            let type_name = obj.get_type().name()?;
            return Err(to_pytype_err(format!(
                "Cannot convert {type_name} to MarginModel"
            )));
        }
    }

    Ok(MarginModelAny::Python(PythonMarginModel {
        obj: obj.clone().unbind(),
    }))
}

/// Converts a margin model into its Python object without copying Python state.
///
/// # Errors
///
/// Returns an error if a built-in model cannot be converted to Python.
pub fn margin_model_any_to_pyobject(py: Python<'_>, model: &MarginModelAny) -> PyResult<Py<PyAny>> {
    match model {
        MarginModelAny::Standard(model) => (*model).into_py_any(py),
        MarginModelAny::Leveraged(model) => (*model).into_py_any(py),
        MarginModelAny::Python(model) => Ok(model.obj.clone_ref(py)),
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StandardMarginModel {
    /// Uses fixed margin percentages without leverage division.
    ///
    /// Margin is calculated as `notional_value * margin_rate`, ignoring the
    /// account leverage. Appropriate for traditional brokers where margin
    /// requirements are fixed percentages of notional value.
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
impl LeveragedMarginModel {
    /// Divides notional value by leverage before applying margin rates.
    ///
    /// Margin is calculated as `(notional_value / leverage) * margin_rate`.
    /// This is the default model, appropriate for crypto exchanges and venues
    /// where leverage directly reduces margin requirements.
    #[new]
    fn py_new() -> Self {
        Self
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use pyo3::ffi::c_str;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        accounts::{MarginAccount, margin_model::MarginModelHandle},
        events::account::stubs::margin_account_state,
        instruments::stubs::*,
        python::instruments::pyobject_to_instrument_any,
    };

    #[rstest]
    #[case(None)]
    #[case(Some(false))]
    #[case(Some(true))]
    fn test_python_margin_model_account_clones_retain_state_and_exact_inputs(
        #[case] use_quote: Option<bool>,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("MarginModel", py.get_type::<PyMarginModel>())
                .unwrap();
            py.run(
                c_str!(
                    "\
class CustomMargin(MarginModel):
    def __init__(self):
        self.calls = []
    def calculate_initial_margin(self, *args):
        self.calls.append(('initial', args))
        return self.initial
    def calculate_maintenance_margin(self, *args):
        self.calls.append(('maintenance', args))
        return self.maintenance
model = CustomMargin()
"
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            let model = locals.get_item("model").unwrap().unwrap();
            model.setattr("initial", Money::from("123.45 USD")).unwrap();
            model
                .setattr("maintenance", Money::from("67.89 USD"))
                .unwrap();
            let configured = pyobject_to_margin_model_any(&model).unwrap();
            let cloned = configured.clone();
            let retained = margin_model_any_to_pyobject(py, &cloned).unwrap();
            let instrument = audusd_sim();
            let mut account = MarginAccount::new(margin_account_state(), true);
            account.set_default_leverage(dec!(9));
            account.set_leverage(instrument.id(), dec!(3.25));
            account.set_margin_model(configured.into());
            let account = account.clone();
            let quantity = Quantity::from("12345");
            let price = Price::from("0.81234");
            let initial = account
                .calculate_initial_margin(&instrument, quantity, price, use_quote)
                .unwrap();
            let maintenance = account
                .calculate_maintenance_margin(&instrument, quantity, price, use_quote)
                .unwrap();
            let calls = model.getattr("calls").unwrap();

            assert!(retained.bind(py).is(&model));
            assert_eq!(account.margin_model().name(), "python");
            assert_eq!(initial, Money::from("123.45 USD"));
            assert_eq!(maintenance, Money::from("67.89 USD"));
            assert_eq!(calls.len().unwrap(), 2);

            for (index, method) in ["initial", "maintenance"].iter().enumerate() {
                let call = calls.get_item(index).unwrap();
                assert_eq!(
                    call.get_item(0).unwrap().extract::<String>().unwrap(),
                    *method
                );
                let args = call.get_item(1).unwrap();
                assert_eq!(args.len().unwrap(), 5);
                assert_eq!(
                    args.get_item(0).unwrap().extract::<CurrencyPair>().unwrap(),
                    instrument
                );
                assert_eq!(
                    args.get_item(1).unwrap().extract::<Quantity>().unwrap(),
                    quantity
                );
                assert_eq!(args.get_item(2).unwrap().extract::<Price>().unwrap(), price);
                assert_eq!(
                    args.get_item(3).unwrap().extract::<Decimal>().unwrap(),
                    dec!(3.25)
                );
                assert_eq!(
                    args.get_item(4).unwrap().extract::<Option<bool>>().unwrap(),
                    use_quote
                );
            }
        });
    }

    #[rstest]
    #[case(InstrumentAny::Betting(betting()))]
    #[case(InstrumentAny::BinaryOption(binary_option()))]
    #[case(InstrumentAny::Cfd(cfd_gold()))]
    #[case(InstrumentAny::Commodity(commodity_gold()))]
    #[case(InstrumentAny::CryptoFuture(crypto_future_btcusdt(
        2,
        6,
        Price::from("0.01"),
        Quantity::from("0.000001")
    )))]
    #[case(InstrumentAny::CryptoFuturesSpread(crypto_futures_spread_btc_deribit()))]
    #[case(InstrumentAny::CryptoOption(crypto_option_btc_deribit(
        3,
        1,
        Price::from("0.001"),
        Quantity::from("0.1")
    )))]
    #[case(InstrumentAny::CryptoOptionSpread(crypto_option_spread_btc_deribit()))]
    #[case(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))]
    #[case(InstrumentAny::CurrencyPair(audusd_sim()))]
    #[case(InstrumentAny::Equity(equity_aapl()))]
    #[case(InstrumentAny::FuturesContract(futures_contract_es(None, None)))]
    #[case(InstrumentAny::FuturesSpread(futures_spread_es()))]
    #[case(InstrumentAny::IndexInstrument(index_instrument_spx()))]
    #[case(InstrumentAny::OptionContract(option_contract_appl()))]
    #[case(InstrumentAny::OptionSpread(option_spread()))]
    #[case(InstrumentAny::PerpetualContract(perpetual_contract_eurusd()))]
    #[case(InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()))]
    fn test_margin_callback_instrument_conversion_preserves_all_fields(
        #[case] instrument: InstrumentAny,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let concrete: &dyn Instrument = match &instrument {
                InstrumentAny::Betting(i) => i,
                InstrumentAny::BinaryOption(i) => i,
                InstrumentAny::Cfd(i) => i,
                InstrumentAny::Commodity(i) => i,
                InstrumentAny::CryptoFuture(i) => i,
                InstrumentAny::CryptoFuturesSpread(i) => i,
                InstrumentAny::CryptoOption(i) => i,
                InstrumentAny::CryptoOptionSpread(i) => i,
                InstrumentAny::CryptoPerpetual(i) => i,
                InstrumentAny::CurrencyPair(i) => i,
                InstrumentAny::Equity(i) => i,
                InstrumentAny::FuturesContract(i) => i,
                InstrumentAny::FuturesSpread(i) => i,
                InstrumentAny::IndexInstrument(i) => i,
                InstrumentAny::OptionContract(i) => i,
                InstrumentAny::OptionSpread(i) => i,
                InstrumentAny::PerpetualContract(i) => i,
                InstrumentAny::TokenizedAsset(i) => i,
            };

            for borrowed in [&instrument as &dyn Instrument, concrete] {
                let obj = instrument_to_pyobject(py, borrowed).unwrap();
                let converted = pyobject_to_instrument_any(py, obj).unwrap();
                assert_eq!(
                    serde_json::to_value(&converted).unwrap(),
                    serde_json::to_value(&instrument).unwrap()
                );
            }
        });
    }

    #[rstest]
    fn test_python_margin_model_handle_is_send_sync_and_runs_on_another_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MarginModelHandle>();
        assert_send_sync::<MarginModelAny>();
        assert_send_sync::<PythonMarginModel>();
        Python::initialize();

        let handle = Python::attach(|py| {
            let locals = PyDict::new(py);
            locals.set_item("result", Money::from("45.67 USD")).unwrap();
            let model = py.eval(c_str!("type('Margin', (), {'calculate_initial_margin': lambda self, *args: result, 'calculate_maintenance_margin': lambda self, *args: result})()"), Some(&locals), None).unwrap();
            MarginModelHandle::from(pyobject_to_margin_model_any(&model).unwrap())
        });

        let result = std::thread::spawn(move || {
            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            handle
                .calculate_initial_margin(
                    &instrument,
                    Quantity::from(1),
                    Price::from("0.8"),
                    dec!(2.5),
                    Some(true),
                )
                .unwrap()
        })
        .join()
        .unwrap();

        assert_eq!(result, Money::from("45.67 USD"));
    }
}
