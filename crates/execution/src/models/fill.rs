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

use std::fmt::Display;

use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_in_range_inclusive_f64},
};
use nautilus_model::{
    data::order::BookOrder,
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use rand::{RngExt, SeedableRng, rngs::StdRng};

pub trait FillModel {
    /// Returns `true` if a limit order should be filled based on the model.
    fn is_limit_filled(&mut self) -> bool;

    /// Returns `true` if an order fill should slip by one tick.
    fn is_slipped(&mut self) -> bool;

    /// Returns whether limit orders at or inside the spread are fillable.
    ///
    /// When true, the matching core treats a limit order as fillable if its
    /// price is at or better than the current best quote on its own side
    /// (BUY >= bid, SELL <= ask), not just when it crosses the spread.
    fn fill_limit_inside_spread(&self) -> bool {
        false
    }

    /// Returns a simulated `OrderBook` for fill simulation.
    ///
    /// Custom fill models provide their own liquidity simulation by returning an
    /// `OrderBook` that represents expected market liquidity. The matching engine
    /// uses this to determine fills.
    ///
    /// Returns `None` to use the matching engine's standard fill logic.
    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook>;
}

#[derive(Debug)]
pub struct ProbabilisticFillState {
    prob_fill_on_limit: f64,
    prob_slippage: f64,
    random_seed: Option<u64>,
    rng: StdRng,
}

impl ProbabilisticFillState {
    /// Creates a new [`ProbabilisticFillState`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    ///
    /// # Panics
    ///
    /// Panics if the range check assertions fail.
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(prob_fill_on_limit, 0.0, 1.0, "prob_fill_on_limit")
            .expect(FAILED);
        check_in_range_inclusive_f64(prob_slippage, 0.0, 1.0, "prob_slippage").expect(FAILED);
        let rng = match random_seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_rng(&mut rand::rng()),
        };
        Ok(Self {
            prob_fill_on_limit,
            prob_slippage,
            random_seed,
            rng,
        })
    }

    pub fn is_limit_filled(&mut self) -> bool {
        self.event_success(self.prob_fill_on_limit)
    }

    pub fn is_slipped(&mut self) -> bool {
        self.event_success(self.prob_slippage)
    }

    pub fn random_bool(&mut self, probability: f64) -> bool {
        self.event_success(probability)
    }

    fn event_success(&mut self, probability: f64) -> bool {
        match probability {
            0.0 => false,
            1.0 => true,
            _ => self.rng.random_bool(probability),
        }
    }
}

impl Clone for ProbabilisticFillState {
    fn clone(&self) -> Self {
        Self::new(
            self.prob_fill_on_limit,
            self.prob_slippage,
            self.random_seed,
        )
        .expect("ProbabilisticFillState clone should not fail with valid parameters")
    }
}

const UNLIMITED: u64 = 10_000_000_000;

fn build_l2_book(instrument_id: InstrumentId) -> OrderBook {
    OrderBook::new(instrument_id, BookType::L2_MBP)
}

fn add_order(book: &mut OrderBook, side: OrderSide, price: Price, size: Quantity, order_id: u64) {
    let order = BookOrder::new(side, price, size, order_id);
    book.add(order, 0, 0, UnixNanos::default());
}

#[derive(Debug)]
pub struct DefaultFillModel {
    state: ProbabilisticFillState,
}

impl DefaultFillModel {
    /// Creates a new [`DefaultFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for DefaultFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for DefaultFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl Display for DefaultFillModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DefaultFillModel(prob_fill_on_limit: {}, prob_slippage: {})",
            self.state.prob_fill_on_limit, self.state.prob_slippage
        )
    }
}

impl FillModel for DefaultFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        _instrument: &InstrumentAny,
        _order: &OrderAny,
        _best_bid: Price,
        _best_ask: Price,
    ) -> Option<OrderBook> {
        None
    }
}

/// Fill model that executes all orders at the best available price with unlimited liquidity.
#[derive(Debug)]
pub struct BestPriceFillModel {
    state: ProbabilisticFillState,
}

impl BestPriceFillModel {
    /// Creates a new [`BestPriceFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for BestPriceFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for BestPriceFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for BestPriceFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn fill_limit_inside_spread(&self) -> bool {
        true
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let mut book = build_l2_book(instrument.id());
        let size_prec = instrument.size_precision();
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(UNLIMITED as f64, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(UNLIMITED as f64, size_prec),
            2,
        );
        Some(book)
    }
}

/// Fill model that forces exactly one tick of slippage for all orders.
#[derive(Debug)]
pub struct OneTickSlippageFillModel {
    state: ProbabilisticFillState,
}

impl OneTickSlippageFillModel {
    /// Creates a new [`OneTickSlippageFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for OneTickSlippageFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for OneTickSlippageFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for OneTickSlippageFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            2,
        );
        Some(book)
    }
}

/// Fill model with 50/50 chance of best price fill or one tick slippage.
#[derive(Debug)]
pub struct ProbabilisticFillModel {
    state: ProbabilisticFillState,
}

impl ProbabilisticFillModel {
    /// Creates a new [`ProbabilisticFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for ProbabilisticFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for ProbabilisticFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for ProbabilisticFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        if self.state.random_bool(0.5) {
            add_order(
                &mut book,
                OrderSide::Buy,
                best_bid,
                Quantity::new(UNLIMITED as f64, size_prec),
                1,
            );
            add_order(
                &mut book,
                OrderSide::Sell,
                best_ask,
                Quantity::new(UNLIMITED as f64, size_prec),
                2,
            );
        } else {
            add_order(
                &mut book,
                OrderSide::Buy,
                best_bid - tick,
                Quantity::new(UNLIMITED as f64, size_prec),
                1,
            );
            add_order(
                &mut book,
                OrderSide::Sell,
                best_ask + tick,
                Quantity::new(UNLIMITED as f64, size_prec),
                2,
            );
        }
        Some(book)
    }
}

/// Fill model with two tiers: first 10 contracts at best price, remainder one tick worse.
#[derive(Debug)]
pub struct TwoTierFillModel {
    state: ProbabilisticFillState,
}

impl TwoTierFillModel {
    /// Creates a new [`TwoTierFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for TwoTierFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for TwoTierFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for TwoTierFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(10.0, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(10.0, size_prec),
            2,
        );
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            3,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            4,
        );
        Some(book)
    }
}

/// Fill model with three tiers: 50 at best, 30 at +1 tick, 20 at +2 ticks.
#[derive(Debug)]
pub struct ThreeTierFillModel {
    state: ProbabilisticFillState,
}

impl ThreeTierFillModel {
    /// Creates a new [`ThreeTierFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for ThreeTierFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for ThreeTierFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for ThreeTierFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let two_ticks = tick + tick;
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(50.0, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(50.0, size_prec),
            2,
        );
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - tick,
            Quantity::new(30.0, size_prec),
            3,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + tick,
            Quantity::new(30.0, size_prec),
            4,
        );
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - two_ticks,
            Quantity::new(20.0, size_prec),
            5,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + two_ticks,
            Quantity::new(20.0, size_prec),
            6,
        );
        Some(book)
    }
}

/// Fill model that simulates partial fills: max 5 contracts at best, unlimited one tick worse.
#[derive(Debug)]
pub struct LimitOrderPartialFillModel {
    state: ProbabilisticFillState,
}

impl LimitOrderPartialFillModel {
    /// Creates a new [`LimitOrderPartialFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for LimitOrderPartialFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for LimitOrderPartialFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for LimitOrderPartialFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(5.0, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(5.0, size_prec),
            2,
        );
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            3,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            4,
        );
        Some(book)
    }
}

/// Fill model that applies different execution based on order size.
/// Small orders (<=10) get 50 contracts at best. Large orders get 10 at best, remainder at +1 tick.
#[derive(Debug)]
pub struct SizeAwareFillModel {
    state: ProbabilisticFillState,
}

impl SizeAwareFillModel {
    /// Creates a new [`SizeAwareFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
        })
    }
}

impl Clone for SizeAwareFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Default for SizeAwareFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for SizeAwareFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        let threshold = Quantity::new(10.0, size_prec);
        if order.quantity() <= threshold {
            // Small orders: good liquidity at best
            add_order(
                &mut book,
                OrderSide::Buy,
                best_bid,
                Quantity::new(50.0, size_prec),
                1,
            );
            add_order(
                &mut book,
                OrderSide::Sell,
                best_ask,
                Quantity::new(50.0, size_prec),
                2,
            );
        } else {
            // Large orders: price impact
            let remaining = order.quantity() - threshold;
            add_order(&mut book, OrderSide::Buy, best_bid, threshold, 1);
            add_order(&mut book, OrderSide::Sell, best_ask, threshold, 2);
            add_order(&mut book, OrderSide::Buy, best_bid - tick, remaining, 3);
            add_order(&mut book, OrderSide::Sell, best_ask + tick, remaining, 4);
        }
        Some(book)
    }
}

/// Fill model that reduces available liquidity by a factor to simulate market competition.
#[derive(Debug)]
pub struct CompetitionAwareFillModel {
    state: ProbabilisticFillState,
    liquidity_factor: f64,
}

impl CompetitionAwareFillModel {
    /// Creates a new [`CompetitionAwareFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
        liquidity_factor: f64,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
            liquidity_factor,
        })
    }
}

impl Clone for CompetitionAwareFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            liquidity_factor: self.liquidity_factor,
        }
    }
}

impl Default for CompetitionAwareFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None, 0.3).unwrap()
    }
}

impl FillModel for CompetitionAwareFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        let typical_volume = 1000.0;

        // Minimum 1 to avoid zero-size orders
        let available_bid = (typical_volume * self.liquidity_factor).max(1.0);
        let available_ask = (typical_volume * self.liquidity_factor).max(1.0);

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(available_bid, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(available_ask, size_prec),
            2,
        );
        Some(book)
    }
}

/// Fill model that adjusts liquidity based on recent trading volume.
/// Uses 25% of recent volume at best price, unlimited one tick worse.
#[derive(Debug)]
pub struct VolumeSensitiveFillModel {
    state: ProbabilisticFillState,
    recent_volume: f64,
}

impl VolumeSensitiveFillModel {
    /// Creates a new [`VolumeSensitiveFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
            recent_volume: 1000.0,
        })
    }

    pub fn set_recent_volume(&mut self, volume: f64) {
        self.recent_volume = volume;
    }
}

impl Clone for VolumeSensitiveFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            recent_volume: self.recent_volume,
        }
    }
}

impl Default for VolumeSensitiveFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for VolumeSensitiveFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());

        // Minimum 1 to avoid zero-size orders
        let available_volume = (self.recent_volume * 0.25).max(1.0);

        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid,
            Quantity::new(available_volume, size_prec),
            1,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask,
            Quantity::new(available_volume, size_prec),
            2,
        );
        add_order(
            &mut book,
            OrderSide::Buy,
            best_bid - tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            3,
        );
        add_order(
            &mut book,
            OrderSide::Sell,
            best_ask + tick,
            Quantity::new(UNLIMITED as f64, size_prec),
            4,
        );
        Some(book)
    }
}

/// Fill model that simulates varying conditions based on market hours.
/// During low liquidity: wider spreads (one tick worse). Normal hours: standard liquidity.
#[derive(Debug)]
pub struct MarketHoursFillModel {
    state: ProbabilisticFillState,
    is_low_liquidity: bool,
}

impl MarketHoursFillModel {
    /// Creates a new [`MarketHoursFillModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if probability parameters are not in range [0, 1].
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            state: ProbabilisticFillState::new(prob_fill_on_limit, prob_slippage, random_seed)?,
            is_low_liquidity: false,
        })
    }

    pub fn set_low_liquidity_period(&mut self, is_low_liquidity: bool) {
        self.is_low_liquidity = is_low_liquidity;
    }

    pub fn is_low_liquidity_period(&self) -> bool {
        self.is_low_liquidity
    }
}

impl Clone for MarketHoursFillModel {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            is_low_liquidity: self.is_low_liquidity,
        }
    }
}

impl Default for MarketHoursFillModel {
    fn default() -> Self {
        Self::new(1.0, 0.0, None).unwrap()
    }
}

impl FillModel for MarketHoursFillModel {
    fn is_limit_filled(&mut self) -> bool {
        self.state.is_limit_filled()
    }

    fn is_slipped(&mut self) -> bool {
        self.state.is_slipped()
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        _order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        let tick = instrument.price_increment();
        let size_prec = instrument.size_precision();
        let mut book = build_l2_book(instrument.id());
        let normal_volume = 500.0;

        if self.is_low_liquidity {
            add_order(
                &mut book,
                OrderSide::Buy,
                best_bid - tick,
                Quantity::new(normal_volume, size_prec),
                1,
            );
            add_order(
                &mut book,
                OrderSide::Sell,
                best_ask + tick,
                Quantity::new(normal_volume, size_prec),
                2,
            );
        } else {
            add_order(
                &mut book,
                OrderSide::Buy,
                best_bid,
                Quantity::new(normal_volume, size_prec),
                1,
            );
            add_order(
                &mut book,
                OrderSide::Sell,
                best_ask,
                Quantity::new(normal_volume, size_prec),
                2,
            );
        }
        Some(book)
    }
}

#[derive(Clone, Debug)]
pub enum FillModelAny {
    Default(DefaultFillModel),
    BestPrice(BestPriceFillModel),
    OneTickSlippage(OneTickSlippageFillModel),
    Probabilistic(ProbabilisticFillModel),
    TwoTier(TwoTierFillModel),
    ThreeTier(ThreeTierFillModel),
    LimitOrderPartialFill(LimitOrderPartialFillModel),
    SizeAware(SizeAwareFillModel),
    CompetitionAware(CompetitionAwareFillModel),
    VolumeSensitive(VolumeSensitiveFillModel),
    MarketHours(MarketHoursFillModel),
}

impl FillModel for FillModelAny {
    fn is_limit_filled(&mut self) -> bool {
        match self {
            Self::Default(m) => m.is_limit_filled(),
            Self::BestPrice(m) => m.is_limit_filled(),
            Self::OneTickSlippage(m) => m.is_limit_filled(),
            Self::Probabilistic(m) => m.is_limit_filled(),
            Self::TwoTier(m) => m.is_limit_filled(),
            Self::ThreeTier(m) => m.is_limit_filled(),
            Self::LimitOrderPartialFill(m) => m.is_limit_filled(),
            Self::SizeAware(m) => m.is_limit_filled(),
            Self::CompetitionAware(m) => m.is_limit_filled(),
            Self::VolumeSensitive(m) => m.is_limit_filled(),
            Self::MarketHours(m) => m.is_limit_filled(),
        }
    }

    fn fill_limit_inside_spread(&self) -> bool {
        match self {
            Self::Default(m) => m.fill_limit_inside_spread(),
            Self::BestPrice(m) => m.fill_limit_inside_spread(),
            Self::OneTickSlippage(m) => m.fill_limit_inside_spread(),
            Self::Probabilistic(m) => m.fill_limit_inside_spread(),
            Self::TwoTier(m) => m.fill_limit_inside_spread(),
            Self::ThreeTier(m) => m.fill_limit_inside_spread(),
            Self::LimitOrderPartialFill(m) => m.fill_limit_inside_spread(),
            Self::SizeAware(m) => m.fill_limit_inside_spread(),
            Self::CompetitionAware(m) => m.fill_limit_inside_spread(),
            Self::VolumeSensitive(m) => m.fill_limit_inside_spread(),
            Self::MarketHours(m) => m.fill_limit_inside_spread(),
        }
    }

    fn is_slipped(&mut self) -> bool {
        match self {
            Self::Default(m) => m.is_slipped(),
            Self::BestPrice(m) => m.is_slipped(),
            Self::OneTickSlippage(m) => m.is_slipped(),
            Self::Probabilistic(m) => m.is_slipped(),
            Self::TwoTier(m) => m.is_slipped(),
            Self::ThreeTier(m) => m.is_slipped(),
            Self::LimitOrderPartialFill(m) => m.is_slipped(),
            Self::SizeAware(m) => m.is_slipped(),
            Self::CompetitionAware(m) => m.is_slipped(),
            Self::VolumeSensitive(m) => m.is_slipped(),
            Self::MarketHours(m) => m.is_slipped(),
        }
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        best_bid: Price,
        best_ask: Price,
    ) -> Option<OrderBook> {
        match self {
            Self::Default(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::BestPrice(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::OneTickSlippage(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::Probabilistic(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::TwoTier(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::ThreeTier(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::LimitOrderPartialFill(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::SizeAware(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::CompetitionAware(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::VolumeSensitive(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
            Self::MarketHours(m) => {
                m.get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask)
            }
        }
    }
}

impl Default for FillModelAny {
    fn default() -> Self {
        Self::Default(DefaultFillModel::default())
    }
}

impl Display for FillModelAny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default(m) => write!(f, "{m}"),
            Self::BestPrice(_) => write!(f, "BestPriceFillModel"),
            Self::OneTickSlippage(_) => write!(f, "OneTickSlippageFillModel"),
            Self::Probabilistic(_) => write!(f, "ProbabilisticFillModel"),
            Self::TwoTier(_) => write!(f, "TwoTierFillModel"),
            Self::ThreeTier(_) => write!(f, "ThreeTierFillModel"),
            Self::LimitOrderPartialFill(_) => write!(f, "LimitOrderPartialFillModel"),
            Self::SizeAware(_) => write!(f, "SizeAwareFillModel"),
            Self::CompetitionAware(_) => write!(f, "CompetitionAwareFillModel"),
            Self::VolumeSensitive(_) => write!(f, "VolumeSensitiveFillModel"),
            Self::MarketHours(_) => write!(f, "MarketHoursFillModel"),
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderType, instruments::stubs::audusd_sim, orders::builder::OrderTestBuilder,
    };
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn fill_model() -> DefaultFillModel {
        let seed = 42;
        DefaultFillModel::new(0.5, 0.1, Some(seed)).unwrap()
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid f64 for 'prob_fill_on_limit' not in range [0, 1], was 1.1"
    )]
    fn test_fill_model_param_prob_fill_on_limit_error() {
        let _ = DefaultFillModel::new(1.1, 0.1, None).unwrap();
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid f64 for 'prob_slippage' not in range [0, 1], was 1.1"
    )]
    fn test_fill_model_param_prob_slippage_error() {
        let _ = DefaultFillModel::new(0.5, 1.1, None).unwrap();
    }

    #[rstest]
    fn test_fill_model_is_limit_filled(mut fill_model: DefaultFillModel) {
        // Fixed seed makes this deterministic
        let result = fill_model.is_limit_filled();
        assert!(!result);
    }

    #[rstest]
    fn test_fill_model_is_slipped(mut fill_model: DefaultFillModel) {
        // Fixed seed makes this deterministic
        let result = fill_model.is_slipped();
        assert!(!result);
    }

    #[rstest]
    fn test_default_fill_model_returns_none() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let mut model = DefaultFillModel::default();
        let result = model.get_orderbook_for_fill_simulation(
            &instrument,
            &order,
            Price::from("0.80000"),
            Price::from("0.80010"),
        );
        assert!(result.is_none());
    }

    #[rstest]
    fn test_best_price_fill_model_returns_book() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let mut model = BestPriceFillModel::default();
        let result = model.get_orderbook_for_fill_simulation(
            &instrument,
            &order,
            Price::from("0.80000"),
            Price::from("0.80010"),
        );
        assert!(result.is_some());
        let book = result.unwrap();
        assert_eq!(book.best_bid_price().unwrap(), Price::from("0.80000"));
        assert_eq!(book.best_ask_price().unwrap(), Price::from("0.80010"));
    }

    #[rstest]
    fn test_one_tick_slippage_fill_model() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let tick = instrument.price_increment();
        let best_bid = Price::from("0.80000");
        let best_ask = Price::from("0.80010");

        let mut model = OneTickSlippageFillModel::default();
        let result =
            model.get_orderbook_for_fill_simulation(&instrument, &order, best_bid, best_ask);
        assert!(result.is_some());
        let book = result.unwrap();

        assert_eq!(book.best_bid_price().unwrap(), best_bid - tick);
        assert_eq!(book.best_ask_price().unwrap(), best_ask + tick);
    }

    #[rstest]
    fn test_fill_model_any_dispatch() {
        let model = FillModelAny::default();
        assert!(matches!(model, FillModelAny::Default(_)));
    }

    #[rstest]
    fn test_fill_model_any_is_limit_filled() {
        let mut model = FillModelAny::Default(DefaultFillModel::new(0.5, 0.1, Some(42)).unwrap());
        let result = model.is_limit_filled();
        assert!(!result);
    }

    #[rstest]
    fn test_default_fill_model_fill_limit_inside_spread_is_false() {
        let model = DefaultFillModel::default();
        assert!(!model.fill_limit_inside_spread());
    }

    #[rstest]
    fn test_best_price_fill_model_fill_limit_inside_spread_is_true() {
        let model = BestPriceFillModel::default();
        assert!(model.fill_limit_inside_spread());
    }

    #[rstest]
    fn test_one_tick_slippage_fill_model_fill_limit_inside_spread_is_false() {
        let model = OneTickSlippageFillModel::default();
        assert!(!model.fill_limit_inside_spread());
    }

    #[rstest]
    fn test_fill_model_any_fill_limit_inside_spread_dispatch() {
        let default = FillModelAny::Default(DefaultFillModel::default());
        assert!(!default.fill_limit_inside_spread());

        let best_price = FillModelAny::BestPrice(BestPriceFillModel::default());
        assert!(best_price.fill_limit_inside_spread());

        let one_tick = FillModelAny::OneTickSlippage(OneTickSlippageFillModel::default());
        assert!(!one_tick.fill_limit_inside_spread());
    }
}
