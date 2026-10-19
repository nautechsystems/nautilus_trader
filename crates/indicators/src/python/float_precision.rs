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

//! Float precision checks for market data passed to Python indicator handlers.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    types::fixed::check_float_precision,
};
use pyo3::PyResult;

pub(crate) fn check(precision: u8) -> PyResult<()> {
    check_float_precision(precision).map_err(to_pyvalue_err)
}

pub(crate) fn check_bar(bar: &Bar) -> PyResult<()> {
    check(bar.open.precision)?;
    check(bar.high.precision)?;
    check(bar.low.precision)?;
    check(bar.close.precision)
}

pub(crate) fn check_bar_volume(bar: &Bar) -> PyResult<()> {
    check(bar.volume.precision)
}

pub(crate) fn check_quote(quote: &QuoteTick) -> PyResult<()> {
    check(quote.bid_price.precision)?;
    check(quote.ask_price.precision)
}

pub(crate) fn check_trade(trade: &TradeTick) -> PyResult<()> {
    check(trade.price.precision)
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use nautilus_model::data::Bar;
    use pyo3::Python;
    use rstest::rstest;

    use super::check_bar;
    use crate::stubs::bar_ethusdt_binance_minute_bid;

    #[rstest]
    #[case::open(|bar: &mut Bar| bar.open.precision = 18)]
    #[case::high(|bar: &mut Bar| bar.high.precision = 18)]
    #[case::low(|bar: &mut Bar| bar.low.precision = 18)]
    #[case::close(|bar: &mut Bar| bar.close.precision = 18)]
    fn test_check_bar_rejects_any_price_above_float_precision(
        #[case] mutate: fn(&mut Bar),
        mut bar_ethusdt_binance_minute_bid: Bar,
    ) {
        ensure_python_initialized();
        mutate(&mut bar_ethusdt_binance_minute_bid);

        let error = check_bar(&bar_ethusdt_binance_minute_bid).unwrap_err();

        Python::attach(|py| {
            assert_eq!(
                error.value(py).to_string(),
                "Fixed-point precision 18 exceeds maximum float precision 16"
            );
        });
    }

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(Python::initialize);
    }
}
