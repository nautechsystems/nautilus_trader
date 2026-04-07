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

//! Complement arbitrage strategy for binary option markets.
//!
//! Binary options have a mathematical complement constraint: Yes + No = 1.0 at
//! resolution. When market inefficiencies cause `yes_ask + no_ask < 1.0 - fees`,
//! buying both sides locks in risk-free profit. When `yes_bid + no_bid > 1.0 + fees`,
//! selling both does the same. This strategy continuously monitors all discovered
//! complement pairs and logs when profitable opportunities appear.

pub mod config;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::ComplementArbConfig;
pub use strategy::ComplementArb;
