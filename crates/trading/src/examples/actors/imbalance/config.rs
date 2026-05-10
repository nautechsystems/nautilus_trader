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

//! Configuration for the book imbalance actor.

use nautilus_model::identifiers::{ActorId, InstrumentId};

/// Configuration for the order book imbalance actor.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
pub struct BookImbalanceActorConfig {
    /// Instruments to subscribe to.
    pub instrument_ids: Vec<InstrumentId>,
    /// How often (in update count) to log a progress line. Set to 0 to disable.
    pub log_interval: u64,
    /// Actor identifier. Defaults to `BOOK_IMBALANCE-001`.
    pub actor_id: Option<ActorId>,
}

impl BookImbalanceActorConfig {
    /// Creates a new [`BookImbalanceActorConfig`].
    #[must_use]
    pub fn new(instrument_ids: Vec<InstrumentId>) -> Self {
        Self {
            instrument_ids,
            log_interval: 100,
            actor_id: None,
        }
    }

    #[must_use]
    pub fn with_log_interval(mut self, interval: u64) -> Self {
        self.log_interval = interval;
        self
    }

    #[must_use]
    pub fn with_actor_id(mut self, actor_id: ActorId) -> Self {
        self.actor_id = Some(actor_id);
        self
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BookImbalanceActorConfig {
    #[new]
    #[pyo3(signature = (instrument_ids, log_interval=100, actor_id=None))]
    fn py_new(
        instrument_ids: Vec<InstrumentId>,
        log_interval: u64,
        actor_id: Option<ActorId>,
    ) -> Self {
        let mut config = Self::new(instrument_ids).with_log_interval(log_interval);

        if let Some(id) = actor_id {
            config.actor_id = Some(id);
        }

        config
    }

    #[getter]
    fn instrument_ids(&self) -> Vec<InstrumentId> {
        self.instrument_ids.clone()
    }

    #[getter]
    fn log_interval(&self) -> u64 {
        self.log_interval
    }

    #[getter]
    fn actor_id(&self) -> Option<ActorId> {
        self.actor_id
    }
}
