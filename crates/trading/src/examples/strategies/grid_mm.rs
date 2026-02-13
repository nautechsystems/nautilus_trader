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
    enums::OrderSide,
    identifiers::{InstrumentId, StrategyId},
    instruments::Instrument,
    orders::Order,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::strategy::{Strategy, StrategyConfig, StrategyCore};

/// Grid market making strategy with inventory-based skewing.
///
/// Places a symmetric grid of limit buy and sell orders around the mid-price.
/// Orders persist across ticks and are only replaced when the mid-price moves
/// by at least `requote_threshold`. The grid is shifted by a skew proportional
/// to the current net position to discourage inventory buildup.
pub struct GridMarketMaker {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    num_levels: usize,
    grid_interval: f64,
    skew_factor: f64,
    max_position: Quantity,
    requote_threshold: f64,
    price_precision: u8,
    last_quoted_mid: Option<Price>,
}

impl GridMarketMaker {
    /// Creates a new [`GridMarketMaker`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        num_levels: usize,
        grid_interval: f64,
        skew_factor: f64,
        max_position: Quantity,
        requote_threshold: f64,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("GRID_MM-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            num_levels,
            grid_interval,
            skew_factor,
            max_position,
            requote_threshold,
            price_precision: 0,
            last_quoted_mid: None,
        }
    }

    fn should_requote(&self, mid: Price) -> bool {
        match self.last_quoted_mid {
            Some(last_mid) => (mid.as_f64() - last_mid.as_f64()).abs() >= self.requote_threshold,
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
        let skew = Price::new(self.skew_factor * net_position, precision);
        let trade_size = self.trade_size.as_decimal();
        let max_pos = self.max_position.as_decimal();
        let mut projected_long = worst_long;
        let mut projected_short = worst_short;
        let mut orders = Vec::new();

        for level in 1..=self.num_levels {
            let offset = Price::new(level as f64 * self.grid_interval, precision);

            if projected_long + trade_size <= max_pos {
                orders.push((OrderSide::Buy, mid - offset - skew));
                projected_long += trade_size;
            }

            if projected_short - trade_size >= -max_pos {
                orders.push((OrderSide::Sell, mid + offset - skew));
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
            .field("instrument_id", &self.instrument_id)
            .field("trade_size", &self.trade_size)
            .field("num_levels", &self.num_levels)
            .field("grid_interval", &self.grid_interval)
            .field("skew_factor", &self.skew_factor)
            .field("max_position", &self.max_position)
            .field("requote_threshold", &self.requote_threshold)
            .finish()
    }
}

impl DataActor for GridMarketMaker {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let price_precision = {
            let cache = self.cache();
            cache
                .instrument(&self.instrument_id)
                .expect("Instrument should be in cache")
                .price_precision()
        };
        self.price_precision = price_precision;

        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.cancel_all_orders(self.instrument_id, None, None)?;
        self.close_all_positions(self.instrument_id, None, None, None, None, None, None)?;
        self.unsubscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        // f64 division by 2 is exact in IEEE 754
        let mid_f64 = (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0;
        let mid = Price::new(mid_f64, self.price_precision);

        if !self.should_requote(mid) {
            return Ok(());
        }

        self.cancel_all_orders(self.instrument_id, None, None)?;

        // Compute worst-case per-side exposure for max_position checks,
        // since cancels are async and pending orders may still fill
        let (net_position, worst_long, worst_short) = {
            let strategy_id = StrategyId::from(self.actor_id.inner().as_str());
            let instrument_id = Some(&self.instrument_id);
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

        let instrument_id = self.instrument_id;
        let trade_size = self.trade_size;

        for (side, price) in grid {
            let order = self.core.order_factory().limit(
                instrument_id,
                side,
                trade_size,
                price,
                None,
                None,
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
    use nautilus_model::{
        enums::OrderSide,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::GridMarketMaker;

    const PRECISION: u8 = 2;

    fn create_strategy(
        num_levels: usize,
        grid_interval: f64,
        skew_factor: f64,
        max_position: Quantity,
        requote_threshold: f64,
    ) -> GridMarketMaker {
        let mut strategy = GridMarketMaker::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Quantity::from("0.100"),
            num_levels,
            grid_interval,
            skew_factor,
            max_position,
            requote_threshold,
        );
        strategy.price_precision = PRECISION;
        strategy
    }

    fn mid(value: &str) -> Price {
        Price::new(value.parse::<f64>().unwrap(), PRECISION)
    }

    #[rstest]
    fn test_should_requote_true_when_no_previous_quote() {
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        assert!(strategy.should_requote(mid("1000.00")));
    }

    #[rstest]
    fn test_should_requote_false_within_threshold() {
        let mut strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(!strategy.should_requote(mid("1000.30")));
    }

    #[rstest]
    fn test_should_requote_true_at_threshold() {
        let mut strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(strategy.should_requote(mid("1000.50")));
    }

    #[rstest]
    fn test_should_requote_true_beyond_threshold_negative() {
        let mut strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        strategy.last_quoted_mid = Some(mid("1000.00"));
        assert!(strategy.should_requote(mid("999.40")));
    }

    #[rstest]
    fn test_grid_orders_flat_position_symmetric() {
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
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

        // Buy prices descend from mid
        assert_eq!(buys[0].1, mid("999.00"));
        assert_eq!(buys[1].1, mid("998.00"));
        assert_eq!(buys[2].1, mid("997.00"));

        // Sell prices ascend from mid
        assert_eq!(sells[0].1, mid("1001.00"));
        assert_eq!(sells[1].1, mid("1002.00"));
        assert_eq!(sells[2].1, mid("1003.00"));
    }

    #[rstest]
    fn test_grid_orders_skew_shifts_prices() {
        // skew_factor=1.0, net_position=2.0 → skew=2.0
        let strategy = create_strategy(1, 5.0, 1.0, Quantity::from("10.0"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), 2.0, dec!(2), dec!(2));

        assert_eq!(orders.len(), 2);
        // Buy: 1000 - 5.0 - 2.0 = 993.0
        assert_eq!(orders[0], (OrderSide::Buy, mid("993.00")));
        // Sell: 1000 + 5.0 - 2.0 = 1003.0
        assert_eq!(orders[1], (OrderSide::Sell, mid("1003.00")));
    }

    fn count_side(orders: &[(OrderSide, Price)], side: OrderSide) -> usize {
        orders.iter().filter(|(s, _)| *s == side).count()
    }

    #[rstest]
    fn test_grid_orders_max_position_limits_buy_levels() {
        // net_position=9.9, trade_size=0.1, max=10.0 → only 1 buy level fits
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), 9.9, dec!(9.9), dec!(9.9));

        assert_eq!(count_side(&orders, OrderSide::Buy), 1);
        assert_eq!(count_side(&orders, OrderSide::Sell), 3);
    }

    #[rstest]
    fn test_grid_orders_max_position_limits_sell_levels() {
        // net_position=-9.9, trade_size=0.1, max=10.0 → only 1 sell level fits
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), -9.9, dec!(-9.9), dec!(-9.9));

        assert_eq!(count_side(&orders, OrderSide::Buy), 3);
        assert_eq!(count_side(&orders, OrderSide::Sell), 1);
    }

    #[rstest]
    fn test_grid_orders_max_position_blocks_all_buys() {
        // net_position=10.0 (at max) → no buys, all sells
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("10.0"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), 10.0, dec!(10), dec!(10));

        assert_eq!(count_side(&orders, OrderSide::Buy), 0);
        assert_eq!(count_side(&orders, OrderSide::Sell), 3);
    }

    #[rstest]
    fn test_grid_orders_projected_exposure_across_levels() {
        // max_position=0.15, trade_size=0.1, 3 levels → only 1 level fits per side
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("0.150"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));

        assert_eq!(count_side(&orders, OrderSide::Buy), 1);
        assert_eq!(count_side(&orders, OrderSide::Sell), 1);
    }

    #[rstest]
    fn test_grid_orders_empty_when_fully_constrained() {
        // max_position=0.05, trade_size=0.1 → nothing fits
        let strategy = create_strategy(3, 1.0, 0.0, Quantity::from("0.050"), 0.50);
        let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));
        assert!(orders.is_empty());
    }
}
