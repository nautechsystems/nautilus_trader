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

//! Example showing how to use the `GreeksCalculator` with a `DataActor`.

use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use nautilus_common::{
    actor::{
        Actor,
        data_actor::{DataActor, DataActorConfig, DataActorCore},
    },
    cache::Cache,
    clock::LiveClock,
    greeks::GreeksCalculator,
};
use nautilus_model::{data::greeks::GreeksData, enums::PositionSide, identifiers::InstrumentId};

/// A custom actor that uses the `GreeksCalculator`.
struct GreeksActor {
    core: DataActorCore,
    greeks_calculator: GreeksCalculator,
}

impl GreeksActor {
    /// Creates a new [`GreeksActor`] instance.
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<LiveClock>>,
    ) -> Self {
        let core = DataActorCore::new(config, cache.clone(), clock.clone());

        // Create the GreeksCalculator with the same clock and cache
        let greeks_calculator = GreeksCalculator::new(cache, clock);

        Self {
            core,
            greeks_calculator,
        }
    }

    /// Calculates greeks for a specific instrument.
    pub fn calculate_instrument_greeks(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<GreeksData> {
        // Example parameters
        let flat_interest_rate = 0.0425;
        let flat_dividend_yield = None;
        let spot_shock = 0.0;
        let vol_shock = 0.0;
        let time_to_expiry_shock = 0.0;
        let use_cached_greeks = false;
        let cache_greeks = true;
        let publish_greeks = true;
        let ts_event = self.core.clock.borrow().timestamp_ns();
        let position = None;
        let percent_greeks = false;
        let index_instrument_id = None;
        let beta_weights = None;

        // Calculate greeks
        self.greeks_calculator.instrument_greeks(
            instrument_id,
            Some(flat_interest_rate),
            flat_dividend_yield,
            Some(spot_shock),
            Some(vol_shock),
            Some(time_to_expiry_shock),
            Some(use_cached_greeks),
            Some(cache_greeks),
            Some(publish_greeks),
            Some(ts_event),
            position,
            Some(percent_greeks),
            index_instrument_id,
            beta_weights,
        )
    }

    /// Calculates portfolio greeks.
    pub fn calculate_portfolio_greeks(
        &self,
    ) -> anyhow::Result<nautilus_model::data::greeks::PortfolioGreeks> {
        // Example parameters
        let underlyings = None;
        let venue = None;
        let instrument_id = None;
        let strategy_id = None;
        let side = Some(PositionSide::NoPositionSide);
        let flat_interest_rate = 0.0425;
        let flat_dividend_yield = None;
        let spot_shock = 0.0;
        let vol_shock = 0.0;
        let time_to_expiry_shock = 0.0;
        let use_cached_greeks = false;
        let cache_greeks = true;
        let publish_greeks = true;
        let percent_greeks = false;
        let index_instrument_id = None;
        let beta_weights = None;

        // Calculate portfolio greeks
        self.greeks_calculator.portfolio_greeks(
            underlyings,
            venue,
            instrument_id,
            strategy_id,
            side,
            Some(flat_interest_rate),
            flat_dividend_yield,
            Some(spot_shock),
            Some(vol_shock),
            Some(time_to_expiry_shock),
            Some(use_cached_greeks),
            Some(cache_greeks),
            Some(publish_greeks),
            Some(percent_greeks),
            index_instrument_id,
            beta_weights,
        )
    }

    /// Subscribes to greeks data for a specific underlying.
    pub fn subscribe_to_greeks(&self, underlying: &str) {
        // Subscribe to greeks data
        self.greeks_calculator
            .subscribe_greeks::<fn(GreeksData)>(underlying, None);
    }
}

impl Actor for GreeksActor {
    fn id(&self) -> ustr::Ustr {
        self.core.actor_id.inner()
    }

    fn handle(&mut self, msg: &dyn std::any::Any) {
        // Let the core handle message routing
        self.core.handle(msg);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// We need to explicitly implement `Deref` for actors to improve API ergonomics.
// In the future we can probably create a macro to do this.
impl Deref for GreeksActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

// We need to explicitly implement `DerefMut` for actors to improve API ergonomics.
// In the future we can probably create a macro to do this.
impl DerefMut for GreeksActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for GreeksActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        // Subscribe to greeks data for SPY
        self.subscribe_to_greeks("SPY");
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_data(&mut self, data: &dyn std::any::Any) -> anyhow::Result<()> {
        // Handle received data
        if let Some(greeks_data) = data.downcast_ref::<GreeksData>() {
            println!("Received greeks data: {greeks_data:?}");
        }

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    // Create components
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(LiveClock::default()));

    // Create actor config
    let config = DataActorConfig::default();

    // Create the GreeksActor
    let mut actor = GreeksActor::new(config, cache, clock);

    // Start the actor
    actor.start()?;

    // Example: Calculate greeks for an instrument
    let instrument_id = InstrumentId::from("SPY.AMEX");
    match actor.calculate_instrument_greeks(instrument_id) {
        Ok(greeks) => println!("Calculated greeks for {instrument_id}: {greeks:?}"),
        Err(e) => println!("Error calculating greeks: {e}"),
    }

    // Example: Calculate portfolio greeks
    match actor.calculate_portfolio_greeks() {
        Ok(portfolio_greeks) => println!("Portfolio greeks: {portfolio_greeks:?}"),
        Err(e) => println!("Error calculating portfolio greeks: {e}"),
    }

    // Stop the actor
    actor.stop()?;

    Ok(())
}
