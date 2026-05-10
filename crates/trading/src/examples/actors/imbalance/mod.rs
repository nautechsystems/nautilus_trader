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

//! Order book imbalance actor.
//!
//! Subscribes to order book deltas for a set of instruments and tracks the
//! cumulative bid/ask quoted volume. For each batch of deltas, sums the
//! resting size at each updated price level per side and computes:
//!
//! `imbalance = (bid_volume - ask_volume) / (bid_volume + ask_volume)`
//!
//! The result ranges from -1.0 (all volume on the ask side) to +1.0 (all
//! volume on the bid side). This measures which side of the book receives
//! more quoted volume across updates, not the incremental change in volume.
//! In a betting exchange context, bids correspond to "back" orders and asks
//! to "lay" orders.

pub mod actor;
pub mod config;

#[cfg(test)]
mod tests;

pub use actor::{BookImbalanceActor, ImbalanceState};
pub use config::BookImbalanceActorConfig;
