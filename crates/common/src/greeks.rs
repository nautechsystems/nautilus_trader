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

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use derive_builder::Builder;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::greeks::{GreeksData, PortfolioGreeks, black_scholes_greeks, imply_vol_and_greeks},
    enums::{InstrumentClass, OptionKind, PositionSide, PriceType},
    identifiers::{InstrumentId, StrategyId, Venue},
    instruments::Instrument,
    position::Position,
};

use crate::{cache::Cache, clock::Clock, msgbus};

/// Type alias for a greeks filter function.
pub type GreeksFilter = Box<dyn Fn(&GreeksData) -> bool>;

/// Cloneable wrapper for greeks filter functions.
#[derive(Clone)]
pub enum GreeksFilterCallback {
    /// Function pointer (non-capturing closure)
    Function(fn(&GreeksData) -> bool),
    /// Boxed closure (may capture variables)
    Closure(std::rc::Rc<dyn Fn(&GreeksData) -> bool>),
}

impl GreeksFilterCallback {
    /// Create a new filter from a function pointer.
    pub fn from_fn(f: fn(&GreeksData) -> bool) -> Self {
        Self::Function(f)
    }

    /// Create a new filter from a closure.
    pub fn from_closure<F>(f: F) -> Self
    where
        F: Fn(&GreeksData) -> bool + 'static,
    {
        Self::Closure(std::rc::Rc::new(f))
    }

    /// Call the filter function.
    pub fn call(&self, data: &GreeksData) -> bool {
        match self {
            Self::Function(f) => f(data),
            Self::Closure(f) => f(data),
        }
    }

    /// Convert to the original GreeksFilter type.
    pub fn to_greeks_filter(self) -> GreeksFilter {
        match self {
            Self::Function(f) => Box::new(f),
            Self::Closure(f) => {
                let f_clone = f.clone();
                Box::new(move |data| f_clone(data))
            }
        }
    }
}

impl Debug for GreeksFilterCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function(_) => f.write_str("GreeksFilterCallback::Function"),
            Self::Closure(_) => f.write_str("GreeksFilterCallback::Closure"),
        }
    }
}

/// Builder for instrument greeks calculation parameters.
#[derive(Debug, Builder)]
#[builder(setter(into), derive(Debug))]
pub struct InstrumentGreeksParams {
    /// The instrument ID to calculate greeks for
    pub instrument_id: InstrumentId,
    /// Flat interest rate (default: 0.0425)
    #[builder(default = "0.0425")]
    pub flat_interest_rate: f64,
    /// Flat dividend yield
    #[builder(default)]
    pub flat_dividend_yield: Option<f64>,
    /// Spot price shock (default: 0.0)
    #[builder(default = "0.0")]
    pub spot_shock: f64,
    /// Volatility shock (default: 0.0)
    #[builder(default = "0.0")]
    pub vol_shock: f64,
    /// Time to expiry shock (default: 0.0)
    #[builder(default = "0.0")]
    pub time_to_expiry_shock: f64,
    /// Whether to use cached greeks (default: false)
    #[builder(default = "false")]
    pub use_cached_greeks: bool,
    /// Whether to cache greeks (default: false)
    #[builder(default = "false")]
    pub cache_greeks: bool,
    /// Whether to publish greeks (default: false)
    #[builder(default = "false")]
    pub publish_greeks: bool,
    /// Event timestamp
    #[builder(default)]
    pub ts_event: Option<UnixNanos>,
    /// Position for PnL calculation
    #[builder(default)]
    pub position: Option<Position>,
    /// Whether to compute percent greeks (default: false)
    #[builder(default = "false")]
    pub percent_greeks: bool,
    /// Index instrument ID for beta weighting
    #[builder(default)]
    pub index_instrument_id: Option<InstrumentId>,
    /// Beta weights for portfolio calculations
    #[builder(default)]
    pub beta_weights: Option<HashMap<InstrumentId, f64>>,
    /// Base value in days for time-weighting vega
    #[builder(default)]
    pub vega_time_weight_base: Option<i32>,
}

impl InstrumentGreeksParams {
    /// Calculate instrument greeks using the builder parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the greeks calculation fails.
    pub fn calculate(&self, calculator: &GreeksCalculator) -> anyhow::Result<GreeksData> {
        calculator.instrument_greeks(
            self.instrument_id,
            Some(self.flat_interest_rate),
            self.flat_dividend_yield,
            Some(self.spot_shock),
            Some(self.vol_shock),
            Some(self.time_to_expiry_shock),
            Some(self.use_cached_greeks),
            Some(self.cache_greeks),
            Some(self.publish_greeks),
            self.ts_event,
            self.position.clone(),
            Some(self.percent_greeks),
            self.index_instrument_id,
            self.beta_weights.clone(),
            self.vega_time_weight_base,
        )
    }
}

/// Builder for portfolio greeks calculation parameters.
#[derive(Builder)]
#[builder(setter(into))]
pub struct PortfolioGreeksParams {
    /// List of underlying symbols to filter by
    #[builder(default)]
    pub underlyings: Option<Vec<String>>,
    /// Venue to filter positions by
    #[builder(default)]
    pub venue: Option<Venue>,
    /// Instrument ID to filter positions by
    #[builder(default)]
    pub instrument_id: Option<InstrumentId>,
    /// Strategy ID to filter positions by
    #[builder(default)]
    pub strategy_id: Option<StrategyId>,
    /// Position side to filter by (default: NoPositionSide)
    #[builder(default)]
    pub side: Option<PositionSide>,
    /// Flat interest rate (default: 0.0425)
    #[builder(default = "0.0425")]
    pub flat_interest_rate: f64,
    /// Flat dividend yield
    #[builder(default)]
    pub flat_dividend_yield: Option<f64>,
    /// Spot price shock (default: 0.0)
    #[builder(default = "0.0")]
    pub spot_shock: f64,
    /// Volatility shock (default: 0.0)
    #[builder(default = "0.0")]
    pub vol_shock: f64,
    /// Time to expiry shock (default: 0.0)
    #[builder(default = "0.0")]
    pub time_to_expiry_shock: f64,
    /// Whether to use cached greeks (default: false)
    #[builder(default = "false")]
    pub use_cached_greeks: bool,
    /// Whether to cache greeks (default: false)
    #[builder(default = "false")]
    pub cache_greeks: bool,
    /// Whether to publish greeks (default: false)
    #[builder(default = "false")]
    pub publish_greeks: bool,
    /// Whether to compute percent greeks (default: false)
    #[builder(default = "false")]
    pub percent_greeks: bool,
    /// Index instrument ID for beta weighting
    #[builder(default)]
    pub index_instrument_id: Option<InstrumentId>,
    /// Beta weights for portfolio calculations
    #[builder(default)]
    pub beta_weights: Option<HashMap<InstrumentId, f64>>,
    /// Filter function for greeks
    #[builder(default)]
    pub greeks_filter: Option<GreeksFilterCallback>,
    /// Base value in days for time-weighting vega
    #[builder(default)]
    pub vega_time_weight_base: Option<i32>,
}

impl std::fmt::Debug for PortfolioGreeksParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PortfolioGreeksParams))
            .field("underlyings", &self.underlyings)
            .field("venue", &self.venue)
            .field("instrument_id", &self.instrument_id)
            .field("strategy_id", &self.strategy_id)
            .field("side", &self.side)
            .field("flat_interest_rate", &self.flat_interest_rate)
            .field("flat_dividend_yield", &self.flat_dividend_yield)
            .field("spot_shock", &self.spot_shock)
            .field("vol_shock", &self.vol_shock)
            .field("time_to_expiry_shock", &self.time_to_expiry_shock)
            .field("use_cached_greeks", &self.use_cached_greeks)
            .field("cache_greeks", &self.cache_greeks)
            .field("publish_greeks", &self.publish_greeks)
            .field("percent_greeks", &self.percent_greeks)
            .field("index_instrument_id", &self.index_instrument_id)
            .field("beta_weights", &self.beta_weights)
            .field("greeks_filter", &self.greeks_filter)
            .finish()
    }
}

impl PortfolioGreeksParams {
    /// Calculate portfolio greeks using the builder parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the portfolio greeks calculation fails.
    pub fn calculate(&self, calculator: &GreeksCalculator) -> anyhow::Result<PortfolioGreeks> {
        let greeks_filter = self
            .greeks_filter
            .as_ref()
            .map(|f| f.clone().to_greeks_filter());

        calculator.portfolio_greeks(
            self.underlyings.clone(),
            self.venue,
            self.instrument_id,
            self.strategy_id,
            self.side,
            Some(self.flat_interest_rate),
            self.flat_dividend_yield,
            Some(self.spot_shock),
            Some(self.vol_shock),
            Some(self.time_to_expiry_shock),
            Some(self.use_cached_greeks),
            Some(self.cache_greeks),
            Some(self.publish_greeks),
            Some(self.percent_greeks),
            self.index_instrument_id,
            self.beta_weights.clone(),
            greeks_filter,
            self.vega_time_weight_base,
        )
    }
}

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
#[derive(Debug)]
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
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument definition is not found or greeks calculation fails.
    ///
    /// # Panics
    ///
    /// Panics if the instrument has no underlying identifier.
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
        vega_time_weight_base: Option<i32>,
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
            let (delta, _, _) = self.modify_greeks(
                multiplier.as_f64(),
                0.0,
                underlying_instrument_id,
                underlying_price + spot_shock,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
                0.0,
                0.0,
                0,
                None,
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
        if use_cached_greeks && let Some(cached_greeks) = cache.greeks(&instrument_id) {
            greeks_data = Some(cached_greeks);
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
            let expiry_in_days = (expiry_utc - utc_now).num_days().min(1) as i32;
            let expiry_in_years = expiry_in_days as f64 / 365.25;
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
            let (delta, gamma, vega) = self.modify_greeks(
                greeks.delta,
                greeks.gamma,
                underlying_instrument_id,
                underlying_price,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
                greeks.vega,
                greeks.vol,
                expiry_in_days,
                vega_time_weight_base,
            );
            greeks_data = Some(GreeksData::new(
                utc_now_ns,
                utc_now_ns,
                instrument_id,
                is_call,
                strike,
                expiry_int,
                expiry_in_days,
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
                vega,
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
                let topic = format!(
                    "data.GreeksData.instrument_id={}",
                    instrument_id.symbol.as_str()
                )
                .into();
                msgbus::publish(topic, &greeks_data.clone().unwrap());
            }
        }

        let mut greeks_data = greeks_data.unwrap();

        if spot_shock != 0.0 || vol_shock != 0.0 || time_to_expiry_shock != 0.0 {
            let underlying_price = greeks_data.underlying_price;
            let shocked_underlying_price = underlying_price + spot_shock;
            let shocked_vol = greeks_data.vol + vol_shock;
            let shocked_time_to_expiry = greeks_data.expiry_in_years - time_to_expiry_shock;
            let shocked_expiry_in_days = (shocked_time_to_expiry * 365.25) as i32;

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
            let (delta, gamma, vega) = self.modify_greeks(
                greeks.delta,
                greeks.gamma,
                underlying_instrument_id,
                shocked_underlying_price,
                underlying_price,
                percent_greeks,
                index_instrument_id,
                beta_weights.as_ref(),
                greeks.vega,
                shocked_vol,
                shocked_expiry_in_days,
                vega_time_weight_base,
            );
            greeks_data = GreeksData::new(
                greeks_data.ts_event,
                greeks_data.ts_event,
                greeks_data.instrument_id,
                greeks_data.is_call,
                greeks_data.strike,
                greeks_data.expiry,
                shocked_expiry_in_days,
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
                vega,
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
        vega_input: f64,
        vol: f64,
        expiry_in_days: i32,
        vega_time_weight_base: Option<i32>,
    ) -> (f64, f64, f64) {
        let mut delta = delta_input;
        let mut gamma = gamma_input;
        let mut vega = vega_input;

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
            if let Some(weights) = beta_weights
                && let Some(&weight) = weights.get(&underlying_instrument_id)
            {
                beta = weight;
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

            // Apply percent vega when percent_greeks is True
            vega *= vol / 100.0;
        }

        // Apply time weighting to vega if vega_time_weight_base is provided
        if let Some(time_base) = vega_time_weight_base
            && expiry_in_days > 0
        {
            let time_weight = (time_base as f64 / expiry_in_days as f64).sqrt();
            vega *= time_weight;
        }

        (delta, gamma, vega)
    }

    /// Calculates the portfolio Greeks for a given set of positions.
    ///
    /// Aggregates the Greeks data for all open positions that match the specified criteria.
    ///
    /// Additional features:
    /// - Apply shocks to the spot value of an instrument's underlying, implied volatility or time to expiry.
    /// - Compute percent greeks.
    /// - Compute beta-weighted delta and gamma with respect to an index.
    ///
    /// # Errors
    ///
    /// Returns an error if any underlying greeks calculation fails.
    ///
    /// # Panics
    ///
    /// Panics if `greeks_filter` is `Some` but the filter function panics when called.
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
        greeks_filter: Option<GreeksFilter>,
        vega_time_weight_base: Option<i32>,
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
                vega_time_weight_base,
            )?;
            let position_greeks = (quantity * &instrument_greeks).into();

            // Apply greeks filter if provided
            if greeks_filter.is_none() || greeks_filter.as_ref().unwrap()(&instrument_greeks) {
                portfolio_greeks = portfolio_greeks + position_greeks;
            }
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
        let pattern = format!("data.GreeksData.instrument_id={underlying}*").into();

        if let Some(custom_handler) = handler {
            let handler = msgbus::handler::TypedMessageHandler::with_any(
                move |greeks: &dyn std::any::Any| {
                    if let Some(greeks_data) = greeks.downcast_ref::<GreeksData>() {
                        custom_handler(greeks_data.clone());
                    }
                },
            );
            msgbus::subscribe(
                pattern,
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
                pattern,
                msgbus::handler::ShareableMessageHandler(Rc::new(default_handler)),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use nautilus_model::{
        enums::PositionSide,
        identifiers::{InstrumentId, StrategyId, Venue},
    };
    use rstest::rstest;

    use super::*;
    use crate::{cache::Cache, clock::TestClock};

    fn create_test_calculator() -> GreeksCalculator {
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        GreeksCalculator::new(cache, clock)
    }

    #[rstest]
    fn test_greeks_calculator_creation() {
        let calculator = create_test_calculator();
        // Test that the calculator can be created
        assert!(format!("{calculator:?}").contains("GreeksCalculator"));
    }

    #[rstest]
    fn test_greeks_calculator_debug() {
        let calculator = create_test_calculator();
        // Test the debug representation
        let debug_str = format!("{calculator:?}");
        assert!(debug_str.contains("GreeksCalculator"));
    }

    #[rstest]
    fn test_greeks_calculator_has_python_bindings() {
        // This test just verifies that the GreeksCalculator struct
        // can be compiled with Python bindings enabled
        let calculator = create_test_calculator();
        // The Python methods are only accessible from Python,
        // but we can verify the struct compiles correctly
        assert!(format!("{calculator:?}").contains("GreeksCalculator"));
    }

    #[rstest]
    fn test_instrument_greeks_params_builder_default() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");

        let params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        assert_eq!(params.instrument_id, instrument_id);
        assert_eq!(params.flat_interest_rate, 0.0425);
        assert_eq!(params.flat_dividend_yield, None);
        assert_eq!(params.spot_shock, 0.0);
        assert_eq!(params.vol_shock, 0.0);
        assert_eq!(params.time_to_expiry_shock, 0.0);
        assert!(!params.use_cached_greeks);
        assert!(!params.cache_greeks);
        assert!(!params.publish_greeks);
        assert_eq!(params.ts_event, None);
        assert_eq!(params.position, None);
        assert!(!params.percent_greeks);
        assert_eq!(params.index_instrument_id, None);
        assert_eq!(params.beta_weights, None);
    }

    #[rstest]
    fn test_instrument_greeks_params_builder_custom_values() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let index_id = InstrumentId::from("SPY.NASDAQ");
        let mut beta_weights = HashMap::new();
        beta_weights.insert(instrument_id, 1.2);

        let params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .flat_interest_rate(0.05)
            .flat_dividend_yield(Some(0.02))
            .spot_shock(0.01)
            .vol_shock(0.05)
            .time_to_expiry_shock(0.1)
            .use_cached_greeks(true)
            .cache_greeks(true)
            .publish_greeks(true)
            .percent_greeks(true)
            .index_instrument_id(Some(index_id))
            .beta_weights(Some(beta_weights.clone()))
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        assert_eq!(params.instrument_id, instrument_id);
        assert_eq!(params.flat_interest_rate, 0.05);
        assert_eq!(params.flat_dividend_yield, Some(0.02));
        assert_eq!(params.spot_shock, 0.01);
        assert_eq!(params.vol_shock, 0.05);
        assert_eq!(params.time_to_expiry_shock, 0.1);
        assert!(params.use_cached_greeks);
        assert!(params.cache_greeks);
        assert!(params.publish_greeks);
        assert!(params.percent_greeks);
        assert_eq!(params.index_instrument_id, Some(index_id));
        assert_eq!(params.beta_weights, Some(beta_weights));
    }

    #[rstest]
    fn test_instrument_greeks_params_debug() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");

        let params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        let debug_str = format!("{params:?}");
        assert!(debug_str.contains("InstrumentGreeksParams"));
        assert!(debug_str.contains("AAPL.NASDAQ"));
    }

    #[rstest]
    fn test_portfolio_greeks_params_builder_default() {
        let params = PortfolioGreeksParamsBuilder::default()
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(params.underlyings, None);
        assert_eq!(params.venue, None);
        assert_eq!(params.instrument_id, None);
        assert_eq!(params.strategy_id, None);
        assert_eq!(params.side, None);
        assert_eq!(params.flat_interest_rate, 0.0425);
        assert_eq!(params.flat_dividend_yield, None);
        assert_eq!(params.spot_shock, 0.0);
        assert_eq!(params.vol_shock, 0.0);
        assert_eq!(params.time_to_expiry_shock, 0.0);
        assert!(!params.use_cached_greeks);
        assert!(!params.cache_greeks);
        assert!(!params.publish_greeks);
        assert!(!params.percent_greeks);
        assert_eq!(params.index_instrument_id, None);
        assert_eq!(params.beta_weights, None);
    }

    #[rstest]
    fn test_portfolio_greeks_params_builder_custom_values() {
        let venue = Venue::from("NASDAQ");
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let strategy_id = StrategyId::from("test-strategy");
        let index_id = InstrumentId::from("SPY.NASDAQ");
        let underlyings = vec!["AAPL".to_string(), "MSFT".to_string()];
        let mut beta_weights = HashMap::new();
        beta_weights.insert(instrument_id, 1.2);

        let params = PortfolioGreeksParamsBuilder::default()
            .underlyings(Some(underlyings.clone()))
            .venue(Some(venue))
            .instrument_id(Some(instrument_id))
            .strategy_id(Some(strategy_id))
            .side(Some(PositionSide::Long))
            .flat_interest_rate(0.05)
            .flat_dividend_yield(Some(0.02))
            .spot_shock(0.01)
            .vol_shock(0.05)
            .time_to_expiry_shock(0.1)
            .use_cached_greeks(true)
            .cache_greeks(true)
            .publish_greeks(true)
            .percent_greeks(true)
            .index_instrument_id(Some(index_id))
            .beta_weights(Some(beta_weights.clone()))
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(params.underlyings, Some(underlyings));
        assert_eq!(params.venue, Some(venue));
        assert_eq!(params.instrument_id, Some(instrument_id));
        assert_eq!(params.strategy_id, Some(strategy_id));
        assert_eq!(params.side, Some(PositionSide::Long));
        assert_eq!(params.flat_interest_rate, 0.05);
        assert_eq!(params.flat_dividend_yield, Some(0.02));
        assert_eq!(params.spot_shock, 0.01);
        assert_eq!(params.vol_shock, 0.05);
        assert_eq!(params.time_to_expiry_shock, 0.1);
        assert!(params.use_cached_greeks);
        assert!(params.cache_greeks);
        assert!(params.publish_greeks);
        assert!(params.percent_greeks);
        assert_eq!(params.index_instrument_id, Some(index_id));
        assert_eq!(params.beta_weights, Some(beta_weights));
    }

    #[rstest]
    fn test_portfolio_greeks_params_debug() {
        let venue = Venue::from("NASDAQ");

        let params = PortfolioGreeksParamsBuilder::default()
            .venue(Some(venue))
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        let debug_str = format!("{params:?}");
        assert!(debug_str.contains("PortfolioGreeksParams"));
        assert!(debug_str.contains("NASDAQ"));
    }

    #[rstest]
    fn test_instrument_greeks_params_builder_missing_required_field() {
        // Test that building without required instrument_id fails
        let result = InstrumentGreeksParamsBuilder::default().build();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_portfolio_greeks_params_builder_fluent_api() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");

        let params = PortfolioGreeksParamsBuilder::default()
            .instrument_id(Some(instrument_id))
            .flat_interest_rate(0.05)
            .spot_shock(0.01)
            .percent_greeks(true)
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(params.instrument_id, Some(instrument_id));
        assert_eq!(params.flat_interest_rate, 0.05);
        assert_eq!(params.spot_shock, 0.01);
        assert!(params.percent_greeks);
    }

    #[rstest]
    fn test_instrument_greeks_params_builder_fluent_chaining() {
        let instrument_id = InstrumentId::from("TSLA.NASDAQ");

        // Test fluent API chaining
        let params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .flat_interest_rate(0.03)
            .spot_shock(0.02)
            .vol_shock(0.1)
            .use_cached_greeks(true)
            .percent_greeks(true)
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        assert_eq!(params.instrument_id, instrument_id);
        assert_eq!(params.flat_interest_rate, 0.03);
        assert_eq!(params.spot_shock, 0.02);
        assert_eq!(params.vol_shock, 0.1);
        assert!(params.use_cached_greeks);
        assert!(params.percent_greeks);
    }

    #[rstest]
    fn test_portfolio_greeks_params_builder_with_underlyings() {
        let underlyings = vec!["AAPL".to_string(), "MSFT".to_string(), "GOOGL".to_string()];

        let params = PortfolioGreeksParamsBuilder::default()
            .underlyings(Some(underlyings.clone()))
            .flat_interest_rate(0.04)
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(params.underlyings, Some(underlyings));
        assert_eq!(params.flat_interest_rate, 0.04);
    }

    #[rstest]
    fn test_builders_with_empty_beta_weights() {
        let instrument_id = InstrumentId::from("NVDA.NASDAQ");
        let empty_beta_weights = HashMap::new();

        let instrument_params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .beta_weights(Some(empty_beta_weights.clone()))
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        let portfolio_params = PortfolioGreeksParamsBuilder::default()
            .beta_weights(Some(empty_beta_weights.clone()))
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(
            instrument_params.beta_weights,
            Some(empty_beta_weights.clone())
        );
        assert_eq!(portfolio_params.beta_weights, Some(empty_beta_weights));
    }

    #[rstest]
    fn test_builders_with_all_shocks() {
        let instrument_id = InstrumentId::from("AMD.NASDAQ");

        let instrument_params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .spot_shock(0.05)
            .vol_shock(0.1)
            .time_to_expiry_shock(0.01)
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        let portfolio_params = PortfolioGreeksParamsBuilder::default()
            .spot_shock(0.05)
            .vol_shock(0.1)
            .time_to_expiry_shock(0.01)
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert_eq!(instrument_params.spot_shock, 0.05);
        assert_eq!(instrument_params.vol_shock, 0.1);
        assert_eq!(instrument_params.time_to_expiry_shock, 0.01);

        assert_eq!(portfolio_params.spot_shock, 0.05);
        assert_eq!(portfolio_params.vol_shock, 0.1);
        assert_eq!(portfolio_params.time_to_expiry_shock, 0.01);
    }

    #[rstest]
    fn test_builders_with_all_boolean_flags() {
        let instrument_id = InstrumentId::from("META.NASDAQ");

        let instrument_params = InstrumentGreeksParamsBuilder::default()
            .instrument_id(instrument_id)
            .use_cached_greeks(true)
            .cache_greeks(true)
            .publish_greeks(true)
            .percent_greeks(true)
            .build()
            .expect("Failed to build InstrumentGreeksParams");

        let portfolio_params = PortfolioGreeksParamsBuilder::default()
            .use_cached_greeks(true)
            .cache_greeks(true)
            .publish_greeks(true)
            .percent_greeks(true)
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert!(instrument_params.use_cached_greeks);
        assert!(instrument_params.cache_greeks);
        assert!(instrument_params.publish_greeks);
        assert!(instrument_params.percent_greeks);

        assert!(portfolio_params.use_cached_greeks);
        assert!(portfolio_params.cache_greeks);
        assert!(portfolio_params.publish_greeks);
        assert!(portfolio_params.percent_greeks);
    }

    #[rstest]
    fn test_greeks_filter_callback_function() {
        // Test function pointer filter
        fn filter_positive_delta(data: &GreeksData) -> bool {
            data.delta > 0.0
        }

        let filter = GreeksFilterCallback::from_fn(filter_positive_delta);

        // Create test data
        let greeks_data = GreeksData::from_delta(
            InstrumentId::from("TEST.NASDAQ"),
            0.5,
            1.0,
            UnixNanos::default(),
        );

        assert!(filter.call(&greeks_data));

        // Test debug formatting
        let debug_str = format!("{filter:?}");
        assert!(debug_str.contains("GreeksFilterCallback::Function"));
    }

    #[rstest]
    fn test_greeks_filter_callback_closure() {
        // Test closure filter that captures a variable
        let min_delta = 0.3;
        let filter =
            GreeksFilterCallback::from_closure(move |data: &GreeksData| data.delta > min_delta);

        // Create test data
        let greeks_data = GreeksData::from_delta(
            InstrumentId::from("TEST.NASDAQ"),
            0.5,
            1.0,
            UnixNanos::default(),
        );

        assert!(filter.call(&greeks_data));

        // Test debug formatting
        let debug_str = format!("{filter:?}");
        assert!(debug_str.contains("GreeksFilterCallback::Closure"));
    }

    #[rstest]
    fn test_greeks_filter_callback_clone() {
        fn filter_fn(data: &GreeksData) -> bool {
            data.delta > 0.0
        }

        let filter1 = GreeksFilterCallback::from_fn(filter_fn);
        let filter2 = filter1.clone();

        let greeks_data = GreeksData::from_delta(
            InstrumentId::from("TEST.NASDAQ"),
            0.5,
            1.0,
            UnixNanos::default(),
        );

        assert!(filter1.call(&greeks_data));
        assert!(filter2.call(&greeks_data));
    }

    #[rstest]
    fn test_portfolio_greeks_params_with_filter() {
        fn filter_high_delta(data: &GreeksData) -> bool {
            data.delta.abs() > 0.1
        }

        let filter = GreeksFilterCallback::from_fn(filter_high_delta);

        let params = PortfolioGreeksParamsBuilder::default()
            .greeks_filter(Some(filter))
            .flat_interest_rate(0.05)
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert!(params.greeks_filter.is_some());
        assert_eq!(params.flat_interest_rate, 0.05);

        // Test that the filter can be called
        let greeks_data = GreeksData::from_delta(
            InstrumentId::from("TEST.NASDAQ"),
            0.5,
            1.0,
            UnixNanos::default(),
        );

        let filter_ref = params.greeks_filter.as_ref().unwrap();
        assert!(filter_ref.call(&greeks_data));
    }

    #[rstest]
    fn test_portfolio_greeks_params_with_closure_filter() {
        let min_gamma = 0.01;
        let filter =
            GreeksFilterCallback::from_closure(move |data: &GreeksData| data.gamma > min_gamma);

        let params = PortfolioGreeksParamsBuilder::default()
            .greeks_filter(Some(filter))
            .build()
            .expect("Failed to build PortfolioGreeksParams");

        assert!(params.greeks_filter.is_some());

        // Test debug formatting includes the filter
        let debug_str = format!("{params:?}");
        assert!(debug_str.contains("greeks_filter"));
    }

    #[rstest]
    fn test_greeks_filter_to_greeks_filter_conversion() {
        fn filter_fn(data: &GreeksData) -> bool {
            data.delta > 0.0
        }

        let callback = GreeksFilterCallback::from_fn(filter_fn);
        let greeks_filter = callback.to_greeks_filter();

        let greeks_data = GreeksData::from_delta(
            InstrumentId::from("TEST.NASDAQ"),
            0.5,
            1.0,
            UnixNanos::default(),
        );

        assert!(greeks_filter(&greeks_data));
    }
}
