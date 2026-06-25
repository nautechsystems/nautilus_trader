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
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct BookImbalanceActorConfig {
    /// Instruments to subscribe to.
    pub instrument_ids: Vec<InstrumentId>,
    /// How often (in update count) to log a progress line. Set to 0 to disable.
    #[builder(default = 100)]
    pub log_interval: u64,
    /// Actor identifier. Defaults to `BOOK_IMBALANCE-001`.
    pub actor_id: Option<ActorId>,
}
