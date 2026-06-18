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
            vega_time_weight_base=None,
            vol_index_instrument_id=None,
            vol_beta_weights=None
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
        vol_index_instrument_id: Option<InstrumentId>,
        vol_beta_weights: Option<HashMap<InstrumentId, f64>>,
    ) -> PyResult<Option<GreeksData>> {
        match self.0.instrument_greeks(
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
            vol_index_instrument_id,
            vol_beta_weights.as_ref(),
        ) {
            Ok(greeks) => Ok(Some(greeks)),
            Err(e) if is_missing_market_data_error(&e) => Ok(None),
            Err(e) => Err(to_pyvalue_err(e)),
        }
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
            vega_time_weight_base=None,
            unshocked_vol=0.0,
            vol_index_instrument_id=None,
            vol_beta_weights=None,
            index_price=None,
            vol_index_price=None
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
        unshocked_vol: f64,
        vol_index_instrument_id: Option<InstrumentId>,
        vol_beta_weights: Option<HashMap<InstrumentId, f64>>,
        index_price: Option<f64>,
        vol_index_price: Option<f64>,
    ) -> PyResult<(f64, f64, f64)> {
        self.0
            .modify_greeks(
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
                unshocked_vol,
                vol_index_instrument_id,
                vol_beta_weights.as_ref(),
                index_price,
                vol_index_price,
            )
            .map_err(to_pyvalue_err)
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
            vega_time_weight_base=None,
            vol_index_instrument_id=None,
            vol_beta_weights=None
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
        vol_index_instrument_id: Option<InstrumentId>,
        vol_beta_weights: Option<HashMap<InstrumentId, f64>>,
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
                vol_index_instrument_id,
                vol_beta_weights.as_ref(),
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

fn is_missing_market_data_error(error: &anyhow::Error) -> bool {
    error.to_string().starts_with("No price available for ")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_model::{
        data::QuoteTick,
        identifiers::InstrumentId,
        instruments::{
            Instrument, InstrumentAny,
            stubs::{equity_aapl, option_contract_appl},
        },
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{cache::Cache, clock::TestClock};

    #[derive(Clone, Copy)]
    enum MissingPriceCase {
        Option,
        Underlying,
        VolIndex,
        NonOption,
    }

    #[rstest]
    #[case::option_price(MissingPriceCase::Option)]
    #[case::underlying_price(MissingPriceCase::Underlying)]
    #[case::vol_index_price(MissingPriceCase::VolIndex)]
    #[case::non_option_price(MissingPriceCase::NonOption)]
    fn test_py_instrument_greeks_returns_none_when_market_price_missing(
        #[case] case: MissingPriceCase,
    ) {
        let (calculator, instrument_id, vol_index_instrument_id) =
            calculator_for_missing_price_case(case);

        let result =
            py_instrument_greeks(&calculator, instrument_id, vol_index_instrument_id).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_py_instrument_greeks_raises_when_instrument_missing() {
        Python::initialize();
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let calculator = make_calculator(cache);

        let error = py_instrument_greeks(
            &calculator,
            InstrumentId::from("AAPL211217C00150000.OPRA"),
            None,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Instrument definition for AAPL211217C00150000.OPRA not found")
        );
    }

    fn py_instrument_greeks(
        calculator: &PyGreeksCalculator,
        instrument_id: InstrumentId,
        vol_index_instrument_id: Option<InstrumentId>,
    ) -> PyResult<Option<GreeksData>> {
        calculator.py_instrument_greeks(
            instrument_id,
            0.0425,
            None,
            0.0,
            0.0,
            0.0,
            false,
            false,
            false,
            0,
            None,
            false,
            None,
            None,
            None,
            vol_index_instrument_id,
            None,
        )
    }

    fn calculator_for_missing_price_case(
        case: MissingPriceCase,
    ) -> (PyGreeksCalculator, InstrumentId, Option<InstrumentId>) {
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let option = option_contract_appl();
        let option_id = option.id();
        let underlying_id = InstrumentId::from("AAPL.OPRA");
        let vol_index_id = InstrumentId::from("VIX.XCBF");

        match case {
            MissingPriceCase::Option => {
                cache
                    .borrow_mut()
                    .add_instrument(InstrumentAny::OptionContract(option))
                    .unwrap();

                (make_calculator(cache), option_id, None)
            }
            MissingPriceCase::Underlying => {
                cache
                    .borrow_mut()
                    .add_instrument(InstrumentAny::OptionContract(option))
                    .unwrap();
                add_quote(&cache, option_id, "10.50");

                (make_calculator(cache), option_id, None)
            }
            MissingPriceCase::VolIndex => {
                cache
                    .borrow_mut()
                    .add_instrument(InstrumentAny::OptionContract(option))
                    .unwrap();
                add_quote(&cache, option_id, "10.50");
                add_quote(&cache, underlying_id, "150.00");

                (make_calculator(cache), option_id, Some(vol_index_id))
            }
            MissingPriceCase::NonOption => {
                let equity = equity_aapl();
                let instrument_id = equity.id();
                cache
                    .borrow_mut()
                    .add_instrument(InstrumentAny::Equity(equity))
                    .unwrap();

                (make_calculator(cache), instrument_id, None)
            }
        }
    }

    fn add_quote(cache: &Rc<RefCell<Cache>>, instrument_id: InstrumentId, price: &str) {
        let ts = UnixNanos::from(1u64);
        cache
            .borrow_mut()
            .add_quote(QuoteTick::new(
                instrument_id,
                Price::from(price),
                Price::from(price),
                Quantity::from(100),
                Quantity::from(100),
                ts,
                ts,
            ))
            .unwrap();
    }

    fn make_calculator(cache: Rc<RefCell<Cache>>) -> PyGreeksCalculator {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        PyGreeksCalculator(GreeksCalculator::new(cache, clock))
    }
}
