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

//! Multi-level order book imbalance (OBI) momentum strategy implementation.

use std::{collections::VecDeque, fmt::Debug, num::NonZeroUsize, time::Duration};

use ahash::AHashSet;
use nautilus_common::{actor::DataActor, timer::TimeEvent};
use nautilus_model::{
    enums::{BookType::L2_MBP, OrderSide, PositionSide, TimeInForce::Ioc},
    events::{
        OrderCanceled, OrderDenied, OrderExpired, OrderFilled, OrderRejected, PositionClosed,
        PositionOpened,
    },
    identifiers::{ClientOrderId, PositionId},
    instruments::Instrument,
    orders::{Order, OrderCore},
    types::Quantity,
};
use nautilus_trading::{Strategy, StrategyCore, nautilus_strategy};

use crate::strategy::obi_momentum::config::ObiMomentumConfig;

/// Name of the timer driving indicator evaluation.
const TIMER_NAME: &str = "OBI_MOM_TIMER";

/// Pushes a value into a bounded ring buffer, evicting the oldest element.
pub(super) fn push_bounded(values: &mut VecDeque<f64>, capacity: usize, value: f64) {
    if values.len() == capacity {
        values.pop_front();
    }
    values.push_back(value);
}

/// Computes the order book imbalance over the given bid/ask `(price, size)` levels.
///
/// With `weighted == false` this is the standard top-N imbalance
/// `(ΣB − ΣA) / (ΣB + ΣA)`. With `weighted == true` each level is weighted by
/// the inverse distance from the mid price, giving closer levels more weight.
/// Returns `None` when the book has no volume.
#[must_use]
pub(super) fn imbalance(
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    mid: f64,
    weighted: bool,
) -> Option<f64> {
    let (bid_vol, ask_vol) = if weighted {
        let weight = |price: f64, size: f64| size / (price - mid).abs().max(1e-12);
        (
            bids.iter().map(|(p, s)| weight(*p, *s)).sum::<f64>(),
            asks.iter().map(|(p, s)| weight(*p, *s)).sum::<f64>(),
        )
    } else {
        (
            bids.iter().map(|(_, s)| *s).sum::<f64>(),
            asks.iter().map(|(_, s)| *s).sum::<f64>(),
        )
    };

    let total = bid_vol + ask_vol;
    if total <= 0.0 {
        return None;
    }
    Some((bid_vol - ask_vol) / total)
}

/// Computes the z-score of the latest sample against the rolling mean/stddev
/// of the window. Returns `None` for an empty window and `0.0` when the
/// window has no dispersion (all samples identical).
#[must_use]
pub(super) fn z_score(samples: &VecDeque<f64>) -> Option<f64> {
    let n = samples.len();
    if n == 0 {
        return None;
    }
    let latest = *samples.back()?;
    let mean = samples.iter().copied().sum::<f64>() / n as f64;
    let variance = samples
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / n as f64;
    if variance <= 0.0 {
        return Some(0.0);
    }
    Some((latest - mean) / variance.sqrt())
}

/// Realized volatility over a window of returns: `sqrt(Σ r²)`.
#[must_use]
pub(super) fn realized_vol(returns: &VecDeque<f64>) -> Option<f64> {
    if returns.is_empty() {
        return None;
    }
    Some(returns.iter().map(|r| r * r).sum::<f64>().sqrt())
}

/// Median of a window of samples.
#[must_use]
pub(super) fn median(values: &VecDeque<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = values.iter().copied().collect();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
    }
}

/// Notional quantity for the given fraction of capital at the given mid price.
#[must_use]
pub(super) fn notional_qty(capital: f64, pct: f64, mid: f64) -> f64 {
    if capital <= 0.0 || pct <= 0.0 || mid <= 0.0 {
        return 0.0;
    }
    pct * capital / mid
}

/// Floors a quantity down to a multiple of the given size increment.
#[must_use]
pub(super) fn floor_to_increment(qty: f64, increment: f64) -> f64 {
    if increment <= 0.0 {
        return qty;
    }
    (qty / increment).floor() * increment
}

/// Multi-level order book imbalance momentum strategy.
///
/// Computes the imbalance between bid and ask volume across the top
/// `num_levels` book levels, standardizes it into a z-score over a rolling
/// window of timer-driven evaluations, and trades the resulting momentum:
/// entering long above `+entry_threshold`, short below `-entry_threshold`,
/// reducing on signal decay below `reduce_threshold`, and flattening below
/// `close_threshold` or on a sign flip. An optional realized-volatility
/// regime filter blocks new entries during low-volatility environments.
pub struct ObiMomentum {
    pub(super) core: StrategyCore,
    pub(super) config: ObiMomentumConfig,
    pub(super) size_precision: Option<u8>,
    pub(super) size_increment: Option<Quantity>,
    pub(super) min_quantity: Option<Quantity>,
    pub(super) capital: Option<f64>,
    pub(super) imbalance_samples: VecDeque<f64>,
    pub(super) returns: VecDeque<f64>,
    pub(super) vol_samples: VecDeque<f64>,
    pub(super) last_mid: Option<f64>,
    pub(super) last_update_ns: Option<u64>,
    pub(super) position_opened_ns: Option<u64>,
    pub(super) entry_order_id: Option<ClientOrderId>,
    pub(super) exit_order_ids: AHashSet<ClientOrderId>,
}

impl ObiMomentum {
    /// Creates a new [`ObiMomentum`] instance from config.
    #[must_use]
    #[allow(dead_code)]
    pub fn new(config: ObiMomentumConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            size_precision: None,
            size_increment: None,
            min_quantity: None,
            capital: config.capital,
            imbalance_samples: VecDeque::with_capacity(config.zscore_window),
            returns: VecDeque::with_capacity(config.regime_vol_window),
            vol_samples: VecDeque::with_capacity(config.regime_history_window),
            last_mid: None,
            last_update_ns: None,
            position_opened_ns: None,
            entry_order_id: None,
            exit_order_ids: AHashSet::new(),
            config,
        }
    }

    /// Returns `true` once the z-score window has been filled.
    fn signal_ready(&self) -> bool {
        self.imbalance_samples.len() >= self.config.zscore_window
    }

    /// Returns `true` when realized volatility is below its rolling median
    /// (low-vol regime, blocks new entries when the filter is enabled).
    fn low_vol_regime(&self) -> bool {
        if !self.config.regime_filter_enabled
            || self.returns.len() < self.config.regime_vol_window
            || self.vol_samples.len() < self.config.regime_history_window
        {
            return false;
        }
        let Some(vol) = realized_vol(&self.returns) else {
            return false;
        };
        median(&self.vol_samples).is_some_and(|median_vol| vol < median_vol)
    }

    /// Evaluates the z-score signal against the current position and trades.
    fn handle_signal(&mut self, signal: f64, mid: f64) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let strategy_id = self.strategy_id().expect("Strategy must be registered");

        if !self.exit_order_ids.is_empty() || self.entry_order_id.is_some() {
            return Ok(());
        }

        let open: Vec<(PositionId, Quantity, PositionSide, f64)> = self
            .cache()
            .positions_open(None, Some(&instrument_id), Some(&strategy_id), None, None)
            .iter()
            .map(|position| {
                (
                    position.id,
                    position.quantity,
                    position.side,
                    position.signed_qty,
                )
            })
            .collect();
        let net_qty: f64 = open.iter().map(|(_, _, _, qty)| qty).sum();

        if net_qty == 0.0 {
            self.handle_entry(signal, mid)?;
        } else {
            self.handle_position(signal, net_qty, mid, &open)?;
        }

        Ok(())
    }

    /// Manages an open position: flatten on timeout/signal decay/flip,
    /// reduce when the signal weakens.
    fn handle_position(
        &mut self,
        signal: f64,
        net_qty: f64,
        mid: f64,
        open: &[(PositionId, Quantity, PositionSide, f64)],
    ) -> anyhow::Result<()> {
        if let Some(max_secs) = self.config.max_holding_secs
            && let Some(opened_ns) = self.position_opened_ns
            && let Some(now_ns) = self.last_update_ns
            && now_ns.saturating_sub(opened_ns) >= max_secs * 1_000_000_000
        {
            log::info!("OBI: holding timeout reached; closing position");
            return self.submit_close(open);
        }

        let close = if net_qty > 0.0 {
            signal < -self.config.close_threshold || signal.abs() < self.config.close_threshold
        } else {
            signal > self.config.close_threshold || signal.abs() < self.config.close_threshold
        };
        if close {
            log::info!("OBI: signal {signal:.3} warrants close; closing position");
            return self.submit_close(open);
        }

        if signal.abs() < self.config.reduce_threshold {
            log::info!("OBI: signal {signal:.3} below reduce threshold; reducing position");
            return self.submit_reduce(open, mid);
        }

        Ok(())
    }

    /// Opens a long/short position when the signal crosses its entry threshold
    /// and the regime filter (if enabled) allows it.
    fn handle_entry(&mut self, signal: f64, mid: f64) -> anyhow::Result<()> {
        if self.low_vol_regime() {
            log::info!("OBI: low-vol regime; skipping entry (signal {signal:.3})");
            return Ok(());
        }

        let Some(trade_size) = self.notional_to_qty(self.config.trade_size_pct, mid) else {
            log::warn!("OBI: cannot resolve trade size (capital/mid unavailable)");
            return Ok(());
        };
        let trade_size =
            if let Some(max_qty) = self.notional_to_qty(self.config.max_position_pct, mid) {
                trade_size.min(max_qty)
            } else {
                trade_size
            };

        let side = if signal > self.config.entry_threshold {
            OrderSide::Buy
        } else if signal < -self.config.entry_threshold {
            OrderSide::Sell
        } else {
            return Ok(());
        };

        let instrument_id = self.config.instrument_id;
        let order = self.order().market(
            instrument_id,
            side,
            trade_size,
            Some(Ioc),
            None, // reduce_only
            None, // quote_quantity
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // tags
            None, // client_order_id
        );
        log::info!(
            "OBI: signal {signal:.3} -> submitting {side:?} entry {trade_size} (~{:.2} USDT)",
            self.config.trade_size_pct * self.capital.unwrap_or(0.0)
        );
        self.entry_order_id = Some(order.client_order_id());
        self.submit_order(order, None, None, None)
    }

    /// Flattens all open positions with reduce-only IOC market orders.
    fn submit_close(
        &mut self,
        open: &[(PositionId, Quantity, PositionSide, f64)],
    ) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        for (position_id, quantity, side, _) in open {
            let closing_side = OrderCore::closing_side(*side);
            let close_order = self.order().market(
                instrument_id,
                closing_side,
                *quantity,
                Some(Ioc),
                Some(true), // reduce_only
                None,
                None,
                None,
                None,
                None,
            );
            self.exit_order_ids.insert(close_order.client_order_id());
            self.submit_order(close_order, Some(*position_id), None, None)?;
        }
        Ok(())
    }

    /// Reduces open positions by one `trade_size` worth of quantity each.
    fn submit_reduce(
        &mut self,
        open: &[(PositionId, Quantity, PositionSide, f64)],
        mid: f64,
    ) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let Some(trade_size) = self.notional_to_qty(self.config.trade_size_pct, mid) else {
            return Ok(());
        };
        for (position_id, quantity, side, _) in open {
            let reduce_qty = if trade_size >= *quantity {
                *quantity
            } else {
                trade_size
            };
            let closing_side = OrderCore::closing_side(*side);
            let reduce_order = self.order().market(
                instrument_id,
                closing_side,
                reduce_qty,
                Some(Ioc),
                Some(true), // reduce_only
                None,
                None,
                None,
                None,
                None,
            );
            self.exit_order_ids.insert(reduce_order.client_order_id());
            self.submit_order(reduce_order, Some(*position_id), None, None)?;
        }
        Ok(())
    }

    fn clear_latch(&mut self, client_order_id: ClientOrderId) {
        if self.entry_order_id == Some(client_order_id) {
            self.entry_order_id = None;
        }
        self.exit_order_ids.remove(&client_order_id);
    }

    /// Resolves the quantity for the given fraction of capital at the given
    /// mid price, floored to the instrument's size increment.
    fn notional_to_qty(&self, pct: f64, mid: f64) -> Option<Quantity> {
        let capital = self.capital?;
        let step = self.size_increment?.as_f64();
        let size_precision = self.size_precision?;
        let qty = floor_to_increment(notional_qty(capital, pct, mid), step);
        if qty <= 0.0 {
            return None;
        }
        let mut qty = Quantity::new(qty, size_precision);
        if let Some(min_qty) = self.min_quantity
            && min_qty > qty
        {
            qty = min_qty;
        }
        Some(qty)
    }

    /// Evaluates the indicator from the cached order book and trades on it.
    fn evaluate(&mut self, now_ns: u64) -> anyhow::Result<()> {
        let order_book = match self.cache().order_book(&self.config.instrument_id) {
            Some(book) => book,
            None => return Ok(()),
        };
        let (Some(bid_price), Some(ask_price)) =
            (order_book.best_bid_price(), order_book.best_ask_price())
        else {
            return Ok(());
        };
        let mid = f64::midpoint(bid_price.as_f64(), ask_price.as_f64());

        if let Some(last_mid) = self.last_mid
            && last_mid > 0.0
        {
            push_bounded(
                &mut self.returns,
                self.config.regime_vol_window,
                (mid - last_mid) / last_mid,
            );
        }
        self.last_mid = Some(mid);
        self.last_update_ns = Some(now_ns);

        let depth = Some(self.config.num_levels);
        let bids: Vec<(f64, f64)> = order_book
            .bids(depth)
            .map(|level| (level.price.value.as_f64(), level.size_decimal().as_f64()))
            .collect();
        let asks: Vec<(f64, f64)> = order_book
            .asks(depth)
            .map(|level| (level.price.value.as_f64(), level.size_decimal().as_f64()))
            .collect();

        let Some(imbalance_value) = imbalance(&bids, &asks, mid, self.config.weighted) else {
            return Ok(());
        };
        push_bounded(
            &mut self.imbalance_samples,
            self.config.zscore_window,
            imbalance_value,
        );

        if let Some(vol) = realized_vol(&self.returns) {
            push_bounded(
                &mut self.vol_samples,
                self.config.regime_history_window,
                vol,
            );
        }

        if !self.signal_ready() {
            return Ok(());
        }
        let Some(signal) = z_score(&self.imbalance_samples) else {
            return Ok(());
        };

        self.handle_signal(signal, mid)
    }
}

nautilus_strategy!(ObiMomentum, {
    fn on_position_opened(&mut self, event: PositionOpened) {
        if event.instrument_id == self.config.instrument_id {
            self.position_opened_ns = Some(event.ts_event.as_u64());
        }
        self.entry_order_id = None;
    }

    fn on_position_closed(&mut self, event: PositionClosed) {
        if event.instrument_id == self.config.instrument_id {
            self.position_opened_ns = None;
        }
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch(event.client_order_id);
        }
    }

    fn on_order_expired(&mut self, event: OrderExpired) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch(event.client_order_id);
        }
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch(event.client_order_id);
        }
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        if event.instrument_id != self.config.instrument_id {
            return;
        }
        // Only discard once fully filled; partial fills must keep the ID so a
        // subsequent order is not misclassified.
        let closed = {
            let cache = self.cache();
            cache
                .order(&event.client_order_id)
                .is_some_and(|order| order.is_closed())
        };
        if closed {
            self.clear_latch(event.client_order_id);
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        if event.instrument_id == self.config.instrument_id {
            self.clear_latch(event.client_order_id);
        }
    }
});

impl Debug for ObiMomentum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ObiMomentum))
            .field("config", &self.config)
            .field("capital", &self.capital)
            .finish()
    }
}

impl DataActor for ObiMomentum {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let (size_precision, size_increment, min_quantity) = {
            let cache = self.cache();
            let instrument = cache.try_instrument(&instrument_id)?;
            (
                instrument.size_precision(),
                instrument.size_increment(),
                instrument.min_quantity(),
            )
        };
        self.size_precision = Some(size_precision);
        self.size_increment = Some(size_increment);
        self.min_quantity = min_quantity;

        if self.capital.is_none() {
            let Some(quote_currency) = self
                .cache()
                .instrument(&instrument_id)
                .map(|i| i.quote_currency())
            else {
                log::warn!("OBI: instrument not found; cannot resolve capital from equity");
                return Ok(());
            };
            let equity = self
                .cache()
                .accounts_all()
                .into_iter()
                .find_map(|account| {
                    account
                        .balance(Some(quote_currency))
                        .map(|balance| balance.total.as_f64())
                })
                .unwrap_or(0.0);
            log::info!("OBI: resolved strategy capital from account equity: {equity:.2}");
            self.capital = Some(equity);
        }

        self.subscribe_book_deltas(
            instrument_id,
            L2_MBP,
            NonZeroUsize::new(50),
            None,
            true,
            None,
        );

        self.clock().set_timer(
            TIMER_NAME,
            Duration::from_millis(self.config.timer_interval_ms),
            None,
            None,
            None,
            None,
            None,
        )?;

        log::info!(
            "OBI momentum started: instrument={instrument_id}, levels={}, weighted={}, \
             zscore_window={}, entry={}, reduce={}, close={}, capital={:.2}, \
             trade_size_pct={}, max_position_pct={}, timer_interval_ms={}",
            self.config.num_levels,
            self.config.weighted,
            self.config.zscore_window,
            self.config.entry_threshold,
            self.config.reduce_threshold,
            self.config.close_threshold,
            self.capital.unwrap_or(0.0),
            self.config.trade_size_pct,
            self.config.max_position_pct,
            self.config.timer_interval_ms,
        );

        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name != TIMER_NAME {
            return Ok(());
        }
        self.evaluate(event.ts_event.as_u64())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.clock().cancel_timer(TIMER_NAME);
        let instrument_id = self.config.instrument_id;
        self.cancel_all_orders(instrument_id, None, None, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None, None)?;
        self.unsubscribe_book_deltas(instrument_id, None, None);
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.size_precision = None;
        self.size_increment = None;
        self.min_quantity = None;
        self.capital = self.config.capital;
        self.imbalance_samples.clear();
        self.returns.clear();
        self.vol_samples.clear();
        self.last_mid = None;
        self.last_update_ns = None;
        self.position_opened_ns = None;
        self.entry_order_id = None;
        self.exit_order_ids.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn deq(values: &[f64]) -> VecDeque<f64> {
        values.iter().copied().collect()
    }

    #[rstest]
    fn test_imbalance_balanced() {
        let bids = [(100.0, 10.0), (99.0, 5.0)];
        let asks = [(101.0, 10.0), (102.0, 5.0)];
        let result = imbalance(&bids, &asks, 100.5, false);
        assert_eq!(result, Some(0.0));
    }

    #[rstest]
    fn test_imbalance_bid_heavy() {
        let bids = [(100.0, 30.0), (99.0, 10.0)];
        let asks = [(101.0, 10.0), (102.0, 10.0)];
        let result = imbalance(&bids, &asks, 100.5, false).unwrap();
        assert!((result - 1.0 / 3.0).abs() < 1e-9);
    }

    #[rstest]
    fn test_imbalance_ask_heavy() {
        let bids = [(100.0, 10.0)];
        let asks = [(101.0, 30.0)];
        let result = imbalance(&bids, &asks, 100.5, false).unwrap();
        assert!((result + 0.5).abs() < 1e-9);
    }

    #[rstest]
    fn test_imbalance_empty_returns_none() {
        assert_eq!(imbalance(&[], &[], 100.0, false), None);
    }

    #[rstest]
    fn test_imbalance_weighted_weights_nearer_levels_more() {
        // Same total volume on both sides, but bid volume concentrated at the
        // best level and ask volume spread deeper: weighting flips the sign.
        let bids = [(100.0, 20.0), (99.0, 10.0)];
        let asks = [(101.0, 15.0), (102.0, 15.0)];
        let plain = imbalance(&bids, &asks, 100.5, false).unwrap();
        let weighted = imbalance(&bids, &asks, 100.5, true).unwrap();
        assert_eq!(plain, 0.0);
        assert!(weighted > 0.0);
    }

    #[rstest]
    fn test_z_score_positive_deviation() {
        let samples = deq(&[0.1, 0.2, 0.15, 0.1, 0.2, 0.15, 0.1, 0.2, 0.15, 0.5]);
        let z = z_score(&samples).unwrap();
        assert!(z > 2.0);
    }

    #[rstest]
    fn test_z_score_empty_returns_none() {
        assert_eq!(z_score(&deq(&[])), None);
    }

    #[rstest]
    fn test_z_score_flat_window_returns_zero() {
        assert_eq!(z_score(&deq(&[0.3, 0.3, 0.3, 0.3])), Some(0.0));
    }

    #[rstest]
    fn test_z_score_negative_deviation() {
        let samples = deq(&[0.9, 0.8, 0.85, 0.9, 0.8, 0.85, 0.9, 0.8, 0.85, 0.1]);
        let z = z_score(&samples).unwrap();
        assert!(z < -2.0);
    }

    #[rstest]
    fn test_realized_vol_empty_returns_none() {
        assert_eq!(realized_vol(&deq(&[])), None);
    }

    #[rstest]
    fn test_realized_vol_constant_returns_zero() {
        assert_eq!(realized_vol(&deq(&[0.0, 0.0, 0.0])), Some(0.0));
    }

    #[rstest]
    fn test_realized_vol_basic() {
        let vol = realized_vol(&deq(&[0.03, -0.04])).unwrap();
        assert!((vol - 0.05).abs() < 1e-9);
    }

    #[rstest]
    fn test_median_odd() {
        assert_eq!(median(&deq(&[3.0, 1.0, 2.0])), Some(2.0));
    }

    #[rstest]
    fn test_median_even() {
        assert_eq!(median(&deq(&[4.0, 1.0, 2.0, 3.0])), Some(2.5));
    }

    #[rstest]
    fn test_median_empty_returns_none() {
        assert_eq!(median(&deq(&[])), None);
    }

    #[rstest]
    fn test_push_bounded_evicts_oldest() {
        let mut samples = VecDeque::with_capacity(3);
        for value in [1.0, 2.0, 3.0, 4.0] {
            push_bounded(&mut samples, 3, value);
        }
        assert_eq!(
            samples.iter().copied().collect::<Vec<_>>(),
            vec![2.0, 3.0, 4.0]
        );
    }

    #[rstest]
    fn test_notional_qty_basic() {
        // 10% of 1000 USDT at a mid of 0.0856 -> ~1168 contracts
        let qty = notional_qty(1000.0, 0.10, 0.0856);
        assert!((qty - 1168.22).abs() < 0.01);
    }

    #[rstest]
    fn test_notional_qty_invalid_inputs_return_zero() {
        assert_eq!(notional_qty(0.0, 0.1, 1.0), 0.0);
        assert_eq!(notional_qty(1000.0, 0.0, 1.0), 0.0);
        assert_eq!(notional_qty(1000.0, 0.1, 0.0), 0.0);
    }

    #[rstest]
    fn test_floor_to_increment_rounds_down() {
        assert_eq!(floor_to_increment(1168.22, 1.0), 1168.0);
        assert_eq!(floor_to_increment(1168.22, 0.5), 1168.0);
        assert_eq!(floor_to_increment(1168.4, 5.0), 1165.0);
        assert_eq!(floor_to_increment(3.9, 1.0), 3.0);
    }

    #[rstest]
    fn test_floor_to_increment_nonpositive_increment_is_identity() {
        assert_eq!(floor_to_increment(3.9, 0.0), 3.9);
        assert_eq!(floor_to_increment(3.9, -1.0), 3.9);
    }
}
