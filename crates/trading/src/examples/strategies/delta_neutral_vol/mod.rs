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

//! Delta-neutral short volatility hedger.
//!
//! Tracks a short OTM call and put (strangle) on a configurable option
//! family and delta-hedges the net Greek exposure with the underlying
//! perpetual swap. Rehedges when portfolio delta exceeds a configurable
//! threshold or on a periodic timer.
//!
//! This strategy subscribes to venue-provided Greeks via
//! `subscribe_option_greeks` and uses them to track portfolio delta.
//! Strike selection uses a simple strike-percentile heuristic at startup.

pub mod config;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::DeltaNeutralVolConfig;
pub use strategy::DeltaNeutralVol;
