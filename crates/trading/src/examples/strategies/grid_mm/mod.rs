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

//! Grid market making strategy with inventory-based skewing.
//!
//! Subscribes to quotes for a single instrument and maintains a symmetric grid
//! of limit orders around the mid-price. Orders are only replaced when the
//! mid-price moves beyond a configurable threshold, allowing resting orders to
//! persist across ticks. The grid is shifted by a skew proportional to the
//! current net position to discourage inventory buildup (Avellaneda-Stoikov
//! inspired).
//!
//! Max-position enforcement uses worst-case same-side exposure: `net_position`
//! drives the skew offset, while `worst_long`/`worst_short` include both open
//! positions and all pending buy/sell orders to account for async cancel latency.

pub mod config;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::GridMarketMakerConfig;
pub use strategy::GridMarketMaker;
