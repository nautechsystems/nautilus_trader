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

//! Complement arbitrage strategy implementation.

use std::fmt::Debug;

use ahash::AHashMap;
use nautilus_common::actor::DataActor;
use nautilus_core::datetime::NANOSECONDS_IN_SECOND;
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, TimeInForce},
    events::{OrderCanceled, OrderExpired, OrderFilled, OrderRejected},
    identifiers::{ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::config::ComplementArbConfig;
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

/// A matched Yes/No complement pair sharing the same condition ID.
#[derive(Debug, Clone)]
pub(super) struct ComplementPair {
    pub condition_id: String,
    pub yes_id: InstrumentId,
    pub no_id: InstrumentId,
    pub label: String,
}

/// State of a per-pair arb execution attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ArbState {
    Idle,
    PendingEntry,
    PartialFill,
    Unwinding,
}

/// Tracks the lifecycle of a single arb attempt on a complement pair.
#[derive(Debug, Clone)]
pub(super) struct ArbExecution {
    pub state: ArbState,
    pub arb_side: OrderSide,
    pub yes_order_id: ClientOrderId,
    pub no_order_id: ClientOrderId,
    pub yes_filled_qty: Decimal,
    pub no_filled_qty: Decimal,
    pub unwind_order_id: Option<ClientOrderId>,
}

/// Complement arbitrage strategy that exploits Yes + No < 1.0 (buy arb) or
/// Yes + No > 1.0 (sell arb) on binary option pairs.
pub struct ComplementArb {
    pub(super) core: StrategyCore,
    config: ComplementArbConfig,
    /// condition_id -> matched pair
    pub(super) pairs: AHashMap<String, ComplementPair>,
    /// instrument_id -> condition_id (reverse lookup)
    pub(super) instrument_to_pair: AHashMap<InstrumentId, String>,
    /// Latest quote per instrument
    pub(super) quotes: AHashMap<InstrumentId, QuoteTick>,
    /// Instruments awaiting their complement, keyed by condition ID.
    /// Value: (instrument_id, outcome, label)
    pub(super) pending_complements: AHashMap<String, (InstrumentId, String, String)>,
    /// Active arb execution per pair: condition_id -> ArbExecution
    pub(super) arb_executions: AHashMap<String, ArbExecution>,
    /// Reverse lookup: client_order_id -> condition_id
    pub(super) order_to_pair: AHashMap<ClientOrderId, String>,
    /// Diagnostic counters
    pub(super) quotes_processed: u64,
    pub(super) buy_arbs_detected: u64,
    pub(super) sell_arbs_detected: u64,
    pub(super) arbs_submitted: u64,
    pub(super) arbs_completed: u64,
    pub(super) arbs_unwound: u64,
    pub(super) arbs_failed: u64,
    /// Tightest spreads seen (closest to arb opportunity)
    pub(super) best_buy_spread: Decimal,
    pub(super) best_sell_spread: Decimal,
    pub(super) best_buy_label: String,
    pub(super) best_sell_label: String,
}

impl ComplementArb {
    /// Creates a new [`ComplementArb`] instance from config.
    #[must_use]
    pub fn new(config: ComplementArbConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            config,
            pairs: AHashMap::new(),
            instrument_to_pair: AHashMap::new(),
            pending_complements: AHashMap::new(),
            quotes: AHashMap::new(),
            arb_executions: AHashMap::new(),
            order_to_pair: AHashMap::new(),
            quotes_processed: 0,
            buy_arbs_detected: 0,
            sell_arbs_detected: 0,
            arbs_submitted: 0,
            arbs_completed: 0,
            arbs_unwound: 0,
            arbs_failed: 0,
            best_buy_spread: Decimal::TWO,   // start high, track min
            best_sell_spread: Decimal::ZERO, // start low, track max
            best_buy_label: String::new(),
            best_sell_label: String::new(),
        }
    }

    /// Per-share fee for one leg using the Polymarket fee curve.
    /// Formula: (fee_rate_bps / 10_000) x p x (1 - p)
    /// Peaks at p=0.50, drops to zero at extremes.
    // 10_000 as Decimal (lo=10000, mid=0, hi=0, negative=false, scale=0)
    const BPS_DIVISOR: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);

    fn leg_fee(fee_rate_bps: Decimal, price: Decimal) -> Decimal {
        fee_rate_bps / Self::BPS_DIVISOR * price * (Decimal::ONE - price)
    }

    /// Extract pair key from instrument ID.
    ///
    /// For Polymarket-style IDs like `0xabc...def-<token_id>.POLYMARKET`,
    /// the condition ID (hex prefix before the last `-`) groups Yes/No pairs.
    pub(super) fn extract_pair_key(id: &InstrumentId) -> Option<String> {
        let s = id.symbol.as_str();
        let dash_pos = s.rfind('-')?;
        if dash_pos == 0 {
            return None;
        }
        Some(s[..dash_pos].to_string())
    }

    /// Attempt to match an instrument with a pending complement in O(1).
    ///
    /// If the complement is already pending, forms a pair and returns it.
    /// Otherwise stores the instrument as pending and returns `None`.
    pub(super) fn try_match_complement(
        &mut self,
        id: InstrumentId,
        outcome: &str,
        label: &str,
    ) -> Option<ComplementPair> {
        let key = Self::extract_pair_key(&id)?;

        // Already have a complete pair
        if self.pairs.contains_key(&key) {
            return None;
        }

        // Check pending for complement
        if let Some((other_id, other_outcome, other_label)) = self.pending_complements.remove(&key)
        {
            let (yes_id, no_id, pair_label) = match (outcome, other_outcome.as_str()) {
                ("Yes", "No") => (id, other_id, label.to_string()),
                ("No", "Yes") => (other_id, id, other_label),
                _ => {
                    // Same outcome or invalid — re-insert original, skip
                    self.pending_complements
                        .insert(key, (other_id, other_outcome, other_label));
                    return None;
                }
            };

            let pair = ComplementPair {
                condition_id: key.clone(),
                yes_id,
                no_id,
                label: pair_label,
            };

            self.instrument_to_pair.insert(yes_id, key.clone());
            self.instrument_to_pair.insert(no_id, key.clone());
            self.pairs.insert(key, pair.clone());

            Some(pair)
        } else {
            // No complement yet — store as pending
            self.pending_complements
                .insert(key, (id, outcome.to_string(), label.to_string()));
            None
        }
    }

    /// Discover complement pairs from cached instruments.
    fn discover_pairs(&mut self) {
        type Entry = (InstrumentId, Option<String>, Option<String>);

        let venue = self.config.venue;
        let instruments: Vec<Entry> = {
            let cache = self.cache();
            cache
                .instruments(&venue, None)
                .iter()
                .filter_map(|inst| {
                    if let InstrumentAny::BinaryOption(opt) = inst {
                        Some((
                            opt.id,
                            opt.description.map(|d| d.to_string()),
                            opt.outcome.map(|o| o.to_string()),
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        };

        // Group by pair key (condition ID)
        let mut groups: AHashMap<String, Vec<Entry>> = AHashMap::new();
        for (id, desc, outcome) in &instruments {
            if let Some(key) = Self::extract_pair_key(id) {
                groups
                    .entry(key)
                    .or_default()
                    .push((*id, desc.clone(), outcome.clone()));
            }
        }

        // Build pairs from groups with exactly 2 instruments
        for (key, members) in &groups {
            if members.len() != 2 {
                continue;
            }

            let (yes_idx, no_idx) = match (members[0].2.as_deref(), members[1].2.as_deref()) {
                (Some("Yes"), Some("No")) => (0, 1),
                (Some("No"), Some("Yes")) => (1, 0),
                _ => continue,
            };

            let label = members[yes_idx].1.clone().unwrap_or_else(|| key.clone());

            let pair = ComplementPair {
                condition_id: key.clone(),
                yes_id: members[yes_idx].0,
                no_id: members[no_idx].0,
                label,
            };

            self.instrument_to_pair.insert(pair.yes_id, key.clone());
            self.instrument_to_pair.insert(pair.no_id, key.clone());
            self.pairs.insert(key.clone(), pair);
        }

        // Store singles as pending complements for incremental discovery
        for (key, members) in &groups {
            if members.len() != 1 {
                continue;
            }

            if self.pairs.contains_key(key) {
                continue;
            }
            let (id, desc, outcome) = &members[0];
            if let Some(out) = outcome {
                let label = desc.clone().unwrap_or_else(|| key.clone());
                self.pending_complements
                    .insert(key.clone(), (*id, out.clone(), label));
            }
        }
    }

    /// Returns true if there is an active (non-Idle) arb execution for this pair.
    pub(super) fn has_active_arb(&self, pair_key: &str) -> bool {
        self.arb_executions
            .get(pair_key)
            .is_some_and(|exec| exec.state != ArbState::Idle)
    }

    /// Submit both legs of an arb as GTD limit orders.
    fn submit_arb(
        &mut self,
        pair: &ComplementPair,
        arb_side: OrderSide,
        yes_price: Price,
        no_price: Price,
        profit_bps: Decimal,
    ) -> anyhow::Result<()> {
        if !self.config.live_trading {
            return Ok(());
        }

        if self.has_active_arb(&pair.condition_id) {
            return Ok(());
        }

        // Guard: global concurrency limit across all pairs.
        // Per-market concurrency is already enforced by `has_active_arb` above.
        let active_count = self
            .arb_executions
            .values()
            .filter(|e| e.state != ArbState::Idle)
            .count();

        if active_count >= self.config.max_concurrent_arbs {
            return Ok(());
        }

        let now_ns = self.core.clock().timestamp_ns();
        let expire_ns = now_ns + self.config.order_expire_secs * NANOSECONDS_IN_SECOND;
        let trade_size = Quantity::from_decimal(self.config.trade_size)?;
        let post_only = Some(self.config.use_post_only);

        let yes_order = self.core.order_factory().limit(
            pair.yes_id,
            arb_side,
            trade_size,
            yes_price,
            Some(TimeInForce::Gtd),
            Some(expire_ns),
            post_only,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![Ustr::from("arb")]),
            None,
        );

        let no_order = self.core.order_factory().limit(
            pair.no_id,
            arb_side,
            trade_size,
            no_price,
            Some(TimeInForce::Gtd),
            Some(expire_ns),
            post_only,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![Ustr::from("arb")]),
            None,
        );

        let yes_order_id = yes_order.client_order_id();
        let no_order_id = no_order.client_order_id();

        let client_id = self.config.client_id;
        self.submit_order(yes_order, None, client_id)?;
        self.submit_order(no_order, None, client_id)?;

        log::info!(
            "ARB SUBMITTED | {} | side={arb_side} | profit={profit_bps:.1}bps | \
             yes={yes_order_id} no={no_order_id}",
            pair.label,
        );

        let exec = ArbExecution {
            state: ArbState::PendingEntry,
            arb_side,
            yes_order_id,
            no_order_id,
            yes_filled_qty: Decimal::ZERO,
            no_filled_qty: Decimal::ZERO,
            unwind_order_id: None,
        };

        let pair_key = pair.condition_id.clone();
        self.order_to_pair.insert(yes_order_id, pair_key.clone());
        self.order_to_pair.insert(no_order_id, pair_key.clone());
        self.arb_executions.insert(pair_key, exec);
        self.arbs_submitted += 1;

        Ok(())
    }

    /// Submit an IOC unwind order to exit a partially-filled position.
    fn submit_unwind(
        &mut self,
        pair_key: &str,
        instrument_id: InstrumentId,
        filled_qty: Decimal,
        arb_side: OrderSide,
    ) -> anyhow::Result<()> {
        let unwind_side = match arb_side {
            OrderSide::Buy => OrderSide::Sell,
            _ => OrderSide::Buy,
        };

        // Get current quote for aggressive pricing
        let quote = self
            .quotes
            .get(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("No quote for unwind instrument {instrument_id}"))?;

        let slippage_bps = self.config.unwind_slippage_bps;
        let slippage_mult = slippage_bps / Self::BPS_DIVISOR;

        let aggressive_price = match unwind_side {
            OrderSide::Sell => {
                // Selling back: price at bid minus slippage
                let bid = quote.bid_price.as_decimal();
                bid * (Decimal::ONE - slippage_mult)
            }
            _ => {
                // Buying back: price at ask plus slippage
                let ask = quote.ask_price.as_decimal();
                ask * (Decimal::ONE + slippage_mult)
            }
        };

        let price = Price::from_decimal(aggressive_price)?;
        let quantity = Quantity::from_decimal(filled_qty)?;

        let order = self.core.order_factory().limit(
            instrument_id,
            unwind_side,
            quantity,
            price,
            Some(TimeInForce::Ioc),
            None,
            Some(false), // not post_only for IOC
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![Ustr::from("arb-unwind")]),
            None,
        );

        let unwind_order_id = order.client_order_id();

        let client_id = self.config.client_id;
        self.submit_order(order, None, client_id)?;

        log::warn!(
            "UNWIND SUBMITTED | pair={pair_key} | {instrument_id} | \
             side={unwind_side} qty={filled_qty} price={aggressive_price:.4} | id={unwind_order_id}",
        );

        // Update execution state
        if let Some(exec) = self.arb_executions.get_mut(pair_key) {
            exec.state = ArbState::Unwinding;
            exec.unwind_order_id = Some(unwind_order_id);
        }

        self.order_to_pair
            .insert(unwind_order_id, pair_key.to_string());

        Ok(())
    }

    /// Handle a fill event — update fill tracking and drive state transitions.
    pub(super) fn handle_fill(&mut self, event: &OrderFilled) {
        let pair_key = match self.order_to_pair.get(&event.client_order_id) {
            Some(key) => key.clone(),
            None => return,
        };

        let fill_qty = event.last_qty.as_decimal();
        let fill_px = event.last_px.as_decimal();

        // Extract info we need before mutating, to avoid borrow conflicts
        let (is_unwind, is_yes, yes_order_id, no_order_id) = {
            let exec = match self.arb_executions.get(&pair_key) {
                Some(e) => e,
                None => return,
            };
            (
                exec.unwind_order_id == Some(event.client_order_id),
                exec.yes_order_id == event.client_order_id,
                exec.yes_order_id,
                exec.no_order_id,
            )
        };

        // Handle unwind order fill
        if is_unwind {
            let closed = {
                let cache = self.cache();
                cache
                    .order(&event.client_order_id)
                    .is_some_and(|o| o.is_closed())
            };

            if closed {
                log::info!("UNWIND COMPLETE | {pair_key} | filled @ {fill_px:.4}",);
                self.arbs_unwound += 1;
                self.cleanup_arb(&pair_key);
            }
            return;
        }

        // Entry leg fill — update tracking
        {
            let exec = match self.arb_executions.get_mut(&pair_key) {
                Some(e) => e,
                None => return,
            };

            if is_yes {
                exec.yes_filled_qty += fill_qty;
            } else {
                exec.no_filled_qty += fill_qty;
            }
        } // drop mutable borrow

        // Check if this order is fully closed
        let this_closed = {
            let cache = self.cache();
            cache
                .order(&event.client_order_id)
                .is_some_and(|o| o.is_closed())
        };

        if !this_closed {
            return; // partial fill on this order, wait for more
        }

        // This order is fully filled — check the other leg
        let other_id = if is_yes { no_order_id } else { yes_order_id };

        let other_closed = {
            let cache = self.cache();
            cache.order(&other_id).is_some_and(|o| o.is_closed())
        };

        let exec = match self.arb_executions.get(&pair_key) {
            Some(e) => e,
            None => return,
        };

        if other_closed {
            // Both legs closed and we have fills on both → arb complete
            if exec.yes_filled_qty > Decimal::ZERO && exec.no_filled_qty > Decimal::ZERO {
                // Read VWAP fill prices from the framework's order state (the cache
                // accumulates `avg_px` across all `OrderFilled` events for each order).
                let (yes_avg_px, no_avg_px) = {
                    let cache = self.cache();
                    let yes_px = cache
                        .order(&yes_order_id)
                        .and_then(|o| o.avg_px())
                        .unwrap_or(0.0);
                    let no_px = cache
                        .order(&no_order_id)
                        .and_then(|o| o.avg_px())
                        .unwrap_or(0.0);
                    (yes_px, no_px)
                };
                log::info!(
                    "ARB COMPLETE | {pair_key} | yes_px={yes_avg_px:.4} no_px={no_avg_px:.4}",
                );
                self.arbs_completed += 1;
                self.cleanup_arb(&pair_key);
            } else {
                self.arbs_failed += 1;
                self.cleanup_arb(&pair_key);
            }
        } else {
            // This leg filled, other still open → PartialFill
            if let Some(exec) = self.arb_executions.get_mut(&pair_key)
                && exec.state == ArbState::PendingEntry
            {
                log::warn!("PARTIAL FILL | {pair_key} | one leg closed, other still open",);
                exec.state = ArbState::PartialFill;
            }
        }
    }

    /// Handle a terminal (non-fill) order event: rejected, expired, or canceled.
    pub(super) fn handle_order_terminal(&mut self, client_order_id: ClientOrderId, reason: &str) {
        let pair_key = match self.order_to_pair.get(&client_order_id) {
            Some(key) => key.clone(),
            None => return,
        };

        let exec = match self.arb_executions.get(&pair_key) {
            Some(e) => e.clone(),
            None => return,
        };

        match exec.state {
            ArbState::PendingEntry => {
                // Determine which leg this is
                let is_yes = exec.yes_order_id == client_order_id;
                let (this_filled, other_filled) = if is_yes {
                    (exec.yes_filled_qty, exec.no_filled_qty)
                } else {
                    (exec.no_filled_qty, exec.yes_filled_qty)
                };
                let other_id = if is_yes {
                    exec.no_order_id
                } else {
                    exec.yes_order_id
                };

                // Check if the other leg is also closed
                let other_closed = {
                    let cache = self.cache();
                    cache.order(&other_id).is_some_and(|o| o.is_closed())
                };

                if other_closed {
                    // Both legs closed
                    if this_filled > Decimal::ZERO || other_filled > Decimal::ZERO {
                        // One side has fills → need to unwind
                        self.initiate_unwind(&pair_key);
                    } else {
                        // Neither side filled → clean failure
                        log::info!("ARB FAILED | {pair_key} | both legs {reason} with no fills",);
                        self.arbs_failed += 1;
                        self.cleanup_arb(&pair_key);
                    }
                } else if this_filled > Decimal::ZERO {
                    // This leg had fills but terminated, other still open
                    // → cancel other leg and unwind our fills
                    let order_to_cancel = {
                        let cache = self.cache();
                        cache.order(&other_id).cloned()
                    };

                    if let Some(order) = order_to_cancel
                        && !order.is_closed()
                        && let Err(e) = self.cancel_order(order, self.config.client_id)
                    {
                        log::error!("Failed to cancel other leg {other_id}: {e}",);
                    }
                    self.initiate_unwind(&pair_key);
                } else {
                    // This leg failed with no fills, other still open → cancel the other
                    log::info!("ARB LEG {reason} | {pair_key} | canceling other leg {other_id}",);
                    let order_to_cancel = {
                        let cache = self.cache();
                        cache.order(&other_id).cloned()
                    };

                    if let Some(order) = order_to_cancel
                        && !order.is_closed()
                        && let Err(e) = self.cancel_order(order, self.config.client_id)
                    {
                        log::error!("Failed to cancel other leg {other_id}: {e}",);
                    }
                }
            }
            ArbState::PartialFill => {
                // The pending leg just failed → unwind the filled leg
                self.initiate_unwind(&pair_key);
            }
            ArbState::Unwinding => {
                // Only react to the unwind order itself failing
                if exec.unwind_order_id == Some(client_order_id) {
                    log::error!(
                        "UNWIND FAILED | {pair_key} | unwind order {client_order_id} {reason} — \
                         manual intervention required",
                    );
                    self.arbs_failed += 1;
                    self.cleanup_arb(&pair_key);
                }
                // Ignore terminal events for entry leg orders during unwind
            }
            ArbState::Idle => {}
        }
    }

    /// Determine which leg was filled and submit an unwind order for it.
    fn initiate_unwind(&mut self, pair_key: &str) {
        let exec = match self.arb_executions.get(pair_key) {
            Some(e) => e.clone(),
            None => return,
        };

        let pair = match self.pairs.get(pair_key) {
            Some(p) => p.clone(),
            None => {
                log::error!("UNWIND | {pair_key} | pair not found, cannot unwind");
                self.arbs_failed += 1;
                self.cleanup_arb(pair_key);
                return;
            }
        };

        // Determine which leg has fills
        let (instrument_id, filled_qty) = if exec.yes_filled_qty > Decimal::ZERO {
            (pair.yes_id, exec.yes_filled_qty)
        } else if exec.no_filled_qty > Decimal::ZERO {
            (pair.no_id, exec.no_filled_qty)
        } else {
            // No fills on either side — nothing to unwind
            log::info!("ARB FAILED | {pair_key} | no fills to unwind");
            self.arbs_failed += 1;
            self.cleanup_arb(pair_key);
            return;
        };

        if let Err(e) = self.submit_unwind(pair_key, instrument_id, filled_qty, exec.arb_side) {
            log::error!(
                "UNWIND FAILED | {pair_key} | submit error: {e} — manual intervention required",
            );
            self.arbs_failed += 1;
            self.cleanup_arb(pair_key);
        }
    }

    /// Remove all tracking state for a completed/failed arb.
    pub(super) fn cleanup_arb(&mut self, pair_key: &str) {
        if let Some(exec) = self.arb_executions.remove(pair_key) {
            self.order_to_pair.remove(&exec.yes_order_id);
            self.order_to_pair.remove(&exec.no_order_id);
            if let Some(unwind_id) = exec.unwind_order_id {
                self.order_to_pair.remove(&unwind_id);
            }
        }
    }

    /// Check for buy arb: buy Yes + buy No when combined ask < 1.0.
    pub(super) fn check_buy_arb(&mut self, pair: &ComplementPair) -> bool {
        let (yes_quote, no_quote) =
            match (self.quotes.get(&pair.yes_id), self.quotes.get(&pair.no_id)) {
                (Some(y), Some(n)) => (y, n),
                _ => return false,
            };

        let trade_size = self.config.trade_size;
        let fee_bps = self.config.fee_estimate_bps;
        let min_profit = self.config.min_profit_bps;

        let yes_ask = yes_quote.ask_price.as_decimal();
        let no_ask = no_quote.ask_price.as_decimal();
        let combined_ask = yes_ask + no_ask;

        // Track tightest buy spread
        if combined_ask < self.best_buy_spread {
            self.best_buy_spread = combined_ask;
            self.best_buy_label = pair.label.clone();
        }

        if combined_ask >= Decimal::ONE {
            return false;
        }

        let fee = Self::leg_fee(fee_bps, yes_ask) + Self::leg_fee(fee_bps, no_ask);
        let profit_bps = (Decimal::ONE - combined_ask - fee) * Self::BPS_DIVISOR;

        if profit_bps < min_profit {
            return false;
        }

        let profit_abs = (Decimal::ONE - combined_ask - fee) * trade_size;

        if profit_abs < self.config.min_profit_abs {
            return false;
        }

        self.buy_arbs_detected += 1;

        log::info!(
            "BUY ARB | {} | profit={profit_bps:.1}bps | \
             yes_ask={yes_ask:.3} + no_ask={no_ask:.3} = {combined_ask:.3} | fee={fee:.4} | $profit={profit_abs:.4}",
            pair.label,
        );

        // Check liquidity
        let yes_liq = yes_quote.ask_size.as_decimal();
        let no_liq = no_quote.ask_size.as_decimal();

        if yes_liq < trade_size || no_liq < trade_size {
            log::info!(
                "BUY ARB | {} | skipped: insufficient liquidity \
                 yes={yes_liq} no={no_liq} need={trade_size}",
                pair.label,
            );
            return false;
        }

        // Capture prices before mutable borrow
        let yes_price = yes_quote.ask_price;
        let no_price = no_quote.ask_price;

        if let Err(e) = self.submit_arb(pair, OrderSide::Buy, yes_price, no_price, profit_bps) {
            log::error!("BUY ARB | {} | submit failed: {e}", pair.label);
        }

        true
    }

    /// Check for sell arb: sell Yes + sell No when combined bid > 1.0.
    pub(super) fn check_sell_arb(&mut self, pair: &ComplementPair) -> bool {
        let (yes_quote, no_quote) =
            match (self.quotes.get(&pair.yes_id), self.quotes.get(&pair.no_id)) {
                (Some(y), Some(n)) => (y, n),
                _ => return false,
            };

        let trade_size = self.config.trade_size;
        let fee_bps = self.config.fee_estimate_bps;
        let min_profit = self.config.min_profit_bps;

        let yes_bid = yes_quote.bid_price.as_decimal();
        let no_bid = no_quote.bid_price.as_decimal();
        let combined_bid = yes_bid + no_bid;

        // Track tightest sell spread
        if combined_bid > self.best_sell_spread {
            self.best_sell_spread = combined_bid;
            self.best_sell_label = pair.label.clone();
        }

        if combined_bid <= Decimal::ONE {
            return false;
        }

        let fee = Self::leg_fee(fee_bps, yes_bid) + Self::leg_fee(fee_bps, no_bid);
        let profit_bps = (combined_bid - fee - Decimal::ONE) * Self::BPS_DIVISOR;

        if profit_bps < min_profit {
            return false;
        }

        let profit_abs = (combined_bid - fee - Decimal::ONE) * trade_size;

        if profit_abs < self.config.min_profit_abs {
            return false;
        }

        self.sell_arbs_detected += 1;

        log::info!(
            "SELL ARB | {} | profit={profit_bps:.1}bps | \
             yes_bid={yes_bid:.3} + no_bid={no_bid:.3} = {combined_bid:.3} | fee={fee:.4} | $profit={profit_abs:.4}",
            pair.label,
        );

        // Check liquidity
        let yes_liq = yes_quote.bid_size.as_decimal();
        let no_liq = no_quote.bid_size.as_decimal();

        if yes_liq < trade_size || no_liq < trade_size {
            log::info!(
                "SELL ARB | {} | skipped: insufficient liquidity \
                 yes={yes_liq} no={no_liq} need={trade_size}",
                pair.label,
            );
            return false;
        }

        // Capture prices before mutable borrow
        let yes_price = yes_quote.bid_price;
        let no_price = no_quote.bid_price;

        if let Err(e) = self.submit_arb(pair, OrderSide::Sell, yes_price, no_price, profit_bps) {
            log::error!("SELL ARB | {} | submit failed: {e}", pair.label);
        }

        true
    }

    pub(super) fn log_diagnostic_summary(&self) {
        log::info!(
            "SUMMARY | quotes={} buy_arbs={} sell_arbs={} \
             submitted={} completed={} unwound={} failed={} | \
             best_buy_spread={:.4} ({}) best_sell_spread={:.4} ({})",
            self.quotes_processed,
            self.buy_arbs_detected,
            self.sell_arbs_detected,
            self.arbs_submitted,
            self.arbs_completed,
            self.arbs_unwound,
            self.arbs_failed,
            self.best_buy_spread,
            self.best_buy_label,
            self.best_sell_spread,
            self.best_sell_label,
        );
    }
}

nautilus_strategy!(ComplementArb, {
    fn on_order_rejected(&mut self, event: OrderRejected) {
        log::warn!(
            "Order rejected: {} — {}",
            event.client_order_id,
            event.reason,
        );
        self.handle_order_terminal(event.client_order_id, "rejected");
    }
    fn on_order_expired(&mut self, event: OrderExpired) {
        self.handle_order_terminal(event.client_order_id, "expired");
    }
});

impl Debug for ComplementArb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ComplementArb))
            .field("pairs", &self.pairs.len())
            .field("buy_arbs_detected", &self.buy_arbs_detected)
            .field("sell_arbs_detected", &self.sell_arbs_detected)
            .field("arbs_submitted", &self.arbs_submitted)
            .field("arbs_completed", &self.arbs_completed)
            .finish()
    }
}

impl DataActor for ComplementArb {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.discover_pairs();

        let instrument_ids: Vec<InstrumentId> = self
            .pairs
            .values()
            .flat_map(|p| [p.yes_id, p.no_id])
            .collect();

        let client_id = self.config.client_id;
        for id in instrument_ids {
            self.subscribe_quotes(id, client_id, None);
        }

        // Subscribe to new instruments arriving on this venue (e.g. new sport markets)
        self.subscribe_instruments(self.config.venue, client_id, None);

        log::info!(
            "ComplementArb started: {} pairs, trade_size={}, min_profit={}bps, fee={}bps, live={}",
            self.pairs.len(),
            self.config.trade_size,
            self.config.min_profit_bps,
            self.config.fee_estimate_bps,
            self.config.live_trading,
        );

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        // Cancel all active arb orders
        let active_order_ids: Vec<ClientOrderId> = self
            .arb_executions
            .values()
            .flat_map(|exec| {
                let mut ids = vec![exec.yes_order_id, exec.no_order_id];
                if let Some(unwind_id) = exec.unwind_order_id {
                    ids.push(unwind_id);
                }
                ids
            })
            .collect();

        let client_id = self.config.client_id;
        for order_id in &active_order_ids {
            let order_to_cancel = {
                let cache = self.cache();
                cache.order(order_id).cloned()
            };

            if let Some(order) = order_to_cancel
                && !order.is_closed()
                && let Err(e) = self.cancel_order(order, client_id)
            {
                log::error!("Failed to cancel order {order_id} on stop: {e}");
            }
        }

        log::info!(
            "ComplementArb stopped: buy_arbs={} sell_arbs={} \
             submitted={} completed={} unwound={} failed={}",
            self.buy_arbs_detected,
            self.sell_arbs_detected,
            self.arbs_submitted,
            self.arbs_completed,
            self.arbs_unwound,
            self.arbs_failed,
        );
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let id = instrument.id();
        log::info!("INSTRUMENT RECEIVED | {id}");

        // Skip instruments we already track
        if self.instrument_to_pair.contains_key(&id) {
            return Ok(());
        }

        if let InstrumentAny::BinaryOption(opt) = instrument {
            let outcome = opt.outcome.map(|o| o.to_string()).unwrap_or_default();
            let description = opt
                .description
                .map_or_else(|| id.to_string(), |d| d.to_string());

            log::info!("NEW INSTRUMENT | {description} | outcome={outcome} | {id}",);

            if let Some(pair) = self.try_match_complement(id, &outcome, &description) {
                let client_id = self.config.client_id;
                self.subscribe_quotes(pair.yes_id, client_id, None);
                self.subscribe_quotes(pair.no_id, client_id, None);

                log::info!(
                    "Paired new complement: {} (total: {})",
                    pair.label,
                    self.pairs.len(),
                );
            }
        }

        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        let instrument_id = quote.instrument_id;
        self.quotes.insert(instrument_id, *quote);

        // Look up which pair this instrument belongs to
        let pair_key = match self.instrument_to_pair.get(&instrument_id) {
            Some(key) => key.clone(),
            None => return Ok(()),
        };

        let pair = match self.pairs.get(&pair_key) {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        self.quotes_processed += 1;

        // Skip arb checks if pair has an active execution
        if !self.arb_executions.contains_key(&pair_key) {
            self.check_buy_arb(&pair);
            self.check_sell_arb(&pair);
        }

        // Periodic diagnostic summary
        if self.quotes_processed.is_multiple_of(500) {
            self.log_diagnostic_summary();
        }

        Ok(())
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        log::info!(
            "Order filled: {} {} @ {}",
            event.client_order_id,
            event.order_side,
            event.last_px,
        );
        self.handle_fill(event);
        Ok(())
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        self.handle_order_terminal(event.client_order_id, "canceled");
        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.pairs.clear();
        self.instrument_to_pair.clear();
        self.pending_complements.clear();
        self.quotes.clear();
        self.arb_executions.clear();
        self.order_to_pair.clear();
        self.quotes_processed = 0;
        self.buy_arbs_detected = 0;
        self.sell_arbs_detected = 0;
        self.arbs_submitted = 0;
        self.arbs_completed = 0;
        self.arbs_unwound = 0;
        self.arbs_failed = 0;
        self.best_buy_spread = Decimal::TWO;
        self.best_sell_spread = Decimal::ZERO;
        self.best_buy_label.clear();
        self.best_sell_label.clear();
        Ok(())
    }
}
