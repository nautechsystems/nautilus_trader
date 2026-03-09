use pyo3::prelude::*;
use strum::IntoEnumIterator;

use crate::enums::TardisExchange;

#[must_use]
#[pyfunction(name = "tardis_exchanges")]
pub fn py_tardis_exchanges() -> Vec<String> {
    TardisExchange::iter().map(|e| e.to_string()).collect()
}

#[must_use]
#[pyfunction(name = "tardis_exchange_from_venue_str")]
pub fn py_tardis_exchange_from_venue_str(venue_str: &str) -> Vec<String> {
    TardisExchange::from_venue_str(venue_str)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[must_use]
#[pyfunction(name = "tardis_exchange_to_venue_str")]
pub fn py_tardis_exchange_to_venue_str(exchange_str: &str) -> String {
    match exchange_str.parse::<TardisExchange>() {
        Ok(exchange) => exchange.as_venue_str().to_string(),
        Err(_) => String::new(),
    }
}

#[must_use]
#[pyfunction(name = "tardis_exchange_is_option_exchange")]
pub fn py_tardis_exchange_is_option_exchange(exchange_str: &str) -> bool {
    match exchange_str.parse::<TardisExchange>() {
        Ok(exchange) => exchange.is_option_exchange(),
        Err(_) => false,
    }
}
