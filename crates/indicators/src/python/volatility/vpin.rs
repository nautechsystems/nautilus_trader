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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::data::{Bar, QuoteTick, TradeTick};
use pyo3::prelude::*;

use crate::{indicator::Indicator, python::float_precision, volatility::vpin::Vpin};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Vpin {
    /// Volume-synchronized probability of informed trading (VPIN).
    ///
    /// Easley, Lopez de Prado and O'Hara (2012), "Flow Toxicity and Liquidity in a High-frequency
    /// World", Review of Financial Studies 25(5). Classified volume fills consecutive buckets of
    /// exactly `bucket_volume` units. A print that overflows a bucket is split, keeping its buy share,
    /// and the remainder carries into the next bucket. The value is the mean order imbalance
    /// `|V_buy - V_sell| / bucket_volume` over the last `window` full buckets, so it lies in `[0, 1]`.
    ///
    /// Volume is classified by input type:
    /// - Trades use the venue aggressor side. A trade with no aggressor falls back to the tick rule
    ///   (an uptick is a buy, a downtick a sell, an unchanged price repeats the last sign); a trade the
    ///   tick rule cannot sign yet stays out of the buckets.
    /// - Bars use bulk volume classification. The buy share of the bar volume is `Phi(dP / sigma)`,
    ///   where `dP` is the change from the previous close and `sigma` the sample standard deviation of
    ///   the last `sigma_window` close changes, the current one included. Bars that arrive before
    ///   `sigma_window` changes exist stay out of the buckets.
    /// - `Vpin.update_raw` takes buy and sell volume classified by the caller.
    ///
    /// `count` is the number of completed buckets. ELO (2012) size buckets at one fiftieth of average
    /// daily volume and average over 50 buckets.
    #[new]
    #[pyo3(signature = (bucket_volume, window, sigma_window=None))]
    fn py_new(bucket_volume: f64, window: usize, sigma_window: Option<usize>) -> PyResult<Self> {
        Self::new_checked(bucket_volume, window, sigma_window).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "Vpin({},{},{})",
            self.bucket_volume, self.window, self.sigma_window
        )
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "bucket_volume")]
    const fn py_bucket_volume(&self) -> f64 {
        self.bucket_volume
    }

    #[getter]
    #[pyo3(name = "window")]
    const fn py_window(&self) -> usize {
        self.window
    }

    #[getter]
    #[pyo3(name = "sigma_window")]
    const fn py_sigma_window(&self) -> usize {
        self.sigma_window
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "count")]
    const fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    /// Updates the indicator with buy and sell volume classified by the caller.
    ///
    /// Both sides may be non-zero, for example a bulk-classified bar. When the input spans
    /// several buckets, each portion keeps the input's buy share.
    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, buy_volume: f64, sell_volume: f64) {
        self.update_raw(buy_volume, sell_volume);
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, quote: &QuoteTick) -> PyResult<()> {
        self.handle_quote(quote).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, trade: &TradeTick) -> PyResult<()> {
        float_precision::check_trade(trade)?;
        float_precision::check(trade.size.precision)?;
        self.handle_trade(trade);
        Ok(())
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) -> PyResult<()> {
        float_precision::check_bar(bar)?;
        float_precision::check_bar_volume(bar)?;
        self.handle_bar(bar);
        Ok(())
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
