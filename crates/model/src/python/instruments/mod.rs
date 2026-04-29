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

//! Instrument definitions the trading domain model.

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    IntoPyObjectExt, Py, PyAny, PyResult, Python,
    types::{PyAnyMethods, PyDict, PyDictMethods},
};

use crate::{
    instruments::{
        BettingInstrument, BinaryOption, Cfd, Commodity, CryptoFuture, CryptoPerpetual,
        CurrencyPair, Equity, FuturesContract, FuturesSpread, IndexInstrument, InstrumentAny,
        OptionContract, OptionSpread, PerpetualContract, TokenizedAsset,
        crypto_option::CryptoOption,
    },
    types::{Currency, Money, Price, Quantity},
};

/// Pre-registers crypto currency codes from a dict prior to strict deserialization.
///
/// Crypto instrument roundtrips (e.g. `CryptoPerpetual.from_dict(...)`) can carry
/// newly listed assets not present in the built-in currency map. Looking up each
/// named field with [`Currency::get_or_create_crypto`] registers any unknown code
/// as a crypto currency (precision 8), mirroring the non-strict Cython path.
///
/// Callers must only pass fields that are guaranteed to hold crypto assets (the
/// underlying of a derivative); `quote_currency` and `settlement_currency` can
/// legitimately be fiat (e.g. inverse perps on BitMEX quoted in USD) and must
/// stay on the strict deserialization path.
///
/// Codes are trimmed before lookup; empty or whitespace-only values are skipped
/// so downstream serde deserialization raises a normal `PyErr` instead of
/// panicking in `Currency::new`.
pub(crate) fn register_crypto_currencies_from_dict(
    py: Python<'_>,
    values: &Py<PyDict>,
    fields: &[&str],
) {
    let dict = values.bind(py);
    for field in fields {
        if let Ok(Some(value)) = dict.get_item(field)
            && let Ok(code) = value.extract::<String>()
        {
            let trimmed = code.trim();
            if !trimmed.is_empty() {
                let _ = Currency::get_or_create_crypto(trimmed);
            }
        }
    }
}

macro_rules! impl_instrument_common_pymethods {
    ($type:ty) => {
        #[pyo3::pymethods]
        impl $type {
            fn __repr__(&self) -> String {
                use crate::instruments::Instrument;
                format!(
                    "{}(id={}, price_precision={}, size_precision={})",
                    stringify!($type),
                    self.id(),
                    self.price_precision(),
                    self.size_precision(),
                )
            }

            /// Returns a price rounded to the instruments price precision.
            #[pyo3(name = "make_price")]
            fn py_make_price(&self, value: f64) -> pyo3::PyResult<Price> {
                use crate::instruments::Instrument;
                self.try_make_price(value)
                    .map_err(nautilus_core::python::to_pyvalue_err)
            }

            /// Returns a quantity rounded to the instruments size precision.
            #[pyo3(name = "make_qty")]
            #[pyo3(signature = (value, round_down=false))]
            fn py_make_qty(&self, value: f64, round_down: bool) -> pyo3::PyResult<Quantity> {
                use crate::instruments::Instrument;
                self.try_make_qty(value, Some(round_down))
                    .map_err(nautilus_core::python::to_pyvalue_err)
            }

            /// Calculates the notional value from the given quantity and price.
            #[pyo3(name = "notional_value")]
            #[pyo3(signature = (quantity, price, use_quote_for_inverse=false))]
            fn py_notional_value(
                &self,
                quantity: Quantity,
                price: Price,
                use_quote_for_inverse: bool,
            ) -> Money {
                use crate::instruments::Instrument;
                self.calculate_notional_value(quantity, price, Some(use_quote_for_inverse))
            }
        }
    };
}

impl_instrument_common_pymethods!(BettingInstrument);
impl_instrument_common_pymethods!(BinaryOption);
impl_instrument_common_pymethods!(Cfd);
impl_instrument_common_pymethods!(Commodity);
impl_instrument_common_pymethods!(CryptoFuture);
impl_instrument_common_pymethods!(CryptoOption);
impl_instrument_common_pymethods!(CryptoPerpetual);
impl_instrument_common_pymethods!(CurrencyPair);
impl_instrument_common_pymethods!(Equity);
impl_instrument_common_pymethods!(FuturesContract);
impl_instrument_common_pymethods!(FuturesSpread);
impl_instrument_common_pymethods!(IndexInstrument);
impl_instrument_common_pymethods!(OptionContract);
impl_instrument_common_pymethods!(OptionSpread);
impl_instrument_common_pymethods!(PerpetualContract);
impl_instrument_common_pymethods!(TokenizedAsset);

pub mod betting;
pub mod binary_option;
pub mod cfd;
pub mod commodity;
pub mod crypto_future;
pub mod crypto_option;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod index_instrument;
pub mod option_contract;
pub mod option_spread;
pub mod perpetual_contract;
pub mod synthetic;
pub mod tokenized_asset;

/// Converts an [`InstrumentAny`] into a Python object.
///
/// # Errors
///
/// Returns a `PyErr` if conversion to a Python object fails.
pub fn instrument_any_to_pyobject(py: Python, instrument: InstrumentAny) -> PyResult<Py<PyAny>> {
    match instrument {
        InstrumentAny::Betting(inst) => inst.into_py_any(py),
        InstrumentAny::BinaryOption(inst) => inst.into_py_any(py),
        InstrumentAny::Cfd(inst) => inst.into_py_any(py),
        InstrumentAny::Commodity(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoFuture(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoOption(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoPerpetual(inst) => inst.into_py_any(py),
        InstrumentAny::CurrencyPair(inst) => inst.into_py_any(py),
        InstrumentAny::Equity(inst) => inst.into_py_any(py),
        InstrumentAny::FuturesContract(inst) => inst.into_py_any(py),
        InstrumentAny::FuturesSpread(inst) => inst.into_py_any(py),
        InstrumentAny::IndexInstrument(inst) => inst.into_py_any(py),
        InstrumentAny::OptionContract(inst) => inst.into_py_any(py),
        InstrumentAny::OptionSpread(inst) => inst.into_py_any(py),
        InstrumentAny::PerpetualContract(inst) => inst.into_py_any(py),
        InstrumentAny::TokenizedAsset(inst) => inst.into_py_any(py),
    }
}

/// Converts a Python object into an [`InstrumentAny`] enum.
///
/// # Errors
///
/// Returns a `PyErr` if extraction fails or the instrument type is unsupported.
#[expect(clippy::needless_pass_by_value)]
pub fn pyobject_to_instrument_any(py: Python, instrument: Py<PyAny>) -> PyResult<InstrumentAny> {
    match instrument.getattr(py, "type_name")?.extract::<&str>(py)? {
        stringify!(BettingInstrument) => Ok(InstrumentAny::Betting(
            instrument.extract::<BettingInstrument>(py)?,
        )),
        stringify!(BinaryOption) => Ok(InstrumentAny::BinaryOption(
            instrument.extract::<BinaryOption>(py)?,
        )),
        stringify!(Cfd) => Ok(InstrumentAny::Cfd(instrument.extract::<Cfd>(py)?)),
        stringify!(Commodity) => Ok(InstrumentAny::Commodity(
            instrument.extract::<Commodity>(py)?,
        )),
        stringify!(CryptoFuture) => Ok(InstrumentAny::CryptoFuture(
            instrument.extract::<CryptoFuture>(py)?,
        )),
        stringify!(CryptoOption) => Ok(InstrumentAny::CryptoOption(
            instrument.extract::<CryptoOption>(py)?,
        )),
        stringify!(CryptoPerpetual) => Ok(InstrumentAny::CryptoPerpetual(
            instrument.extract::<CryptoPerpetual>(py)?,
        )),
        stringify!(CurrencyPair) => Ok(InstrumentAny::CurrencyPair(
            instrument.extract::<CurrencyPair>(py)?,
        )),
        stringify!(Equity) => Ok(InstrumentAny::Equity(instrument.extract::<Equity>(py)?)),
        stringify!(FuturesContract) => Ok(InstrumentAny::FuturesContract(
            instrument.extract::<FuturesContract>(py)?,
        )),
        stringify!(FuturesSpread) => Ok(InstrumentAny::FuturesSpread(
            instrument.extract::<FuturesSpread>(py)?,
        )),
        stringify!(IndexInstrument) => Ok(InstrumentAny::IndexInstrument(
            instrument.extract::<IndexInstrument>(py)?,
        )),
        stringify!(OptionContract) => Ok(InstrumentAny::OptionContract(
            instrument.extract::<OptionContract>(py)?,
        )),
        stringify!(OptionSpread) => Ok(InstrumentAny::OptionSpread(
            instrument.extract::<OptionSpread>(py)?,
        )),
        stringify!(PerpetualContract) => Ok(InstrumentAny::PerpetualContract(
            instrument.extract::<PerpetualContract>(py)?,
        )),
        stringify!(TokenizedAsset) => Ok(InstrumentAny::TokenizedAsset(
            instrument.extract::<TokenizedAsset>(py)?,
        )),
        _ => Err(to_pyvalue_err(
            "Error in conversion from `Py<PyAny>` to `InstrumentAny`",
        )),
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, types::PyDict};
    use rstest::rstest;

    use super::register_crypto_currencies_from_dict;
    use crate::{enums::CurrencyType, types::Currency};

    #[rstest]
    fn test_register_crypto_currencies_from_dict_unknown_code() {
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("base_currency", "NEWHLP1").unwrap();
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["base_currency"]);

            let created = Currency::try_from_str("NEWHLP1").unwrap();
            assert_eq!(created.precision, 8);
            assert_eq!(created.currency_type, CurrencyType::Crypto);
        });
    }

    #[rstest]
    fn test_register_crypto_currencies_from_dict_known_code_not_overwritten() {
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("quote_currency", "USD").unwrap();
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["quote_currency"]);

            let usd = Currency::try_from_str("USD").unwrap();
            assert_eq!(usd.precision, 2);
            assert_eq!(usd.currency_type, CurrencyType::Fiat);
        });
    }

    #[rstest]
    fn test_register_crypto_currencies_from_dict_missing_key() {
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["base_currency"]);

            assert!(Currency::try_from_str("base_currency").is_none());
        });
    }

    #[rstest]
    fn test_register_crypto_currencies_from_dict_non_string_value() {
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("base_currency", 42).unwrap();
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["base_currency"]);

            assert!(Currency::try_from_str("42").is_none());
        });
    }

    #[rstest]
    fn test_register_crypto_currencies_from_dict_trims_padding() {
        // Whitespace-padded codes must be trimmed before registration so the
        // global map doesn't accumulate `" BTC "`-style garbage entries.
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("base_currency", "  NEWHLP2  ").unwrap();
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["base_currency"]);

            assert!(Currency::try_from_str("NEWHLP2").is_some());
            assert!(Currency::try_from_str("  NEWHLP2  ").is_none());
        });
    }

    #[rstest]
    fn test_register_crypto_currencies_from_dict_blank_code_skipped() {
        // Blank or whitespace-only codes must be skipped so strict deserialize produces
        // a normal PyErr, not a panic from `Currency::new` via get_or_create_crypto.
        Python::initialize();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("base_currency", "").unwrap();
            dict.set_item("quote_currency", "   ").unwrap();
            let values: Py<PyDict> = dict.unbind();

            register_crypto_currencies_from_dict(py, &values, &["base_currency", "quote_currency"]);

            assert!(Currency::try_from_str("").is_none());
            assert!(Currency::try_from_str("   ").is_none());
        });
    }
}
