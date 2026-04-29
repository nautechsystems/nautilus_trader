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

//! Hurst/VPIN directional strategy implementation.

use std::{collections::VecDeque, fmt::Debug};

use ahash::AHashSet;
use nautilus_common::actor::DataActor;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::{AggressorSide, OrderSide, PositionSide, TimeInForce},
    events::{
        OrderCanceled, OrderDenied, OrderExpired, OrderFilled, OrderRejected, PositionClosed,
        PositionOpened,
    },
    identifiers::{ClientOrderId, PositionId, StrategyId},
    orders::{Order, OrderCore},
    types::Quantity,
};

use super::config::HurstVpinDirectionalConfig;
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

/// Directional strategy combining a Hurst-exponent regime filter on dollar bars
/// with a VPIN (Volume-synchronized Probability of Informed Trading) signal
/// derived from trade aggressor flow, with entry timing gated by the live
/// quote stream.
///
/// The strategy is sampled on information-driven (value) bars rather than
/// clock time, following Lopez de Prado (*Advances in Financial Machine
/// Learning*, Chapter 2). The Hurst exponent is estimated by rescaled range
/// over the window of dollar bar log returns. VPIN is averaged over completed
/// volume buckets, with a signed variant carrying the net informed direction.
pub struct HurstVpinDirectional {
    pub(super) core: StrategyCore,
    pub(super) config: HurstVpinDirectionalConfig,
    pub(super) returns: VecDeque<f64>,
    pub(super) abs_imbalances: VecDeque<f64>,
    pub(super) signed_imbalances: VecDeque<f64>,
    pub(super) last_close: Option<f64>,
    pub(super) bucket_buy_volume: f64,
    pub(super) bucket_sell_volume: f64,
    pub(super) hurst: Option<f64>,
    pub(super) vpin: Option<f64>,
    pub(super) signed_vpin: Option<f64>,
    pub(super) position_opened_ns: Option<u64>,
    pub(super) exit_cooldown: bool,
    pub(super) entry_order_id: Option<ClientOrderId>,
    pub(super) exit_order_ids: AHashSet<ClientOrderId>,
}

impl HurstVpinDirectional {
    /// Creates a new [`HurstVpinDirectional`] instance from config.
    #[must_use]
    pub fn new(config: HurstVpinDirectionalConfig) -> Self {
        let hurst_window = config.hurst_window;
        let vpin_window = config.vpin_window;
        Self {
            core: StrategyCore::new(config.base.clone()),
            config,
            returns: VecDeque::with_capacity(hurst_window),
            abs_imbalances: VecDeque::with_capacity(vpin_window),
            signed_imbalances: VecDeque::with_capacity(vpin_window),
            last_close: None,
            bucket_buy_volume: 0.0,
            bucket_sell_volume: 0.0,
            hurst: None,
            vpin: None,
            signed_vpin: None,
            position_opened_ns: None,
            exit_cooldown: false,
            entry_order_id: None,
            exit_order_ids: AHashSet::new(),
        }
    }

    pub(super) fn signals_ready(&self) -> bool {
        self.hurst.is_some()
            && self.vpin.is_some()
            && self.signed_vpin.is_some()
            && self.returns.len() == self.config.hurst_window
            && self.abs_imbalances.len() == self.config.vpin_window
    }

    pub(super) fn push_bounded(values: &mut VecDeque<f64>, capacity: usize, value: f64) {
        if values.len() == capacity {
            values.pop_front();
        }
        values.push_back(value);
    }

    pub(super) fn rolling_mean(values: &VecDeque<f64>) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        Some(values.iter().copied().sum::<f64>() / values.len() as f64)
    }

    #[allow(
        clippy::cognitive_complexity,
        reason = "R/S regression is inherently nested"
    )]
    pub(super) fn estimate_hurst(&self) -> Option<f64> {
        if self.returns.len() < self.config.hurst_window {
            return None;
        }

        let returns: Vec<f64> = self.returns.iter().copied().collect();
        let mut log_lags: Vec<f64> = Vec::new();
        let mut log_rs: Vec<f64> = Vec::new();

        for &lag in &self.config.hurst_lags {
            if lag < 2 || lag > returns.len() {
                continue;
            }

            let mut rs_values: Vec<f64> = Vec::new();

            for start in (0..=returns.len().saturating_sub(lag)).step_by(lag) {
                let chunk = &returns[start..start + lag];
                let mean = chunk.iter().sum::<f64>() / lag as f64;

                let mut running = 0.0f64;
                let mut cum_min = 0.0f64;
                let mut cum_max = 0.0f64;
                let mut var_sum = 0.0f64;

                for value in chunk {
                    let deviation = value - mean;
                    running += deviation;
                    if running < cum_min {
                        cum_min = running;
                    }

                    if running > cum_max {
                        cum_max = running;
                    }
                    var_sum += deviation * deviation;
                }
                let r_range = cum_max - cum_min;
                let stdev = (var_sum / lag as f64).sqrt();
                if r_range > 0.0 && stdev > 0.0 {
                    rs_values.push(r_range / stdev);
                }
            }

            if !rs_values.is_empty() {
                let avg_rs = rs_values.iter().copied().sum::<f64>() / rs_values.len() as f64;
                log_lags.push((lag as f64).ln());
                log_rs.push(avg_rs.ln());
            }
        }

        if log_lags.len() < 2 {
            return None;
        }

        let n = log_lags.len() as f64;
        let sx: f64 = log_lags.iter().sum();
        let sy: f64 = log_rs.iter().sum();
        let sxx: f64 = log_lags.iter().map(|x| x * x).sum();
        let sxy: f64 = log_lags.iter().zip(log_rs.iter()).map(|(x, y)| x * y).sum();
        let denom = n * sxx - sx * sx;
        if denom == 0.0 {
            return None;
        }
        Some((n * sxy - sx * sy) / denom)
    }

    pub(super) fn try_open_position(&mut self) -> anyhow::Result<()> {
        let (hurst, vpin, signed_vpin) = match (self.hurst, self.vpin, self.signed_vpin) {
            (Some(h), Some(v), Some(s)) => (h, v, s),
            _ => return Ok(()),
        };

        if hurst < self.config.hurst_enter || vpin < self.config.vpin_threshold {
            return Ok(());
        }

        if signed_vpin > 0.0 {
            self.submit_entry(OrderSide::Buy)?;
        } else if signed_vpin < 0.0 {
            self.submit_entry(OrderSide::Sell)?;
        }

        Ok(())
    }

    pub(super) fn check_regime_exit(&mut self) -> anyhow::Result<()> {
        if !self.exit_order_ids.is_empty() {
            return Ok(());
        }
        let hurst = match self.hurst {
            Some(h) => h,
            None => return Ok(()),
        };

        if hurst >= self.config.hurst_exit {
            return Ok(());
        }

        let has_open_position = self.has_open_position();
        if !has_open_position {
            return Ok(());
        }

        log::info!("Regime decay (Hurst={hurst:.3}); closing position");
        self.submit_close()
    }

    pub(super) fn check_holding_timeout(&mut self, tick: &QuoteTick) -> anyhow::Result<()> {
        if !self.exit_order_ids.is_empty() {
            return Ok(());
        }
        let opened_ns = match self.position_opened_ns {
            Some(ns) => ns,
            None => return Ok(()),
        };
        let held_ns = tick.ts_event.as_u64().saturating_sub(opened_ns);
        if held_ns < self.config.max_holding_secs * 1_000_000_000 {
            return Ok(());
        }

        log::info!("Holding timeout reached; closing position");
        self.submit_close()
    }

    fn submit_entry(&mut self, side: OrderSide) -> anyhow::Result<()> {
        let order = self.core.order_factory().market(
            self.config.instrument_id,
            side,
            self.config.trade_size,
            Some(TimeInForce::Ioc),
            None, // reduce_only
            None, // quote_quantity
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
            None, // client_order_id
        );
        self.entry_order_id = Some(order.client_order_id());
        self.submit_order(order, None, None)
    }

    fn submit_close(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let strategy_id = StrategyId::from(self.actor_id.inner().as_str());

        let positions: Vec<(PositionId, Quantity, PositionSide)> = self
            .cache()
            .positions_open(None, Some(&instrument_id), Some(&strategy_id), None, None)
            .iter()
            .map(|p| (p.id, p.quantity, p.side))
            .collect();

        if positions.is_empty() {
            return Ok(());
        }

        self.exit_cooldown = true;

        for (position_id, quantity, side) in positions {
            let closing_side = OrderCore::closing_side(side);
            let close_order = self.core.order_factory().market(
                instrument_id,
                closing_side,
                quantity,
                Some(TimeInForce::Ioc),
                Some(true), // reduce_only
                None,
                None,
                None,
                None,
                None,
            );
            self.exit_order_ids.insert(close_order.client_order_id());
            self.submit_order(close_order, Some(position_id), None)?;
        }

        Ok(())
    }

    fn has_open_position(&self) -> bool {
        let instrument_id = self.config.instrument_id;
        let strategy_id = StrategyId::from(self.actor_id.inner().as_str());
        !self
            .cache()
            .positions_open(None, Some(&instrument_id), Some(&strategy_id), None, None)
            .is_empty()
    }

    fn clear_latch_for(&mut self, client_order_id: &ClientOrderId) {
        if self.entry_order_id.as_ref() == Some(client_order_id) {
            self.entry_order_id = None;
        }
        self.exit_order_ids.remove(client_order_id);
    }
}

nautilus_strategy!(HurstVpinDirectional, {
    fn on_position_opened(&mut self, event: PositionOpened) {
        if event.instrument_id == self.config.instrument_id {
            self.position_opened_ns = Some(event.ts_event.as_u64());
        }
    }

    fn on_position_closed(&mut self, event: PositionClosed) {
        if event.instrument_id == self.config.instrument_id {
            self.position_opened_ns = None;
        }
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch_for(&event.client_order_id);
        }
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch_for(&event.client_order_id);
        }
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch_for(&event.client_order_id);
        }
    }
});

impl Debug for HurstVpinDirectional {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HurstVpinDirectional))
            .field("config", &self.config)
            .field("hurst", &self.hurst)
            .field("vpin", &self.vpin)
            .field("signed_vpin", &self.signed_vpin)
            .finish()
    }
}

impl DataActor for HurstVpinDirectional {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let bar_instrument_id = self.config.bar_type.instrument_id();
        if bar_instrument_id != instrument_id {
            anyhow::bail!(
                "bar_type instrument {bar_instrument_id} does not match traded instrument {instrument_id}"
            );
        }
        {
            let cache = self.cache();
            if cache.instrument(&instrument_id).is_none() {
                anyhow::bail!("Instrument {instrument_id} not found in cache");
            }
        }

        self.subscribe_bars(self.config.bar_type, None, None);
        self.subscribe_quotes(instrument_id, None, None);
        self.subscribe_trades(instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        self.cancel_all_orders(instrument_id, None, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None)?;
        self.unsubscribe_bars(self.config.bar_type, None, None);
        self.unsubscribe_quotes(instrument_id, None, None);
        self.unsubscribe_trades(instrument_id, None, None);
        Ok(())
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        let size = tick.size.as_f64();
        match tick.aggressor_side {
            AggressorSide::Buyer => self.bucket_buy_volume += size,
            AggressorSide::Seller => self.bucket_sell_volume += size,
            _ => {}
        }
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        let close = bar.close.as_f64();

        if let Some(prev) = self.last_close
            && prev > 0.0
            && close > 0.0
        {
            let window = self.config.hurst_window;
            Self::push_bounded(&mut self.returns, window, (close / prev).ln());
        }
        self.last_close = Some(close);

        let total = self.bucket_buy_volume + self.bucket_sell_volume;
        if total > 0.0 {
            let imbalance = (self.bucket_buy_volume - self.bucket_sell_volume) / total;
            let vpin_window = self.config.vpin_window;
            Self::push_bounded(&mut self.abs_imbalances, vpin_window, imbalance.abs());
            Self::push_bounded(&mut self.signed_imbalances, vpin_window, imbalance);
        }
        self.bucket_buy_volume = 0.0;
        self.bucket_sell_volume = 0.0;

        self.hurst = self.estimate_hurst();
        self.vpin = Self::rolling_mean(&self.abs_imbalances);
        self.signed_vpin = Self::rolling_mean(&self.signed_imbalances);

        if let Some(h) = self.hurst {
            log::info!(
                "Hurst={h:.3} VPIN={:.3} signed={:+.3} bar_close={close:.2}",
                self.vpin.unwrap_or(0.0),
                self.signed_vpin.unwrap_or(0.0),
            );
        }

        self.exit_cooldown = false;
        self.check_regime_exit()
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if !self.signals_ready() {
            return Ok(());
        }

        if self.has_open_position() {
            return self.check_holding_timeout(quote);
        }

        if self.exit_cooldown {
            return Ok(());
        }

        if self.entry_order_id.is_some() || !self.exit_order_ids.is_empty() {
            return Ok(());
        }

        let strategy_id = StrategyId::from(self.actor_id.inner().as_str());
        let has_working = {
            let cache = self.cache();
            !cache
                .orders_open(
                    None,
                    Some(&self.config.instrument_id),
                    Some(&strategy_id),
                    None,
                    None,
                )
                .is_empty()
                || !cache
                    .orders_inflight(
                        None,
                        Some(&self.config.instrument_id),
                        Some(&strategy_id),
                        None,
                        None,
                    )
                    .is_empty()
        };

        if has_working {
            return Ok(());
        }

        self.try_open_position()
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        if event.instrument_id != self.config.instrument_id {
            return Ok(());
        }

        let closed = self
            .cache()
            .order(&event.client_order_id)
            .is_some_and(|o| o.is_closed());
        if closed {
            self.clear_latch_for(&event.client_order_id);
        }
        Ok(())
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        if event.instrument_id != self.config.instrument_id {
            return Ok(());
        }
        self.clear_latch_for(&event.client_order_id);
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.returns.clear();
        self.abs_imbalances.clear();
        self.signed_imbalances.clear();
        self.last_close = None;
        self.bucket_buy_volume = 0.0;
        self.bucket_sell_volume = 0.0;
        self.hurst = None;
        self.vpin = None;
        self.signed_vpin = None;
        self.position_opened_ns = None;
        self.exit_cooldown = false;
        self.entry_order_id = None;
        self.exit_order_ids.clear();
        Ok(())
    }
}
