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

//! Event-driven option chain aggregation and snapshot publishing.

pub mod aggregator;
pub mod atm_tracker;
pub mod constants;
pub mod handlers;
pub mod manager;

pub use aggregator::{OptionChainAggregator, RebalanceAction};
pub use atm_tracker::AtmTracker;
pub use handlers::{OptionChainGreeksHandler, OptionChainQuoteHandler, OptionChainSlicePublisher};
pub use manager::OptionChainManager;
