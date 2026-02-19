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

//! Grid market making strategy with inventory-based skewing.
//!
//! Subscribes to quotes for a single instrument and maintains a symmetric grid
//! of limit orders around the mid-price. Orders are only replaced when the
//! mid-price moves beyond a configurable threshold, allowing resting orders to
//! persist across ticks. The grid is shifted by a skew proportional to the
//! current net position to discourage inventory buildup (Avellaneda-Stoikov
//! inspired).

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use ahash::AHashSet;
use nautilus_common::actor::{DataActor, DataActorCore};
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, TimeInForce},
    events::{OrderCanceled, OrderFilled},
    identifiers::{ClientOrderId, InstrumentId, StrategyId},
    instruments::Instrument,
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::strategy::{Strategy, StrategyConfig, StrategyCore};

/// Configuration for the grid market making strategy.
#[derive(Debug, Clone)]
pub struct GridMarketMakerConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Instrument ID to trade.
    pub instrument_id: InstrumentId,
    /// Trade size per grid level. When `None` the strategy resolves it from
    /// the instrument's `min_quantity` during `on_start`.
    pub trade_size: Option<Quantity>,
    /// Number of price levels on each side (buy & sell).
    pub num_levels: usize,
    /// Grid spacing in basis points of mid-price (geometric grid).
    /// E.g. `10` = 10 bps = 0.1%. Buy level N = mid × (1 - bps/10000)^N.
    pub grid_step_bps: u32,
    /// How aggressively to shift the grid based on inventory.
    pub skew_factor: f64,
    /// Hard cap on net exposure (long or short).
    pub max_position: Quantity,
    /// Minimum mid-price move in basis points before re-quoting.
    /// E.g. `5` = 5 bps = 0.05%.
    pub requote_threshold_bps: u32,
    /// Optional order expiry in seconds. When set, orders use GTD
    /// time-in-force with `expire_time = now + expire_time_secs`.
    pub expire_time_secs: Option<u64>,
    /// When `true`, resubmit the full grid on the next quote after receiving
    /// an order cancel event. Useful for exchanges like dYdX where short-term
    /// orders are canceled by the protocol after expiry.
    pub on_cancel_resubmit: bool,
}

impl GridMarketMakerConfig {
    /// Creates a new [`GridMarketMakerConfig`] with required fields and sensible defaults.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, max_position: Quantity) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(StrategyId::from("GRID_MM-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            },
            instrument_id,
            trade_size: None,
            num_levels: 3,
            grid_step_bps: 10,
            skew_factor: 0.0,
            max_position,
            requote_threshold_bps: 5,
            expire_time_secs: None,
            on_cancel_resubmit: false,
        }
    }

    #[must_use]
    pub fn with_trade_size(mut self, trade_size: Quantity) -> Self {
        self.trade_size = Some(trade_size);
        self
    }

    #[must_use]
    pub fn with_num_levels(mut self, num_levels: usize) -> Self {
        self.num_levels = num_levels;
        self
    }

    #[must_use]
    pub fn with_grid_step_bps(mut self, bps: u32) -> Self {
        self.grid_step_bps = bps;
        self
    }

    #[must_use]
    pub fn with_skew_factor(mut self, skew_factor: f64) -> Self {
        self.skew_factor = skew_factor;
        self
    }

    #[must_use]
    pub fn with_requote_threshold_bps(mut self, bps: u32) -> Self {
        self.requote_threshold_bps = bps;
        self
    }

    #[must_use]
    pub fn with_expire_time_secs(mut self, secs: u64) -> Self {
        self.expire_time_secs = Some(secs);
        self
    }

    #[must_use]
    pub fn with_on_cancel_resubmit(mut self, enabled: bool) -> Self {
        self.on_cancel_resubmit = enabled;
        self
    }

    #[must_use]
    pub fn with_strategy_id(mut self, strategy_id: StrategyId) -> Self {
        self.base.strategy_id = Some(strategy_id);
        self
    }

    #[must_use]
    pub fn with_order_id_tag(mut self, tag: String) -> Self {
        self.base.order_id_tag = Some(tag);
        self
    }
}

/// Grid market making strategy with inventory-based skewing.
///
/// Places a symmetric grid of limit buy and sell orders around the mid-price.
/// Orders persist across ticks and are only replaced when the mid-price moves
/// by at least `requote_threshold_bps`. The grid is shifted by a skew proportional
/// to the current net position to discourage inventory buildup.
pub struct GridMarketMaker {
    core: StrategyCore,
    config: GridMarketMakerConfig,
    trade_size: Option<Quantity>,
    price_precision: u8,
    last_quoted_mid: Option<Price>,
    pending_self_cancels: AHashSet<ClientOrderId>,
}

impl GridMarketMaker {
    /// Creates a new [`GridMarketMaker`] instance from config.
    #[must_use]
    pub fn new(config: GridMarketMakerConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base.clone()),
            trade_size: config.trade_size,
            config,
            price_precision: 0,
            last_quoted_mid: None,
            pending_self_cancels: AHashSet::new(),
        }
    }

    fn should_requote(&self, mid: Price) -> bool {
        match self.last_quoted_mid {
            Some(last_mid) => {
                let last_f64 = last_mid.as_f64();
                if last_f64 == 0.0 {
                    return true;
                }
                let threshold = self.config.requote_threshold_bps as f64 / 10_000.0;
                (mid.as_f64() - last_f64).abs() / last_f64 >= threshold
            }
            None => true,
        }
    }

    // Computes grid order prices and sides, respecting projected
    // position limits across all levels.
    //
    // `net_position` drives skew pricing. `worst_long`/`worst_short`
    // are the worst-case same-side exposures (positions + all pending
    // buy/sell orders) used for max_position enforcement.
    fn grid_orders(
        &self,
        mid: Price,
        net_position: f64,
        worst_long: Decimal,
        worst_short: Decimal,
    ) -> Vec<(OrderSide, Price)> {
        let precision = self.price_precision;
        let mid_f64 = mid.as_f64();
        let skew_f64 = self.config.skew_factor * net_position;
        let pct = self.config.grid_step_bps as f64 / 10_000.0;
        let trade_size = self
            .trade_size
            .expect("trade_size should be resolved in on_start")
            .as_decimal();
        let max_pos = self.config.max_position.as_decimal();
        let mut projected_long = worst_long;
        let mut projected_short = worst_short;
        let mut orders = Vec::new();

        for level in 1..=self.config.num_levels {
            let buy_price = Price::new(
                mid_f64 * (1.0 - pct).powi(level as i32) - skew_f64,
                precision,
            );
            let sell_price = Price::new(
                mid_f64 * (1.0 + pct).powi(level as i32) - skew_f64,
                precision,
            );

            if projected_long + trade_size <= max_pos {
                orders.push((OrderSide::Buy, buy_price));
                projected_long += trade_size;
            }

            if projected_short - trade_size >= -max_pos {
                orders.push((OrderSide::Sell, sell_price));
                projected_short -= trade_size;
            }
        }

        orders
    }
}

impl Deref for GridMarketMaker {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for GridMarketMaker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Debug for GridMarketMaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(GridMarketMaker))
            .field("config", &self.config)
            .field("trade_size", &self.trade_size)
            .finish()
    }
}

impl DataActor for GridMarketMaker {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        let (price_precision, size_precision, min_quantity) = {
            let cache = self.cache();
            let instrument = cache
                .instrument(&instrument_id)
                .expect("Instrument should be in cache");
            (
                instrument.price_precision(),
                instrument.size_precision(),
                instrument.min_quantity(),
            )
        };
        self.price_precision = price_precision;

        // Resolve trade_size from instrument when not explicitly provided
        if self.trade_size.is_none() {
            self.trade_size =
                Some(min_quantity.unwrap_or_else(|| Quantity::new(1.0, size_precision)));
        }

        self.subscribe_quotes(instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let instrument_id = self.config.instrument_id;
        self.cancel_all_orders(instrument_id, None, None)?;
        self.close_all_positions(instrument_id, None, None, None, None, None, None)?;
        self.unsubscribe_quotes(instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        // f64 division by 2 is exact in IEEE 754
        let mid_f64 = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
        let mid = Price::new(mid_f64, self.price_precision);

        if !self.should_requote(mid) {
            return Ok(());
        }

        let instrument_id = self.config.instrument_id;

        if self.config.on_cancel_resubmit {
            let strategy_id = StrategyId::from(self.actor_id.inner().as_str());
            let inst = Some(&instrument_id);
            let strategy = Some(&strategy_id);
            let ids: Vec<ClientOrderId> = {
                let cache = self.cache();
                cache
                    .orders_open(None, inst, strategy, None, None)
                    .iter()
                    .chain(
                        cache
                            .orders_inflight(None, inst, strategy, None, None)
                            .iter(),
                    )
                    .map(|o| o.client_order_id())
                    .collect()
            };
            self.pending_self_cancels.extend(ids);
        }

        self.cancel_all_orders(instrument_id, None, None)?;

        // Compute worst-case per-side exposure for max_position checks,
        // since cancels are async and pending orders may still fill
        let (net_position, worst_long, worst_short) = {
            let strategy_id = StrategyId::from(self.actor_id.inner().as_str());
            let instrument_id = Some(&self.config.instrument_id);
            let strategy = Some(&strategy_id);
            let cache = self.cache();

            let mut position_qty = 0.0_f64;
            let mut position_dec = Decimal::ZERO;
            for p in cache.positions_open(None, instrument_id, strategy, None, None) {
                position_qty += p.signed_qty;
                position_dec += p.quantity.as_decimal()
                    * if p.signed_qty < 0.0 {
                        Decimal::NEGATIVE_ONE
                    } else {
                        Decimal::ONE
                    };
            }

            let mut pending_buy_dec = Decimal::ZERO;
            let mut pending_sell_dec = Decimal::ZERO;
            let mut seen = AHashSet::new();

            // Deduplicate open/inflight (can overlap during state transitions)
            for order in cache
                .orders_open(None, instrument_id, strategy, None, None)
                .iter()
                .chain(
                    cache
                        .orders_inflight(None, instrument_id, strategy, None, None)
                        .iter(),
                )
            {
                if !seen.insert(order.client_order_id()) {
                    continue;
                }
                let qty = order.leaves_qty().as_decimal();
                match order.order_side() {
                    OrderSide::Buy => pending_buy_dec += qty,
                    _ => pending_sell_dec += qty,
                }
            }

            (
                position_qty,
                position_dec + pending_buy_dec,
                position_dec - pending_sell_dec,
            )
        };

        let grid = self.grid_orders(mid, net_position, worst_long, worst_short);

        // Don't advance the requote anchor when no orders are placed,
        // otherwise the strategy can stall with zero resting orders
        if grid.is_empty() {
            return Ok(());
        }

        let trade_size = self
            .trade_size
            .expect("trade_size should be resolved in on_start");

        // Compute time-in-force and expire time from config
        let (tif, expire_time) = match self.config.expire_time_secs {
            Some(secs) => {
                let now_ns = self.core.clock().timestamp_ns();
                let expire_ns = now_ns + secs * 1_000_000_000;
                (Some(TimeInForce::Gtd), Some(expire_ns))
            }
            None => (None, None),
        };

        for (side, price) in grid {
            let order = self.core.order_factory().limit(
                instrument_id,
                side,
                trade_size,
                price,
                tif,
                expire_time,
                Some(true), // post_only
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
            self.submit_order(order, None, None)?;
        }

        self.last_quoted_mid = Some(mid);
        Ok(())
    }

    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        self.pending_self_cancels.remove(&event.client_order_id);
        Ok(())
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) -> anyhow::Result<()> {
        if self.pending_self_cancels.remove(&event.client_order_id) {
            return Ok(());
        }

        if self.config.on_cancel_resubmit {
            // Reset so the next incoming quote triggers a full grid resubmission
            self.last_quoted_mid = None;
        }
        Ok(())
    }
}

impl Strategy for GridMarketMaker {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::actor::DataActor;
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::OrderSide,
        events::OrderCanceled,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::{GridMarketMaker, GridMarketMakerConfig};

    const PRECISION: u8 = 2;

    fn create_strategy(
        num_levels: usize,
        grid_step_bps: u32,
        skew_factor: f64,
        max_position: Quantity,
        requote_threshold_bps: u32,
    ) -> GridMarketMaker {
        let config =
            GridMarketMakerConfig::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), max_position)
                .with_trade_size(Quantity::from("0.100"))
                .with_num_levels(num_levels)
                .with_grid_step_bps(grid_step_bps)
                .with_skew_factor(skew_factor)
                .with_requote_threshold_bps(requote_threshold_bps);

        let mut strategy = GridMarketMaker::new(config);
        strategy.price_precision = PRECISION;
        strategy
    }

    fn mid(value: &str) -> Price {
        Price::new(value.parse::<f64>().unwrap(), PRECISION)
    }

    #[rstest]
    fn test_should_requote_true_when_no_previous_quote() {
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        assert!(strategy.should_requote(mid("1000.00")));
    }

    #[rstest]
    fn test_should_requote_false_within_threshold() {
        let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(!strategy.should_requote(mid("1000.30")));
    }

    #[rstest]
    fn test_should_requote_true_at_threshold() {
        let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(strategy.should_requote(mid("1000.50")));
    }

    #[rstest]
    fn test_should_requote_true_beyond_threshold_negative() {
        let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(strategy.should_requote(mid("999.40")));
    }

    #[rstest]
    fn test_grid_orders_flat_position_symmetric() {
        // 1% geometric grid: buy = mid × 0.99^level, sell = mid × 1.01^level
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));

        assert_eq!(orders.len(), 6);

        let buys: Vec<_> = orders
            .iter()
            .filter(|(s, _)| *s == OrderSide::Buy)
            .collect();
        let sells: Vec<_> = orders
            .iter()
            .filter(|(s, _)| *s == OrderSide::Sell)
            .collect();
        assert_eq!(buys.len(), 3);
        assert_eq!(sells.len(), 3);

        // Buy prices: 1000 × 0.99^1 = 990.00, 0.99^2 = 980.10, 0.99^3 = 970.30
        assert_eq!(buys[0].1, mid("990.00"));
        assert_eq!(buys[1].1, mid("980.10"));
        assert_eq!(buys[2].1, mid("970.30"));

        // Sell prices: 1000 × 1.01^1 = 1010.00, 1.01^2 = 1020.10, 1.01^3 = 1030.30
        assert_eq!(sells[0].1, mid("1010.00"));
        assert_eq!(sells[1].1, mid("1020.10"));
        assert_eq!(sells[2].1, mid("1030.30"));
    }

    #[rstest]
    fn test_grid_orders_skew_shifts_prices() {
        // 500 bps (5%) geometric grid, skew_factor=1.0, net_position=2.0 → skew_f64=2.0
        let strategy = create_strategy(1, 500, 1.0, Quantity::from("10.0"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 2.0, dec!(2), dec!(2));

        assert_eq!(orders.len(), 2);
        // Buy: 1000 × 0.95^1 - 2.0 = 950.0 - 2.0 = 948.0
        assert_eq!(orders[0], (OrderSide::Buy, mid("948.00")));
        // Sell: 1000 × 1.05^1 - 2.0 = 1050.0 - 2.0 = 1048.0
        assert_eq!(orders[1], (OrderSide::Sell, mid("1048.00")));
    }

    fn count_side(orders: &[(OrderSide, Price)], side: OrderSide) -> usize {
        orders.iter().filter(|(s, _)| *s == side).count()
    }

    #[rstest]
    fn test_grid_orders_max_position_limits_buy_levels() {
        // net_position=9.9, trade_size=0.1, max=10.0 → only 1 buy level fits
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 9.9, dec!(9.9), dec!(9.9));

        assert_eq!(count_side(&orders, OrderSide::Buy), 1);
        assert_eq!(count_side(&orders, OrderSide::Sell), 3);
    }

    #[rstest]
    fn test_grid_orders_max_position_limits_sell_levels() {
        // net_position=-9.9, trade_size=0.1, max=10.0 → only 1 sell level fits
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), -9.9, dec!(-9.9), dec!(-9.9));

        assert_eq!(count_side(&orders, OrderSide::Buy), 3);
        assert_eq!(count_side(&orders, OrderSide::Sell), 1);
    }

    #[rstest]
    fn test_grid_orders_max_position_blocks_all_buys() {
        // net_position=10.0 (at max) → no buys, all sells
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 10.0, dec!(10), dec!(10));

        assert_eq!(count_side(&orders, OrderSide::Buy), 0);
        assert_eq!(count_side(&orders, OrderSide::Sell), 3);
    }

    #[rstest]
    fn test_grid_orders_projected_exposure_across_levels() {
        // max_position=0.15, trade_size=0.1, 3 levels → only 1 level fits per side
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.150"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));

        assert_eq!(count_side(&orders, OrderSide::Buy), 1);
        assert_eq!(count_side(&orders, OrderSide::Sell), 1);
    }

    #[rstest]
    fn test_grid_orders_empty_when_fully_constrained() {
        // max_position=0.05, trade_size=0.1 → nothing fits
        let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.050"), 5);
        let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));
        assert!(orders.is_empty());
    }

    fn order_canceled(client_order_id: &str) -> OrderCanceled {
        OrderCanceled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("GRID_MM-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            ClientOrderId::from(client_order_id),
            UUID4::new(),
            0.into(),
            0.into(),
            false,
            None,
            None,
        )
    }

    fn create_cancel_resubmit_strategy() -> GridMarketMaker {
        let config = GridMarketMakerConfig::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Quantity::from("10.0"),
        )
        .with_trade_size(Quantity::from("0.100"))
        .with_on_cancel_resubmit(true);

        let mut strategy = GridMarketMaker::new(config);
        strategy.price_precision = PRECISION;
        strategy
    }

    #[rstest]
    fn test_on_order_canceled_self_cancel_preserves_last_quoted_mid() {
        let mut strategy = create_cancel_resubmit_strategy();
        strategy.last_quoted_mid = Some(mid("1000.00"));
        strategy
            .pending_self_cancels
            .insert(ClientOrderId::from("O-001"));

        let event = order_canceled("O-001");
        strategy.on_order_canceled(&event).unwrap();

        assert!(strategy.pending_self_cancels.is_empty());
        assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));
    }

    #[rstest]
    fn test_on_order_canceled_protocol_cancel_resets_last_quoted_mid() {
        // ID not in pending set → protocol-initiated cancel resets mid
        let mut strategy = create_cancel_resubmit_strategy();
        strategy.last_quoted_mid = Some(mid("1000.00"));

        let event = order_canceled("O-999");
        strategy.on_order_canceled(&event).unwrap();

        assert_eq!(strategy.last_quoted_mid, None);
    }

    #[rstest]
    fn test_on_order_canceled_self_cancel_then_protocol_cancel() {
        let mut strategy = create_cancel_resubmit_strategy();
        strategy.last_quoted_mid = Some(mid("1000.00"));
        strategy
            .pending_self_cancels
            .insert(ClientOrderId::from("O-001"));

        // Self-cancel consumed
        let self_event = order_canceled("O-001");
        strategy.on_order_canceled(&self_event).unwrap();
        assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));

        // Protocol cancel triggers reset
        let protocol_event = order_canceled("O-002");
        strategy.on_order_canceled(&protocol_event).unwrap();
        assert_eq!(strategy.last_quoted_mid, None);
    }

    #[rstest]
    fn test_on_order_canceled_filled_order_does_not_block_protocol_cancel() {
        // Order O-001 tracked as self-cancel but fills before cancel ack,
        // so O-002 (protocol cancel) must still trigger reset
        let mut strategy = create_cancel_resubmit_strategy();
        strategy.last_quoted_mid = Some(mid("1000.00"));
        strategy
            .pending_self_cancels
            .insert(ClientOrderId::from("O-001"));

        // O-001 filled (no cancel event) → O-002 is a protocol cancel
        let event = order_canceled("O-002");
        strategy.on_order_canceled(&event).unwrap();

        assert_eq!(strategy.last_quoted_mid, None);
    }

    #[rstest]
    fn test_on_order_canceled_without_resubmit_does_nothing() {
        // on_cancel_resubmit=false: cancel never resets mid
        let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
        strategy.last_quoted_mid = Some(mid("1000.00"));

        let event = order_canceled("O-001");
        strategy.on_order_canceled(&event).unwrap();

        assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));
    }
}
