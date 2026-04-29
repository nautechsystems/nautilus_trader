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

use nautilus_common::{actor::DataActor, timer::TimeEvent};
use nautilus_core::params::Params;
use nautilus_model::{
    data::{QuoteTick, option_chain::OptionGreeks},
    enums::{OptionKind, OrderSide},
    events::{OrderCanceled, OrderFilled},
    identifiers::InstrumentId,
    instruments::Instrument,
    orders::Order,
    types::{Price, Quantity},
};
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
    pub(super) call_delta_ready: bool,
    pub(super) put_delta_ready: bool,
    pub(super) call_position: f64,
    pub(super) put_position: f64,
    pub(super) hedge_position: f64,
    pub(super) hedge_pending: bool,
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
            call_delta_ready: false,
            put_delta_ready: false,
            call_position: 0.0,
            put_position: 0.0,
            hedge_position: 0.0,
            hedge_pending: false,
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

    /// Returns `true` when strangle entry can proceed: both mark IVs
    /// are available, no positions exist yet, and entry was not already sent.
    #[must_use]
    pub fn should_enter_strangle(&self) -> bool {
        self.config.enter_strangle
            && self.greeks_initialized()
            && self.call_mark_iv.is_some()
            && self.put_mark_iv.is_some()
            && self.call_position == 0.0
            && self.put_position == 0.0
            && !self.has_working_entry_orders()
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
        let call_iv = self.call_mark_iv.unwrap();
        let put_iv = self.put_mark_iv.unwrap();
        let offset = self.config.entry_iv_offset;

        let call_entry_iv = call_iv - offset;
        let put_entry_iv = put_iv - offset;

        log::info!(
            "Entering strangle: SELL {} x {call_id} @ iv={call_entry_iv:.4} \
             + SELL {} x {put_id} @ iv={put_entry_iv:.4} (offset={offset})",
            self.config.contracts,
            self.config.contracts,
        );

        let contracts = self.config.contracts;
        let tif = self.config.entry_time_in_force;
        let client_id = self.config.client_id;

        let call_order = self.core.order_factory().limit(
            call_id,
            OrderSide::Sell,
            Quantity::new(contracts as f64, 0),
            Price::new(call_entry_iv, 4),
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

        let mut call_params = Params::new();
        call_params.insert(
            self.config.iv_param_key.clone(),
            json!(call_entry_iv.to_string()),
        );

        self.submit_order_with_params(call_order, None, Some(client_id), call_params)?;

        let put_order = self.core.order_factory().limit(
            put_id,
            OrderSide::Sell,
            Quantity::new(contracts as f64, 0),
            Price::new(put_entry_iv, 4),
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

        let mut put_params = Params::new();
        put_params.insert(
            self.config.iv_param_key.clone(),
            json!(put_entry_iv.to_string()),
        );

        self.submit_order_with_params(put_order, None, Some(client_id), put_params)?;

        Ok(())
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

        let order = self.core.order_factory().market(
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

        if let Err(e) = self.submit_order(order, None, Some(self.config.client_id)) {
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
        let now_ns = self.timestamp_ns().as_u64();

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
            log::info!(
                "Strangle entry enabled: SELL {} x {call_id} (call) + SELL {} x {put_id} (put) \
                 once Greeks arrive (iv_offset={})",
                self.config.contracts,
                self.config.contracts,
                self.config.entry_iv_offset,
            );
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

        if let Some(call_id) = self.call_instrument_id {
            self.cancel_all_orders(call_id, None, None)?;
        }

        if let Some(put_id) = self.put_instrument_id {
            self.cancel_all_orders(put_id, None, None)?;
        }

        let hedge_id = self.config.hedge_instrument_id;
        self.unsubscribe_quotes(hedge_id, None, None);
        self.cancel_all_orders(hedge_id, None, None)?;
        self.hedge_pending = false;

        log::info!("Delta-neutral vol strategy stopped, positions left unchanged");

        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        if Some(greeks.instrument_id) == self.call_instrument_id {
            self.call_delta = greeks.greeks.delta;
            self.call_delta_ready = true;

            if let Some(iv) = greeks.mark_iv {
                self.call_mark_iv = Some(iv);
            }
        } else if Some(greeks.instrument_id) == self.put_instrument_id {
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
        if quote.instrument_id == self.config.hedge_instrument_id {
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
