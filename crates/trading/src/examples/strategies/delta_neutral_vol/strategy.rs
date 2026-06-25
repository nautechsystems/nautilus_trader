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

//! Delta-neutral short volatility hedger implementation.

use std::fmt::Debug;

use anyhow::Context;
use nautilus_common::{actor::DataActor, timer::TimeEvent};
use nautilus_core::params::Params;
use nautilus_model::{
    data::{QuoteTick, black_scholes::compute_greeks, option_chain::OptionGreeks},
    enums::{OptionKind, OrderSide, TimeInForce},
    events::{OrderCanceled, OrderFilled},
    identifiers::{ClientId, InstrumentId},
    instruments::Instrument,
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::json;
use ustr::Ustr;

use super::config::DeltaNeutralVolConfig;
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

const REHEDGE_TIMER: &str = "delta_rehedge";

/// Delta-neutral short volatility hedger.
///
/// Tracks a short OTM call and put (strangle) on a configurable option
/// family and delta-hedges the net Greek exposure with the underlying
/// perpetual swap. Rehedges when portfolio delta exceeds a threshold
/// or on a periodic timer.
pub struct DeltaNeutralVol {
    pub(super) core: StrategyCore,
    pub(super) config: DeltaNeutralVolConfig,
    pub(super) call_instrument_id: Option<InstrumentId>,
    pub(super) put_instrument_id: Option<InstrumentId>,
    pub(super) subscribed_greeks: Vec<InstrumentId>,
    pub(super) call_delta: f64,
    pub(super) put_delta: f64,
    pub(super) call_mark_iv: Option<f64>,
    pub(super) put_mark_iv: Option<f64>,
    pub(super) call_quote: Option<QuoteTick>,
    pub(super) put_quote: Option<QuoteTick>,
    pub(super) call_greeks: Option<OptionGreeks>,
    pub(super) put_greeks: Option<OptionGreeks>,
    pub(super) call_delta_ready: bool,
    pub(super) put_delta_ready: bool,
    pub(super) call_position: f64,
    pub(super) put_position: f64,
    pub(super) hedge_position: f64,
    pub(super) hedge_pending: bool,
    pub(super) entry_attempted: bool,
}

impl DeltaNeutralVol {
    /// Creates a new [`DeltaNeutralVol`] instance from config.
    #[must_use]
    pub fn new(config: DeltaNeutralVolConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            call_instrument_id: None,
            put_instrument_id: None,
            subscribed_greeks: Vec::new(),
            call_delta: 0.0,
            put_delta: 0.0,
            call_mark_iv: None,
            put_mark_iv: None,
            call_quote: None,
            put_quote: None,
            call_greeks: None,
            put_greeks: None,
            call_delta_ready: false,
            put_delta_ready: false,
            call_position: 0.0,
            put_position: 0.0,
            hedge_position: 0.0,
            hedge_pending: false,
            entry_attempted: false,
            config,
        }
    }

    /// Computes the net portfolio delta across option legs and hedge position.
    #[must_use]
    pub fn portfolio_delta(&self) -> f64 {
        self.call_delta * self.call_position
            + self.put_delta * self.put_position
            + self.hedge_position
    }

    /// Returns `true` when both greeks legs have been initialized.
    #[must_use]
    pub fn greeks_initialized(&self) -> bool {
        self.call_instrument_id.is_some()
            && self.put_instrument_id.is_some()
            && self.call_delta_ready
            && self.put_delta_ready
    }

    /// Returns `true` when portfolio delta exceeds the rehedge threshold.
    #[must_use]
    pub fn should_rehedge(&self) -> bool {
        self.greeks_initialized()
            && self.portfolio_delta().abs() > self.config.rehedge_delta_threshold
    }

    /// Returns `true` when strangle entry can proceed.
    #[must_use]
    pub fn should_enter_strangle(&self) -> bool {
        self.config.enter_strangle
            && self.greeks_initialized()
            && self.entry_price_data_ready()
            && self.call_position == 0.0
            && self.put_position == 0.0
            && !self.entry_attempted
            && !self.has_working_entry_orders()
    }

    /// Returns `true` when the configured entry pricing mode has enough data.
    #[must_use]
    pub fn entry_price_data_ready(&self) -> bool {
        if self.config.entry_premium_offset_ticks.is_some() {
            let Some(call_id) = self.call_instrument_id else {
                return false;
            };
            let Some(put_id) = self.put_instrument_id else {
                return false;
            };

            return self.premium_entry_data_ready(call_id, self.call_quote, self.call_greeks)
                && self.premium_entry_data_ready(put_id, self.put_quote, self.put_greeks);
        }

        self.call_mark_iv.is_some() && self.put_mark_iv.is_some()
    }

    fn premium_entry_data_ready(
        &self,
        instrument_id: InstrumentId,
        quote: Option<QuoteTick>,
        greeks: Option<OptionGreeks>,
    ) -> bool {
        if quote.is_some_and(|q| q.ask_price.as_decimal() > Decimal::ZERO) {
            return true;
        }

        let Some(greeks) = greeks else {
            return false;
        };

        self.premium_from_greeks_ready(instrument_id, greeks)
    }

    fn premium_from_greeks_ready(&self, instrument_id: InstrumentId, greeks: OptionGreeks) -> bool {
        let Some(underlying_price) = greeks.underlying_price else {
            return false;
        };
        let Some(vol) = greeks.ask_iv.filter(|v| *v > 0.0).or(greeks.mark_iv) else {
            return false;
        };
        let has_option_terms = {
            let cache = self.cache();
            let Some(instrument) = cache.instrument(&instrument_id) else {
                return false;
            };

            instrument.strike_price().is_some()
                && instrument.expiration_ns().is_some()
                && instrument.option_kind().is_some()
        };

        underlying_price > 0.0 && vol > 0.0 && has_option_terms
    }

    /// Returns `true` when any open or in-flight orders exist on the option legs.
    #[must_use]
    pub fn has_working_entry_orders(&self) -> bool {
        let cache = self.cache();

        for id in [self.call_instrument_id, self.put_instrument_id]
            .into_iter()
            .flatten()
        {
            let open = cache.orders_open(None, Some(&id), None, None, None);
            let inflight = cache.orders_inflight(None, Some(&id), None, None, None);

            if !open.is_empty() || !inflight.is_empty() {
                return true;
            }
        }
        false
    }

    fn enter_strangle(&mut self) -> anyhow::Result<()> {
        if !self.should_enter_strangle() {
            return Ok(());
        }

        let call_id = self.call_instrument_id.unwrap();
        let put_id = self.put_instrument_id.unwrap();
        let contracts = self.config.contracts;
        let tif = self.config.entry_time_in_force;
        let client_id = self.config.client_id;

        if let Some(offset_ticks) = self.config.entry_premium_offset_ticks {
            let call_price =
                self.entry_premium_price(call_id, self.call_quote, self.call_greeks)?;
            let put_price = self.entry_premium_price(put_id, self.put_quote, self.put_greeks)?;

            log::info!(
                "Entering strangle: SELL {contracts} x {call_id} @ premium={call_price} \
                 + SELL {contracts} x {put_id} @ premium={put_price} \
                 (ask_offset_ticks={offset_ticks})",
            );

            self.submit_entry_order(call_id, contracts, call_price, tif, client_id, None)?;
            self.submit_entry_order(put_id, contracts, put_price, tif, client_id, None)?;
        } else {
            let call_iv = self.call_mark_iv.unwrap();
            let put_iv = self.put_mark_iv.unwrap();
            let offset = self.config.entry_iv_offset;
            let call_entry_iv = call_iv - offset;
            let put_entry_iv = put_iv - offset;

            log::info!(
                "Entering strangle: SELL {contracts} x {call_id} @ iv={call_entry_iv:.4} \
                 + SELL {contracts} x {put_id} @ iv={put_entry_iv:.4} (offset={offset})",
            );

            let mut call_params = Params::new();
            call_params.insert(
                self.config.iv_param_key.clone(),
                json!(call_entry_iv.to_string()),
            );

            self.submit_entry_order(
                call_id,
                contracts,
                Price::new(call_entry_iv, 4),
                tif,
                client_id,
                Some(call_params),
            )?;

            let mut put_params = Params::new();
            put_params.insert(
                self.config.iv_param_key.clone(),
                json!(put_entry_iv.to_string()),
            );

            self.submit_entry_order(
                put_id,
                contracts,
                Price::new(put_entry_iv, 4),
                tif,
                client_id,
                Some(put_params),
            )?;
        }

        self.entry_attempted = true;

        Ok(())
    }

    fn entry_premium_price(
        &self,
        instrument_id: InstrumentId,
        quote: Option<QuoteTick>,
        greeks: Option<OptionGreeks>,
    ) -> anyhow::Result<Price> {
        if let Some(quote) = quote
            && quote.ask_price.as_decimal() > Decimal::ZERO
        {
            return self.offset_entry_price(instrument_id, quote.ask_price.as_f64());
        }

        let greeks = greeks.with_context(|| {
            format!("missing quote and Greeks for premium entry on {instrument_id}")
        })?;
        let base_price = self.entry_premium_from_greeks(instrument_id, greeks)?;

        self.offset_entry_price(instrument_id, base_price)
    }

    fn offset_entry_price(
        &self,
        instrument_id: InstrumentId,
        base_price: f64,
    ) -> anyhow::Result<Price> {
        let offset_ticks = self
            .config
            .entry_premium_offset_ticks
            .context("missing premium entry offset")?;

        let cache = self.cache();
        let instrument = cache.try_instrument(&instrument_id)?;

        instrument
            .next_ask_price(base_price, offset_ticks)
            .with_context(|| {
                format!(
                    "failed to offset premium for {instrument_id}: price={base_price}, ticks={offset_ticks}"
                )
            })
    }

    fn entry_premium_from_greeks(
        &self,
        instrument_id: InstrumentId,
        greeks: OptionGreeks,
    ) -> anyhow::Result<f64> {
        let (strike, expiration_ns, is_call) = {
            let cache = self.cache();
            let instrument = cache.try_instrument(&instrument_id)?;
            let strike = instrument
                .strike_price()
                .with_context(|| format!("missing strike for {instrument_id}"))?
                .as_f64();
            let expiration_ns = instrument
                .expiration_ns()
                .with_context(|| format!("missing expiry for {instrument_id}"))?
                .as_u64();
            let option_kind = instrument
                .option_kind()
                .with_context(|| format!("missing option kind for {instrument_id}"))?;
            let is_call = matches!(option_kind, OptionKind::Call);

            (strike, expiration_ns, is_call)
        };
        let now_ns = self.clock().timestamp_ns().as_u64();

        if expiration_ns <= now_ns {
            anyhow::bail!("Cannot price premium entry for expired instrument {instrument_id}");
        }

        let underlying_price = greeks
            .underlying_price
            .with_context(|| format!("missing underlying price for {instrument_id}"))?;
        let (vol_source, vol) = greeks
            .ask_iv
            .filter(|v| *v > 0.0)
            .map(|v| ("ask_iv", v))
            .or_else(|| greeks.mark_iv.filter(|v| *v > 0.0).map(|v| ("mark_iv", v)))
            .with_context(|| format!("missing positive IV for {instrument_id}"))?;
        let years_to_expiry =
            (expiration_ns - now_ns) as f64 / 1_000_000_000.0 / (365.25 * 24.0 * 60.0 * 60.0);
        let price = compute_greeks(
            underlying_price as f32,
            strike as f32,
            years_to_expiry as f32,
            0.0,
            0.0,
            vol as f32,
            is_call,
        )
        .price as f64;

        if !price.is_finite() || price <= 0.0 {
            anyhow::bail!(
                "Computed non-positive premium for {instrument_id}: price={price}, \
                 underlying={underlying_price}, strike={strike}, {vol_source}={vol}"
            );
        }

        log::info!(
            "Premium quote unavailable for {instrument_id}; using {vol_source}={vol:.4}, \
             underlying={underlying_price:.2}, strike={strike:.2}, t={years_to_expiry:.6}"
        );

        Ok(price)
    }

    fn submit_entry_order(
        &mut self,
        instrument_id: InstrumentId,
        contracts: u64,
        price: Price,
        tif: TimeInForce,
        client_id: ClientId,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let order = self.order().limit(
            instrument_id,
            OrderSide::Sell,
            Quantity::new(contracts as f64, 0),
            price,
            Some(tif),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        self.submit_order(order, None, Some(client_id), params)
    }

    fn check_rehedge(&mut self) -> anyhow::Result<()> {
        let delta = self.portfolio_delta();

        if !self.should_rehedge() {
            return Ok(());
        }

        if self.hedge_pending {
            log::info!("Hedge order already pending, skipping rehedge");
            return Ok(());
        }

        let hedge_qty = delta.abs();
        let side = if delta > 0.0 {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        log::info!(
            "Rehedging: portfolio_delta={delta:.4}, submitting {side:?} {hedge_qty:.4} on {}",
            self.config.hedge_instrument_id,
        );

        let hedge_id = self.config.hedge_instrument_id;
        let size_precision = {
            let cache = self.cache();
            cache
                .instrument(&hedge_id)
                .map_or(2, |i| i.size_precision())
        };

        let order = self.order().market(
            hedge_id,
            side,
            Quantity::new(hedge_qty, size_precision),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        self.hedge_pending = true;

        if let Err(e) = self.submit_order(order, None, Some(self.config.client_id), None) {
            self.hedge_pending = false;
            return Err(e);
        }

        Ok(())
    }
}

nautilus_strategy!(DeltaNeutralVol);

impl Debug for DeltaNeutralVol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeltaNeutralVol))
            .field("config", &self.config)
            .field("call_instrument_id", &self.call_instrument_id)
            .field("put_instrument_id", &self.put_instrument_id)
            .field("call_delta", &self.call_delta)
            .field("put_delta", &self.put_delta)
            .field("portfolio_delta", &self.portfolio_delta())
            .finish()
    }
}

impl DataActor for DeltaNeutralVol {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venue = self.config.hedge_instrument_id.venue;
        let underlying = Ustr::from(&self.config.option_family);
        let now_ns = self.clock().timestamp_ns().as_u64();

        let mut calls: Vec<(InstrumentId, f64, u64)> = Vec::new();
        let mut puts: Vec<(InstrumentId, f64, u64)> = Vec::new();

        {
            let cache = self.cache();
            let instruments = cache.instruments(&venue, Some(&underlying));

            for inst in &instruments {
                let Some(expiry_ns) = inst.expiration_ns() else {
                    continue;
                };

                if expiry_ns.as_u64() <= now_ns {
                    continue;
                }

                if let Some(ref filter) = self.config.expiry_filter {
                    let symbol = inst.symbol().inner();
                    if !symbol.as_str().contains(filter.as_str()) {
                        continue;
                    }
                }

                let strike = match inst.strike_price() {
                    Some(p) => p.as_f64(),
                    None => continue,
                };

                match inst.option_kind() {
                    Some(OptionKind::Call) => {
                        calls.push((inst.id(), strike, expiry_ns.as_u64()));
                    }
                    Some(OptionKind::Put) => {
                        puts.push((inst.id(), strike, expiry_ns.as_u64()));
                    }
                    None => {}
                }
            }
        }

        if calls.is_empty() || puts.is_empty() {
            log::warn!(
                "Insufficient options found for family '{}': {} calls, {} puts",
                self.config.option_family,
                calls.len(),
                puts.len(),
            );
            return Ok(());
        }

        if self.config.expiry_filter.is_none() {
            let nearest = calls
                .iter()
                .chain(puts.iter())
                .map(|(_, _, exp)| *exp)
                .min()
                .unwrap();
            calls.retain(|(_, _, exp)| *exp == nearest);
            puts.retain(|(_, _, exp)| *exp == nearest);
        }

        if calls.is_empty() || puts.is_empty() {
            log::warn!(
                "Nearest expiry has incomplete chain: {} calls, {} puts",
                calls.len(),
                puts.len(),
            );
            return Ok(());
        }

        log::info!(
            "Found {} calls and {} puts for family '{}'",
            calls.len(),
            puts.len(),
            self.config.option_family,
        );

        // Strike price approximates delta ordering: higher strikes have
        // lower call delta, lower strikes have more negative put delta.
        // A production strategy would subscribe to all greeks first,
        // then select strikes once actual deltas arrive.
        calls.sort_by(|(_, s1, _), (_, s2, _)| s1.partial_cmp(s2).unwrap());
        puts.sort_by(|(_, s1, _), (_, s2, _)| s1.partial_cmp(s2).unwrap());

        // Select call at ~80th percentile strike (OTM, ~0.20 delta)
        let call_idx = ((1.0 - self.config.target_call_delta) * calls.len() as f64) as usize;
        let call_idx = call_idx.min(calls.len() - 1);
        let (call_id, call_strike, _) = calls[call_idx];

        // Select put at ~20th percentile strike (OTM, ~-0.20 delta)
        let put_idx = (self.config.target_put_delta.abs() * puts.len() as f64) as usize;
        let put_idx = put_idx.min(puts.len() - 1);
        let (put_id, put_strike, _) = puts[put_idx];

        self.call_instrument_id = Some(call_id);
        self.put_instrument_id = Some(put_id);

        log::info!("Selected call: {call_id} (strike={call_strike})");
        log::info!("Selected put: {put_id} (strike={put_strike})");
        log::info!(
            "Strangle: {} contracts per leg, hedge on {}",
            self.config.contracts,
            self.config.hedge_instrument_id,
        );

        let (cached_call_pos, cached_put_pos, cached_hedge_pos) = {
            let cache = self.cache();
            let hedge_id = self.config.hedge_instrument_id;

            let call_pos: f64 = cache
                .positions_open(None, Some(&call_id), None, None, None)
                .iter()
                .map(|p| p.signed_qty)
                .sum();

            let put_pos: f64 = cache
                .positions_open(None, Some(&put_id), None, None, None)
                .iter()
                .map(|p| p.signed_qty)
                .sum();

            let hedge_pos: f64 = cache
                .positions_open(None, Some(&hedge_id), None, None, None)
                .iter()
                .map(|p| p.signed_qty)
                .sum();

            (call_pos, put_pos, hedge_pos)
        };

        self.call_position = cached_call_pos;
        self.put_position = cached_put_pos;
        self.hedge_position = cached_hedge_pos;

        if self.call_position != 0.0 || self.put_position != 0.0 || self.hedge_position != 0.0 {
            log::info!(
                "Hydrated positions: call={}, put={}, hedge={}",
                self.call_position,
                self.put_position,
                self.hedge_position,
            );
        }

        let client_id = self.config.client_id;

        self.subscribe_option_greeks(call_id, Some(client_id), None);
        self.subscribed_greeks.push(call_id);

        self.subscribe_option_greeks(put_id, Some(client_id), None);
        self.subscribed_greeks.push(put_id);

        if self.config.enter_strangle && self.config.entry_premium_offset_ticks.is_some() {
            self.subscribe_quotes(call_id, Some(client_id), None);
            self.subscribe_quotes(put_id, Some(client_id), None);
        }

        self.subscribe_quotes(self.config.hedge_instrument_id, None, None);

        let interval_ns = self.config.rehedge_interval_secs * 1_000_000_000;
        self.clock()
            .set_timer_ns(REHEDGE_TIMER, interval_ns, None, None, None, None, None)?;

        log::info!(
            "Rehedge timer set: every {}s, threshold={}",
            self.config.rehedge_interval_secs,
            self.config.rehedge_delta_threshold,
        );

        if self.config.enter_strangle {
            if let Some(offset_ticks) = self.config.entry_premium_offset_ticks {
                log::info!(
                    "Strangle entry enabled: SELL {} x {call_id} (call) + SELL {} x {put_id} \
                     (put) once premium data arrives (ask_offset_ticks={offset_ticks})",
                    self.config.contracts,
                    self.config.contracts,
                );
            } else {
                log::info!(
                    "Strangle entry enabled: SELL {} x {call_id} (call) + SELL {} x {put_id} \
                     (put) once Greeks arrive (iv_offset={})",
                    self.config.contracts,
                    self.config.contracts,
                    self.config.entry_iv_offset,
                );
            }
        } else {
            log::info!(
                "Strangle entry disabled: hedging externally-held positions only. \
                 Monitoring {call_id} (call) + {put_id} (put)",
            );
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.clock().cancel_timer(REHEDGE_TIMER);

        let ids: Vec<InstrumentId> = self.subscribed_greeks.drain(..).collect();
        let client_id = self.config.client_id;

        for instrument_id in ids {
            self.unsubscribe_option_greeks(instrument_id, Some(client_id), None);
        }

        let premium_entry_active =
            self.config.enter_strangle && self.config.entry_premium_offset_ticks.is_some();

        if let Some(call_id) = self.call_instrument_id {
            if premium_entry_active {
                self.unsubscribe_quotes(call_id, Some(client_id), None);
            }
            self.cancel_all_orders(call_id, None, None, None)?;
        }

        if let Some(put_id) = self.put_instrument_id {
            if premium_entry_active {
                self.unsubscribe_quotes(put_id, Some(client_id), None);
            }
            self.cancel_all_orders(put_id, None, None, None)?;
        }

        let hedge_id = self.config.hedge_instrument_id;
        self.unsubscribe_quotes(hedge_id, None, None);
        self.cancel_all_orders(hedge_id, None, None, None)?;
        self.hedge_pending = false;

        log::info!("Delta-neutral vol strategy stopped, positions left unchanged");

        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        if Some(greeks.instrument_id) == self.call_instrument_id {
            self.call_greeks = Some(*greeks);
            self.call_delta = greeks.greeks.delta;
            self.call_delta_ready = true;

            if let Some(iv) = greeks.mark_iv {
                self.call_mark_iv = Some(iv);
            }
        } else if Some(greeks.instrument_id) == self.put_instrument_id {
            self.put_greeks = Some(*greeks);
            self.put_delta = greeks.greeks.delta;
            self.put_delta_ready = true;

            if let Some(iv) = greeks.mark_iv {
                self.put_mark_iv = Some(iv);
            }
        }

        let portfolio_delta = self.portfolio_delta();

        log::info!(
            "Greeks update: {} delta={:.4} | portfolio_delta={portfolio_delta:.4} \
             (call={:.4}*{}, put={:.4}*{}, hedge={})",
            greeks.instrument_id,
            greeks.greeks.delta,
            self.call_delta,
            self.call_position,
            self.put_delta,
            self.put_position,
            self.hedge_position,
        );

        self.enter_strangle()?;
        self.check_rehedge()?;

        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if Some(quote.instrument_id) == self.call_instrument_id {
            self.call_quote = Some(*quote);
            log::debug!(
                "Call quote: bid={} ask={} on {}",
                quote.bid_price,
                quote.ask_price,
                quote.instrument_id,
            );
            self.enter_strangle()?;
        } else if Some(quote.instrument_id) == self.put_instrument_id {
            self.put_quote = Some(*quote);
            log::debug!(
                "Put quote: bid={} ask={} on {}",
                quote.bid_price,
                quote.ask_price,
                quote.instrument_id,
            );
            self.enter_strangle()?;
        } else if quote.instrument_id == self.config.hedge_instrument_id {
            log::debug!(
                "Hedge quote: bid={} ask={} on {}",
                quote.bid_price,
                quote.ask_price,
                quote.instrument_id,
            );
        }

        Ok(())
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        let qty = event.last_qty.as_f64();
        let signed_qty = match event.order_side {
            OrderSide::Buy => qty,
            OrderSide::Sell => -qty,
            _ => 0.0,
        };

        if event.instrument_id == self.config.hedge_instrument_id {
            self.hedge_position += signed_qty;

            let is_closed = self
                .cache()
                .order(&event.client_order_id)
                .is_some_and(|o| o.is_closed());

            if is_closed {
                self.hedge_pending = false;
            }
        } else if Some(event.instrument_id) == self.call_instrument_id {
            self.call_position += signed_qty;
        } else if Some(event.instrument_id) == self.put_instrument_id {
            self.put_position += signed_qty;
        }

        log::info!(
            "Fill: {} {:.4} {} | positions: call={}, put={}, hedge={}",
            event.order_side,
            event.last_qty,
            event.instrument_id,
            self.call_position,
            self.put_position,
            self.hedge_position,
        );

        Ok(())
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        let instrument_id = self
            .cache()
            .order(&event.client_order_id)
            .map(|o| o.instrument_id());

        if instrument_id == Some(self.config.hedge_instrument_id) {
            self.hedge_pending = false;
        }

        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.as_str() == REHEDGE_TIMER {
            self.check_rehedge()?;
        }

        Ok(())
    }
}
