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

use crate::{indicator::Indicator, volatility::vpin::Vpin};

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
    /// Volume classification depends on the input type. Trades and bars for the same market activity
    /// can produce different values. Use one input source per event to avoid counting volume twice.
    ///
    /// - Trades use the venue aggressor side. A trade with no aggressor falls back to the tick rule
    ///   (an uptick is a buy, a downtick is a sell, and an unchanged price repeats the last nonzero
    ///   tick direction). Unsigned trades before the first price change contribute no volume.
    /// - Bars use bulk volume classification. The buy share of the bar volume is `Phi(dP / sigma)`,
    ///   where `dP` is the change from the previous close and `sigma` the sample standard deviation of
    ///   the last `sigma_window` close changes, including the current one. Pass `sigma_window` to
    ///   override the default of 50 changes. Bars contribute no volume until the window fills.
    ///   Any zero-variance window splits the bar volume 50/50.
    /// - `Vpin.update_raw` uses the buy and sell volume supplied by the caller.
    ///
    /// `count` tracks completed buckets and saturates at the platform counter limit. The indicator
    /// initializes after `window` buckets close.
    ///
    /// Bar and trade handlers use fixed-point quantities when `bucket_volume` fits the model's
    /// fixed-point scale. Updates that take floating-point volumes use the supplied `f64` values.
    /// A decimal sum that would fill a bucket can fall short when supplied as binary floats.
    /// Once fixed-point accounting begins, floating-point volumes contribute the same fraction of
    /// a bucket in fixed-point units. Rounding when mixing volume sources can delay bucket completion.
    /// Bucket sizes outside the model scale use floating-point rounding for all volume inputs.
    ///
    /// The paper sizes buckets at one fiftieth of average daily volume and averages over 50 buckets.
    /// VPIN is a research indicator with contested predictive value. Thresholds are strategy-specific.
    #[new]
    #[pyo3(signature = (bucket_volume, window, sigma_window=None))]
    fn py_new(bucket_volume: f64, window: usize, sigma_window: Option<usize>) -> PyResult<Self> {
        Self::new_checked(bucket_volume, window, sigma_window).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    /// Returns the volume of a complete bucket.
    #[getter]
    #[pyo3(name = "bucket_volume")]
    const fn py_bucket_volume(&self) -> f64 {
        self.bucket_volume()
    }

    /// Returns the number of completed buckets in the VPIN average.
    #[getter]
    #[pyo3(name = "window")]
    const fn py_window(&self) -> usize {
        self.window()
    }

    /// Returns the number of bar close changes used to estimate the sample standard deviation.
    #[getter]
    #[pyo3(name = "sigma_window")]
    const fn py_sigma_window(&self) -> usize {
        self.sigma_window()
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
    /// Both sides may be nonzero. When an input spans several buckets, each portion keeps the
    /// input's buy share. Volumes aggregated by side carry no trade order.
    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, buy_volume: f64, sell_volume: f64) {
        self.update_raw(buy_volume, sell_volume);
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, quote: &QuoteTick) -> PyResult<()> {
        self.handle_quote(quote).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, trade: &TradeTick) {
        self.handle_trade(trade);
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.handle_bar(bar);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
