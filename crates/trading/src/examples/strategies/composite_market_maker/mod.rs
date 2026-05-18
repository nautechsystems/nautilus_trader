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

//! Composite market making strategy with book-mid quoting and external-signal skew.
//!
//! Subscribes to quotes for two instruments: a target instrument (the market the
//! strategy quotes on) and a signal instrument (typically a `SyntheticInstrument`
//! published by the data engine, but any instrument with a quote stream works).
//! Quotes a single bid and a single ask around the target's book mid, with two
//! independent skew terms applied on top:
//!
//! - **Inventory skew** discourages position buildup. With `inventory_skew_factor`
//!   positive, both sides shift down by `factor * net_position`: the bid moves
//!   further from the market when long, the ask moves closer.
//! - **Signal skew** lifts both quotes when the signal trades above its baseline.
//!   The residual is `(signal_mid - baseline) / baseline`. With
//!   `signal_skew_factor` positive, both sides shift up by `factor * residual`,
//!   which captures expected drift inferred from the signal.
//!
//! Max-position enforcement uses worst-case same-side exposure: open positions
//! plus all pending buy/sell orders are projected forward so the cap holds even
//! while async cancels are in flight (same pattern as `GridMarketMaker`).

pub mod config;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::CompositeMarketMakerConfig;
pub use strategy::CompositeMarketMaker;
