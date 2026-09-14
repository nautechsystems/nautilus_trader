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

//! Execution state, reconciliation decisions, and adapter-facing dispatch support.
//!
//! [`manager`] owns cached execution reconciliation and its activity and retry state.
//! Internal reconciliation operations build and validate reports and collect targeted reports.
//! [`context`], [`failure`], [`reports`], and [`emitter`] provide the shared adapter interfaces.
//! The manager coordinates individual reconciliation operations; the live node schedules them.

pub mod config;
pub mod context;
pub mod emitter;
pub mod failure;
pub mod manager;
pub mod reports;

#[cfg(feature = "node")]
pub(crate) mod client;

mod recency;
mod reconciliation;
