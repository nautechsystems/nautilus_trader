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

//! Native policy and diagnostics for exhausted submission recovery.

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId};
use serde::{Deserialize, Serialize};

/// Policy when recovery queries cannot establish a submission's venue outcome.
///
/// This configuration is reserved for submission recovery. The runtime currently
/// uses local resolution for both variants; retention is not implemented yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.live",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.live")
)]
pub enum SubmissionRecoveryPolicy {
    /// Selects local resolution using the existing timeout and missing-order policies.
    #[default]
    ResolveLocally,
    /// Selects retention without further automatic per-order queries (not implemented yet).
    RetainUnresolved,
}

/// Recovery path which exhausted its query budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionRecoverySource {
    /// The scheduled inflight checker exhausted its retry limit.
    Inflight,
    /// Full-history reconciliation and its targeted query found no definitive outcome.
    MissingOrder,
}

/// A submission's recovery budget expired without establishing its venue outcome.
///
/// This diagnostic is not an order event and does not change order status.
/// It defines the payload only; the runtime does not emit it yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmissionRecoveryExhausted {
    /// Trader which submitted the order.
    pub trader_id: TraderId,
    /// Execution client responsible for the order, when known.
    pub client_id: Option<ClientId>,
    /// Strategy which submitted the order.
    pub strategy_id: StrategyId,
    /// Instrument traded by the order.
    pub instrument_id: InstrumentId,
    /// Original client order identity.
    pub client_order_id: ClientOrderId,
    /// Recovery path which exhausted its budget.
    pub source: SubmissionRecoverySource,
    /// Native retry counter at exhaustion, not a count of wire requests.
    pub retry_count: u32,
    /// UNIX timestamp in nanoseconds when recovery was exhausted.
    pub ts_event: UnixNanos,
}
