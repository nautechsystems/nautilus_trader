use ahash::HashMap;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::enums::PriceType;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::xrate::get_exchange_rate;

/// Calculates the exchange rate between two currencies using provided bid and ask quotes.
///
/// # Errors
///
/// Returns an error if:
/// - `price_type` is equal to `Last` or `Mark` (cannot calculate from quotes).
/// - `quotes_bid` or `quotes_ask` is empty.
/// - `quotes_bid` and `quotes_ask` lengths are not equal.
/// - The bid or ask side of a pair is missing.
#[pyfunction]
#[pyo3(name = "get_exchange_rate")]
#[pyo3(signature = (from_currency, to_currency, price_type, quotes_bid, quotes_ask))]
pub fn py_get_exchange_rate(
    from_currency: &str,
    to_currency: &str,
    price_type: PriceType,
    quotes_bid: HashMap<String, f64>,
    quotes_ask: HashMap<String, f64>,
) -> PyResult<Option<f64>> {
    get_exchange_rate(
        Ustr::from(from_currency),
        Ustr::from(to_currency),
        price_type,
        quotes_bid.into_iter().collect(),
        quotes_ask.into_iter().collect(),
    )
    .map_err(to_pyvalue_err)
}
