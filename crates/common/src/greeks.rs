// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Greeks calculator for options and futures.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use anyhow;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::greeks::{GreeksData, PortfolioGreeks, black_scholes_greeks, imply_vol_and_greeks},
    enums::{InstrumentClass, OptionKind, PositionSide, PriceType},
    identifiers::{InstrumentId, StrategyId, Venue},
    instruments::Instrument,
    position::Position,
};
use ustr::Ustr;

use crate::{cache::Cache, clock::Clock, msgbus};

/// Calculates instrument and portfolio greeks (sensitivities of price moves with respect to market data moves).
///
/// Useful for risk management of options and futures portfolios.
///
/// Currently implemented greeks are:
/// - Delta (first derivative of price with respect to spot move).
/// - Gamma (second derivative of price with respect to spot move).
/// - Vega (first derivative of price with respect to implied volatility of an option).
/// - Theta (first derivative of price with respect to time to expiry).
///
/// Vega is expressed in terms of absolute percent changes ((dV / dVol) / 100).
/// Theta is expressed in terms of daily changes ((dV / d(T-t)) / 365.25, where T is the expiry of an option and t is the current time).
///
/// Also note that for ease of implementation we consider that american options (for stock options for example) are european for the computation of greeks.
#[allow(dead_code)]
pub struct GreeksCalculator {
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
}

impl GreeksCalculator {
    /// Creates a new [`GreeksCalculator`] instance.
    pub fn new(cache: Rc<RefCell<Cache>>, clock: Rc<RefCell<dyn Clock>>) -> Self {
        Self { cache, clock }
    }

    /// Calculates option or underlying greeks for a given instrument and a quantity of 1.
    ///
    /// Additional features:
    /// - Apply shocks to the spot value of the instrument's underlying, implied volatility or time to expiry.
    /// - Compute percent greeks.
    /// - Compute beta-weighted delta and gamma with respect to an index.
    #[allow(clippy::too_many_arguments)]
    pub fn instrument_greeks(
        &self,
        instrument_id: InstrumentId,
        flat_interest_rate: Option<f64>,
        flat_dividend_yield: Option<f64>,
        spot_shock: Option<f64>,
        vol_shock: Option<f64>,
        time_to_expiry_shock: Option<f64>,
        use_cached_greeks: Option<bool>,
        cache_greeks: Option<bool>,
        publish_greeks: Option<bool>,
        ts_event: Option<UnixNanos>,
        position: Option<Position>,
        percent_greeks: Option<bool>,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<HashMap<InstrumentId, f64>>,
    ) -> anyhow::Result<GreeksData> {
        // Set default values
        let flat_interest_rate = flat_interest_rate.unwrap_or(0.0425);
        let spot_shock = spot_shock.unwrap_or(0.0);
        let vol_shock = vol_shock.unwrap_or(0.0);
        let time_to_expiry_shock = time_to_expiry_shock.unwrap_or(0.0);
        let use_cached_greeks = use_cached_greeks.unwrap_or(false);
        let cache_greeks = cache_greeks.unwrap_or(false);
        let publish_greeks = publish_greeks.unwrap_or(false);
        let ts_event = ts_event.unwrap_or_default();
        let percent_greeks = percent_greeks.unwrap_or(false);

        let cache = self.cache.borrow();
        let instrument = cache.instrument(&instrument_id);
        let instrument = match instrument {
            Some(instrument) => instrument,
            None => anyhow::bail!(format!(
                "Instrument definition for {instrument_id} not found."
            )),
        };

        if instrument.instrument_class() != InstrumentClass::Option {
            let multiplier = instrument.multiplier();
            let underlying_instrument_id = instrument.id();
            let underlying_price = cache
                .price(&underlying_instrument_id, PriceType::Last)
                .unwrap_or_default()
                .as_f64();
            let (delta, _) = self.modify_greeks(
                multiplier.as_f64(),
                0.0,
                underlying_instrument_id,
                underlying_price + spot_shock,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
            );
            let mut greeks_data =
                GreeksData::from_delta(instrument_id, delta, multiplier.as_f64(), ts_event);

            if let Some(pos) = position {
                greeks_data.pnl = multiplier * ((underlying_price + spot_shock) - pos.avg_px_open);
                greeks_data.price = greeks_data.pnl;
            }

            return Ok(greeks_data);
        }

        let mut greeks_data = None;
        let underlying = instrument.underlying().unwrap();
        let underlying_str = format!("{}.{}", underlying, instrument_id.venue);
        let underlying_instrument_id = InstrumentId::from(underlying_str.as_str());

        // Use cached greeks if requested
        if use_cached_greeks {
            if let Some(cached_greeks) = cache.greeks(&instrument_id) {
                greeks_data = Some(cached_greeks);
            }
        }

        if greeks_data.is_none() {
            let utc_now_ns = if ts_event != UnixNanos::default() {
                ts_event
            } else {
                self.clock.borrow().timestamp_ns()
            };

            let utc_now = utc_now_ns.to_datetime_utc();
            let expiry_utc = instrument
                .expiration_ns()
                .map(|ns| ns.to_datetime_utc())
                .unwrap_or_default();
            let expiry_int = expiry_utc
                .format("%Y%m%d")
                .to_string()
                .parse::<i32>()
                .unwrap_or(0);
            let expiry_in_years = (expiry_utc - utc_now).num_days().min(1) as f64 / 365.25;
            let currency = instrument.quote_currency().code.to_string();
            let interest_rate = match cache.yield_curve(&currency) {
                Some(yield_curve) => yield_curve(expiry_in_years),
                None => flat_interest_rate,
            };

            // cost of carry is 0 for futures
            let mut cost_of_carry = 0.0;

            if let Some(dividend_curve) = cache.yield_curve(&underlying_instrument_id.to_string()) {
                let dividend_yield = dividend_curve(expiry_in_years);
                cost_of_carry = interest_rate - dividend_yield;
            } else if let Some(div_yield) = flat_dividend_yield {
                // Use a dividend rate of 0. to have a cost of carry of interest rate for options on stocks
                cost_of_carry = interest_rate - div_yield;
            }

            let multiplier = instrument.multiplier();
            let is_call = instrument.option_kind().unwrap_or(OptionKind::Call) == OptionKind::Call;
            let strike = instrument.strike_price().unwrap_or_default().as_f64();
            let option_mid_price = cache
                .price(&instrument_id, PriceType::Mid)
                .unwrap_or_default()
                .as_f64();
            let underlying_price = cache
                .price(&underlying_instrument_id, PriceType::Last)
                .unwrap_or_default()
                .as_f64();

            let greeks = imply_vol_and_greeks(
                underlying_price,
                interest_rate,
                cost_of_carry,
                is_call,
                strike,
                expiry_in_years,
                option_mid_price,
                multiplier.as_f64(),
            );
            let (delta, gamma) = self.modify_greeks(
                greeks.delta,
                greeks.gamma,
                underlying_instrument_id,
                underlying_price,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
            );
            greeks_data = Some(GreeksData::new(
                utc_now_ns,
                utc_now_ns,
                instrument_id,
                is_call,
                strike,
                expiry_int,
                expiry_in_years,
                multiplier.as_f64(),
                1.0,
                underlying_price,
                interest_rate,
                cost_of_carry,
                greeks.vol,
                0.0,
                greeks.price,
                delta,
                gamma,
                greeks.vega,
                greeks.theta,
                (greeks.delta / multiplier.as_f64()).abs(),
            ));

            // Adding greeks to cache if requested
            if cache_greeks {
                let mut cache = self.cache.borrow_mut();
                cache
                    .add_greeks(greeks_data.clone().unwrap())
                    .unwrap_or_default();
            }

            // Publishing greeks on the message bus if requested
            if publish_greeks {
                let topic_str = format!(
                    "data.GreeksData.instrument_id={}",
                    instrument_id.symbol.as_str()
                );
                let topic = Ustr::from(topic_str.as_str());
                msgbus::publish(&topic, &greeks_data.clone().unwrap());
            }
        }

        let mut greeks_data = greeks_data.unwrap();

        if spot_shock != 0.0 || vol_shock != 0.0 || time_to_expiry_shock != 0.0 {
            let underlying_price = greeks_data.underlying_price;
            let shocked_underlying_price = underlying_price + spot_shock;
            let shocked_vol = greeks_data.vol + vol_shock;
            let shocked_time_to_expiry = greeks_data.expiry_in_years - time_to_expiry_shock;

            let greeks = black_scholes_greeks(
                shocked_underlying_price,
                greeks_data.interest_rate,
                greeks_data.cost_of_carry,
                shocked_vol,
                greeks_data.is_call,
                greeks_data.strike,
                shocked_time_to_expiry,
                greeks_data.multiplier,
            );
            let (delta, gamma) = self.modify_greeks(
                greeks.delta,
                greeks.gamma,
                underlying_instrument_id,
                shocked_underlying_price,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
            );
            greeks_data = GreeksData::new(
                greeks_data.ts_event,
                greeks_data.ts_event,
                greeks_data.instrument_id,
                greeks_data.is_call,
                greeks_data.strike,
                greeks_data.expiry,
                shocked_time_to_expiry,
                greeks_data.multiplier,
                greeks_data.quantity,
                shocked_underlying_price,
                greeks_data.interest_rate,
                greeks_data.cost_of_carry,
                shocked_vol,
                0.0,
                greeks.price,
                delta,
                gamma,
                greeks.vega,
                greeks.theta,
                (greeks.delta / greeks_data.multiplier).abs(),
            );
        }

        if let Some(pos) = position {
            greeks_data.pnl = greeks_data.price - greeks_data.multiplier * pos.avg_px_open;
        }

        Ok(greeks_data)
    }

    /// Modifies delta and gamma based on beta weighting and percentage calculations.
    ///
    /// The beta weighting of delta and gamma follows this equation linking the returns of a stock x to the ones of an index I:
    /// (x - x0) / x0 = alpha + beta (I - I0) / I0 + epsilon
    ///
    /// beta can be obtained by linear regression of stock_return = alpha + beta index_return, it's equal to:
    /// beta = Covariance(stock_returns, index_returns) / Variance(index_returns)
    ///
    /// Considering alpha == 0:
    /// x = x0 + beta x0 / I0 (I-I0)
    /// I = I0 + 1 / beta I0 / x0 (x - x0)
    ///
    /// These two last equations explain the beta weighting below, considering the price of an option is V(x) and delta and gamma
    /// are the first and second derivatives respectively of V.
    ///
    /// Also percent greeks assume a change of variable to percent returns by writing:
    /// V(x = x0 * (1 + stock_percent_return / 100))
    /// or V(I = I0 * (1 + index_percent_return / 100))
    #[allow(clippy::too_many_arguments)]
    pub fn modify_greeks(
        &self,
        delta_input: f64,
        gamma_input: f64,
        underlying_instrument_id: InstrumentId,
        underlying_price: f64,
        unshocked_underlying_price: f64,
        percent_greeks: bool,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<&HashMap<InstrumentId, f64>>,
    ) -> (f64, f64) {
        let mut delta = delta_input;
        let mut gamma = gamma_input;

        let mut index_price = None;

        if let Some(index_id) = index_instrument_id {
            let cache = self.cache.borrow();
            index_price = Some(
                cache
                    .price(&index_id, PriceType::Last)
                    .unwrap_or_default()
                    .as_f64(),
            );

            let mut beta = 1.0;
            if let Some(weights) = beta_weights {
                if let Some(&weight) = weights.get(&underlying_instrument_id) {
                    beta = weight;
                }
            }

            if let Some(ref mut idx_price) = index_price {
                if underlying_price != unshocked_underlying_price {
                    *idx_price += 1.0 / beta
                        * (*idx_price / unshocked_underlying_price)
                        * (underlying_price - unshocked_underlying_price);
                }

                let delta_multiplier = beta * underlying_price / *idx_price;
                delta *= delta_multiplier;
                gamma *= delta_multiplier.powi(2);
            }
        }

        if percent_greeks {
            if let Some(idx_price) = index_price {
                delta *= idx_price / 100.0;
                gamma *= (idx_price / 100.0).powi(2);
            } else {
                delta *= underlying_price / 100.0;
                gamma *= (underlying_price / 100.0).powi(2);
            }
        }

        (delta, gamma)
    }

    /// Calculates the portfolio Greeks for a given set of positions.
    ///
    /// Aggregates the Greeks data for all open positions that match the specified criteria.
    ///
    /// Additional features:
    /// - Apply shocks to the spot value of an instrument's underlying, implied volatility or time to expiry.
    /// - Compute percent greeks.
    /// - Compute beta-weighted delta and gamma with respect to an index.
    #[allow(clippy::too_many_arguments)]
    pub fn portfolio_greeks(
        &self,
        underlyings: Option<Vec<String>>,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<PositionSide>,
        flat_interest_rate: Option<f64>,
        flat_dividend_yield: Option<f64>,
        spot_shock: Option<f64>,
        vol_shock: Option<f64>,
        time_to_expiry_shock: Option<f64>,
        use_cached_greeks: Option<bool>,
        cache_greeks: Option<bool>,
        publish_greeks: Option<bool>,
        percent_greeks: Option<bool>,
        index_instrument_id: Option<InstrumentId>,
        beta_weights: Option<HashMap<InstrumentId, f64>>,
    ) -> anyhow::Result<PortfolioGreeks> {
        let ts_event = self.clock.borrow().timestamp_ns();
        let mut portfolio_greeks =
            PortfolioGreeks::new(ts_event, ts_event, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        // Set default values
        let flat_interest_rate = flat_interest_rate.unwrap_or(0.0425);
        let spot_shock = spot_shock.unwrap_or(0.0);
        let vol_shock = vol_shock.unwrap_or(0.0);
        let time_to_expiry_shock = time_to_expiry_shock.unwrap_or(0.0);
        let use_cached_greeks = use_cached_greeks.unwrap_or(false);
        let cache_greeks = cache_greeks.unwrap_or(false);
        let publish_greeks = publish_greeks.unwrap_or(false);
        let percent_greeks = percent_greeks.unwrap_or(false);
        let side = side.unwrap_or(PositionSide::NoPositionSide);

        let cache = self.cache.borrow();
        let open_positions = cache.positions(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            Some(side),
        );
        let open_positions: Vec<Position> = open_positions.iter().map(|&p| p.clone()).collect();

        for position in open_positions {
            let position_instrument_id = position.instrument_id;

            if let Some(ref underlyings_list) = underlyings {
                let mut skip_position = true;

                for underlying in underlyings_list {
                    if position_instrument_id
                        .symbol
                        .as_str()
                        .starts_with(underlying)
                    {
                        skip_position = false;
                        break;
                    }
                }

                if skip_position {
                    continue;
                }
            }

            let quantity = position.signed_qty;
            let instrument_greeks = self.instrument_greeks(
                position_instrument_id,
                Some(flat_interest_rate),
                flat_dividend_yield,
                Some(spot_shock),
                Some(vol_shock),
                Some(time_to_expiry_shock),
                Some(use_cached_greeks),
                Some(cache_greeks),
                Some(publish_greeks),
                Some(ts_event),
                Some(position),
                Some(percent_greeks),
                index_instrument_id,
                beta_weights.clone(),
            )?;
            portfolio_greeks = portfolio_greeks + (quantity * &instrument_greeks).into();
        }

        Ok(portfolio_greeks)
    }

    /// Subscribes to Greeks data for a given underlying instrument.
    ///
    /// Useful for reading greeks from a backtesting data catalog and caching them for later use.
    pub fn subscribe_greeks<F>(&self, underlying: &str, handler: Option<F>)
    where
        F: Fn(GreeksData) + 'static + Send + Sync,
    {
        let topic_str = format!("data.GreeksData.instrument_id={}*", underlying);
        let topic = Ustr::from(topic_str.as_str());

        if let Some(custom_handler) = handler {
            let handler = msgbus::handler::TypedMessageHandler::with_any(
                move |greeks: &dyn std::any::Any| {
                    if let Some(greeks_data) = greeks.downcast_ref::<GreeksData>() {
                        custom_handler(greeks_data.clone());
                    }
                },
            );
            msgbus::subscribe(
                topic.as_str(),
                msgbus::handler::ShareableMessageHandler(Rc::new(handler)),
                None,
            );
        } else {
            let cache_ref = self.cache.clone();
            let default_handler = msgbus::handler::TypedMessageHandler::with_any(
                move |greeks: &dyn std::any::Any| {
                    if let Some(greeks_data) = greeks.downcast_ref::<GreeksData>() {
                        let mut cache = cache_ref.borrow_mut();
                        cache.add_greeks(greeks_data.clone()).unwrap_or_default();
                    }
                },
            );
            msgbus::subscribe(
                topic.as_str(),
                msgbus::handler::ShareableMessageHandler(Rc::new(default_handler)),
                None,
            );
        }
    }
}
