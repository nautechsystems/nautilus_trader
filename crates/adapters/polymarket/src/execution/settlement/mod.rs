// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Adapter-owned settlement evidence registry per RFC #4876.
//!
//! Polymarket reports trades before on-chain settlement is final, so a valid fill can later
//! require correction. This module separates venue settlement from per-leg core application:
//! every trade is one record with a settlement disposition and per-leg application states, all
//! evidence is admitted through one shared boundary, a fresh targeted terminal REST result is
//! the settlement authority with first-result-wins, and reconciliation defers to the registry
//! while evidence is unresolved.

pub(crate) mod admission;
pub(crate) mod registry;
pub(crate) mod state;

pub(crate) use admission::{
    AdmissionContext, AdmissionError, AdmittedLeg, AdmittedTrade, TradeEvidence,
    admit_trade_evidence, admit_trade_legs,
};
pub(crate) use registry::SettlementRegistry;
pub(crate) use state::{SettlementAction, UncertainOrder};
