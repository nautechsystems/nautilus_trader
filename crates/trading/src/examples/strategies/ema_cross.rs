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

//! Dual-EMA crossover strategy.
//!
//! Subscribes to quotes for a single instrument, maintains fast and slow
//! exponential moving averages, and submits market orders when the fast
//! EMA crosses above (buy) or below (sell) the slow EMA.

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use nautilus_common::actor::{DataActor, DataActorCore};
use nautilus_indicators::{
    average::ema::ExponentialMovingAverage,
    indicator::{Indicator, MovingAverage},
};
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, PriceType},
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};

use crate::strategy::{Strategy, StrategyConfig, StrategyCore};

/// Dual-EMA crossover strategy.
///
/// Generates buy signals when the fast EMA crosses above the slow EMA,
/// and sell signals when the fast crosses below.
pub struct EmaCross {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    prev_fast_above: Option<bool>,
}

impl EmaCross {
    /// Creates a new [`EmaCross`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        fast_period: usize,
        slow_period: usize,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("EMA_CROSS-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            ema_fast: ExponentialMovingAverage::new(fast_period, Some(PriceType::Mid)),
            ema_slow: ExponentialMovingAverage::new(slow_period, Some(PriceType::Mid)),
            prev_fast_above: None,
        }
    }

    fn enter(&mut self, side: OrderSide) -> anyhow::Result<()> {
        let order = self.core.order_factory().market(
            self.instrument_id,
            side,
            self.trade_size,
            None, // time_in_force
            None, // reduce_only
            None, // quote_quantity
            None, // display_qty
            None, // expire_time
            None, // emulation_trigger
            None, // tags
        );
        self.submit_order(order, None, None)
    }
}

impl Deref for EmaCross {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for EmaCross {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Debug for EmaCross {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmaCross))
            .field("instrument_id", &self.instrument_id)
            .field("trade_size", &self.trade_size)
            .field("fast_period", &self.ema_fast.period)
            .field("slow_period", &self.ema_slow.period)
            .finish()
    }
}

impl DataActor for EmaCross {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.ema_fast.handle_quote(quote);
        self.ema_slow.handle_quote(quote);

        if !self.ema_fast.initialized() || !self.ema_slow.initialized() {
            return Ok(());
        }

        let fast = self.ema_fast.value();
        let slow = self.ema_slow.value();
        let fast_above = fast > slow;

        if let Some(prev) = self.prev_fast_above {
            if fast_above && !prev {
                self.enter(OrderSide::Buy)?;
            } else if !fast_above && prev {
                self.enter(OrderSide::Sell)?;
            }
        }

        self.prev_fast_above = Some(fast_above);
        Ok(())
    }
}

impl Strategy for EmaCross {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}
