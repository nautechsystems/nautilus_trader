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

//! Order book imbalance actor implementation.

use std::{collections::BTreeMap, fmt::Debug};

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActor, DataActorConfig, DataActorCore},
    nautilus_actor,
};
use nautilus_model::{
    data::OrderBookDeltas,
    enums::{BookType, OrderSide},
    identifiers::{ActorId, InstrumentId},
};

/// Per-instrument imbalance tracking state.
#[derive(Debug)]
pub struct ImbalanceState {
    /// Total number of book delta batches processed.
    pub update_count: u64,
    /// Cumulative bid-side volume across all updates.
    pub bid_volume_total: f64,
    /// Cumulative ask-side volume across all updates.
    pub ask_volume_total: f64,
}

impl ImbalanceState {
    /// Creates a new [`ImbalanceState`] with zero counts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            update_count: 0,
            bid_volume_total: 0.0,
            ask_volume_total: 0.0,
        }
    }

    /// Returns the cumulative quoted volume imbalance, or 0.0 if no volume observed.
    #[must_use]
    pub fn imbalance(&self) -> f64 {
        let total = self.bid_volume_total + self.ask_volume_total;
        if total > 0.0 {
            (self.bid_volume_total - self.ask_volume_total) / total
        } else {
            0.0
        }
    }
}

impl Default for ImbalanceState {
    fn default() -> Self {
        Self::new()
    }
}

/// Actor that tracks bid/ask quoted volume imbalance from order book deltas.
///
/// On start, subscribes to [`OrderBookDeltas`] for each configured instrument.
/// On each update, sums the resting size at each updated level per side and
/// accumulates running totals. Logs the cumulative imbalance at a configurable
/// interval. On stop, prints a per-instrument summary.
pub struct BookImbalanceActor {
    core: DataActorCore,
    instrument_ids: Vec<InstrumentId>,
    log_interval: u64,
    states: AHashMap<InstrumentId, ImbalanceState>,
}

impl BookImbalanceActor {
    /// Creates a new [`BookImbalanceActor`] from config.
    #[must_use]
    pub fn from_config(config: super::config::BookImbalanceActorConfig) -> Self {
        Self::new(config.instrument_ids, config.log_interval, config.actor_id)
    }

    /// Creates a new [`BookImbalanceActor`].
    ///
    /// `actor_id` sets the actor identifier. Pass `None` for the default
    /// `"BOOK_IMBALANCE-001"`.
    ///
    /// `log_interval` controls how often (in update count) a progress line
    /// is printed. Set to 0 to disable periodic logging.
    #[must_use]
    pub fn new(
        instrument_ids: Vec<InstrumentId>,
        log_interval: u64,
        actor_id: Option<ActorId>,
    ) -> Self {
        let config = DataActorConfig {
            actor_id: Some(actor_id.unwrap_or(ActorId::from("BOOK_IMBALANCE-001"))),
            ..Default::default()
        };
        Self {
            core: DataActorCore::new(config),
            instrument_ids,
            log_interval,
            states: AHashMap::new(),
        }
    }

    /// Returns the per-instrument imbalance states.
    #[must_use]
    pub fn states(&self) -> &AHashMap<InstrumentId, ImbalanceState> {
        &self.states
    }

    /// Prints a summary of all tracked instruments to stdout.
    pub fn print_summary(&self) {
        println!("\n--- Book imbalance summary ---");
        let sorted: BTreeMap<String, &ImbalanceState> = self
            .states
            .iter()
            .map(|(id, state)| (id.to_string(), state))
            .collect();

        for (id, state) in &sorted {
            println!(
                "  {id}  updates: {}  bid_vol: {:.2}  ask_vol: {:.2}  imbalance: {:.4}",
                state.update_count,
                state.bid_volume_total,
                state.ask_volume_total,
                state.imbalance(),
            );
        }
    }
}

nautilus_actor!(BookImbalanceActor);

impl Debug for BookImbalanceActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BookImbalanceActor))
            .field("instrument_ids", &self.instrument_ids)
            .field("log_interval", &self.log_interval)
            .finish()
    }
}

impl DataActor for BookImbalanceActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let ids = self.instrument_ids.clone();
        for instrument_id in ids {
            self.subscribe_book_deltas(
                instrument_id,
                BookType::L2_MBP,
                None,  // depth
                None,  // client_id
                false, // managed
                None,  // params
            );
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.print_summary();
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        let mut bid_volume = 0.0;
        let mut ask_volume = 0.0;

        for delta in &deltas.deltas {
            let size = delta.order.size.as_f64();
            match delta.order.side {
                OrderSide::Buy => bid_volume += size,
                OrderSide::Sell => ask_volume += size,
                _ => {}
            }
        }

        let state = self.states.entry(deltas.instrument_id).or_default();

        state.update_count += 1;
        state.bid_volume_total += bid_volume;
        state.ask_volume_total += ask_volume;

        if self.log_interval > 0 && state.update_count.is_multiple_of(self.log_interval) {
            println!(
                "[{}] update #{}: batch bid={:.2} ask={:.2}  cumulative imbalance={:.4}",
                deltas.instrument_id,
                state.update_count,
                bid_volume,
                ask_volume,
                state.imbalance(),
            );
        }

        Ok(())
    }
}
