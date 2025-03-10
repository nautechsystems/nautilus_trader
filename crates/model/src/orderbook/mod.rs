// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Order book components which can handle L1/L2/L3 data.

pub mod aggregation;
pub mod analysis;
pub mod book;
pub mod display;
pub mod error;
pub mod ladder;
pub mod level;
pub mod own;

#[cfg(test)]
mod tests;

// Re-exports
pub use crate::orderbook::{
    book::OrderBook,
    error::{BookIntegrityError, InvalidBookOperation},
    ladder::BookPrice,
    level::BookLevel,
    own::OwnBookOrder,
};
