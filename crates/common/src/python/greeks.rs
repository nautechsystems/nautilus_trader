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

use std::collections::HashMap;

use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
};
use nautilus_model::{
    data::greeks::{GreeksData, PortfolioGreeks},
    enums::PositionSide,
    identifiers::{InstrumentId, StrategyId, Venue},
    position::Position,
    types::Price,
};
use pyo3::prelude::*;

use crate::{
    greeks::{GreeksCalculator, GreeksFilter},
    python::{cache::PyCache, clock::PyClock},
};

#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "GreeksCalculator",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
#[derive(Debug)]
pub struct PyGreeksCalculator(GreeksCalculator);

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyGreeksCalculator {
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(cache: PyCache, clock: PyClock) -> Self {
        Self(GreeksCalculator::new(cache.cache_rc(), clock.clock_rc()))
    }

    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    #[pyo3(
        name = "instrument_greeks",
        signature = (
            instrument_id,
            flat_interest_rate=0.0425,
            flat_dividend_yield=None,
            spot_shock=0.0,
            vol_shock=0.0,
            time_to_expiry_shock=0.0,
            use_cached_greeks=false,
            update_vol=false,
            cache_greeks=false,
            ts_event=0,
            position=None,
            percent_greeks=false,
            index_instrument_id=None,
            beta_weights=None,
            vega_time_weight_base=None
        )
    )]
    fn py_instrument_greeks(
        &self,
        instrument_id: InstrumentId,
        flat_interest_rate: f64,
        flat_dividend_yield: Option<f64>,
        spot_shock: f64,
        vol_shock: f64,
        time_to_expiry_shock: f64,
        use_cached_greeks: bool,
        update_vol: bool,
        cache_greeks: bool,
        ts_event: u64,
        position: Option<Position>,
        percent_greeks: bool,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<HashMap<InstrumentId, f64>>,
        vega_time_weight_base: Option<i32>,
    ) -> PyResult<GreeksData> {
        self.0
            .instrument_greeks(
                instrument_id,
                Some(flat_interest_rate),
                flat_dividend_yield,
                Some(spot_shock),
                Some(vol_shock),
                Some(time_to_expiry_shock),
                Some(use_cached_greeks),
                Some(update_vol),
                Some(cache_greeks),
                Some(false),
                (ts_event != 0).then(|| UnixNanos::from(ts_event)),
                position,
                Some(percent_greeks),
                index_instrument_id,
                beta_weights.as_ref(),
                vega_time_weight_base,
            )
            .map_err(to_pyvalue_err)
    }

    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    #[pyo3(
        name = "modify_greeks",
        signature = (
            delta_input,
            gamma_input,
            underlying_instrument_id,
            underlying_price,
            unshocked_underlying_price,
            percent_greeks,
            index_instrument_id=None,
            beta_weights=None,
            vega_input=0.0,
            vol=0.0,
            expiry_in_days=0,
            vega_time_weight_base=None
        )
    )]
    fn py_modify_greeks(
        &self,
        delta_input: f64,
        gamma_input: f64,
        underlying_instrument_id: InstrumentId,
        underlying_price: f64,
        unshocked_underlying_price: f64,
        percent_greeks: bool,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<HashMap<InstrumentId, f64>>,
        vega_input: f64,
        vol: f64,
        expiry_in_days: i32,
        vega_time_weight_base: Option<i32>,
    ) -> (f64, f64, f64) {
        self.0.modify_greeks(
            delta_input,
            gamma_input,
            underlying_instrument_id,
            underlying_price,
            unshocked_underlying_price,
            percent_greeks,
            index_instrument_id,
            beta_weights.as_ref(),
            vega_input,
            vol,
            expiry_in_days,
            vega_time_weight_base,
        )
    }

    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    #[pyo3(
        name = "portfolio_greeks",
        signature = (
            underlyings=None,
            venue=None,
            instrument_id=None,
            strategy_id=None,
            side=None,
            flat_interest_rate=0.0425,
            flat_dividend_yield=None,
            spot_shock=0.0,
            vol_shock=0.0,
            time_to_expiry_shock=0.0,
            use_cached_greeks=false,
            update_vol=false,
            cache_greeks=false,
            percent_greeks=false,
            index_instrument_id=None,
            beta_weights=None,
            greeks_filter=None,
            vega_time_weight_base=None
        )
    )]
    fn py_portfolio_greeks(
        &self,
        underlyings: Option<Vec<String>>,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<PositionSide>,
        flat_interest_rate: f64,
        flat_dividend_yield: Option<f64>,
        spot_shock: f64,
        vol_shock: f64,
        time_to_expiry_shock: f64,
        use_cached_greeks: bool,
        update_vol: bool,
        cache_greeks: bool,
        percent_greeks: bool,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<HashMap<InstrumentId, f64>>,
        greeks_filter: Option<Py<PyAny>>,
        vega_time_weight_base: Option<i32>,
    ) -> PyResult<PortfolioGreeks> {
        let greeks_filter: Option<GreeksFilter> = greeks_filter.map(|callback| {
            Box::new(move |data: &GreeksData| {
                Python::attach(|py| {
                    callback
                        .bind(py)
                        .call1((data.clone().into_py_any_unwrap(py),))
                        .and_then(|result| result.extract::<bool>())
                        .unwrap_or(false)
                })
            }) as GreeksFilter
        });

        self.0
            .portfolio_greeks(
                underlyings.as_deref(),
                venue,
                instrument_id,
                strategy_id,
                Some(side.unwrap_or(PositionSide::NoPositionSide)),
                Some(flat_interest_rate),
                flat_dividend_yield,
                Some(spot_shock),
                Some(vol_shock),
                Some(time_to_expiry_shock),
                Some(use_cached_greeks),
                Some(update_vol),
                Some(cache_greeks),
                Some(false),
                Some(percent_greeks),
                index_instrument_id,
                beta_weights.as_ref(),
                greeks_filter.as_ref(),
                vega_time_weight_base,
            )
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "cache_futures_spread")]
    fn py_cache_futures_spread(
        &self,
        call_instrument_id: InstrumentId,
        put_instrument_id: InstrumentId,
        futures_instrument_id: InstrumentId,
    ) -> PyResult<Price> {
        self.0
            .cache_futures_spread(call_instrument_id, put_instrument_id, futures_instrument_id)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "get_cached_futures_spread_price")]
    fn py_get_cached_futures_spread_price(
        &self,
        underlying_instrument_id: InstrumentId,
    ) -> Option<Price> {
        self.0
            .get_cached_futures_spread_price(underlying_instrument_id)
    }
}
