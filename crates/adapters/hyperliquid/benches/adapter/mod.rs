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

// Keep the production crate-root paths so restricted visibility resolves inside benchmarks
#[allow(dead_code, unused_imports)]
#[path = "../../src/account.rs"]
pub mod account;

#[allow(dead_code, unused_imports)]
#[path = "../../src/common/mod.rs"]
pub mod common;

#[allow(dead_code, unused_imports)]
#[path = "../../src/config.rs"]
pub mod config;

#[allow(dead_code, unused_imports)]
#[path = "../../src/data.rs"]
pub mod data;

#[allow(dead_code, unused_imports)]
#[path = "../../src/data_types.rs"]
pub mod data_types;

#[allow(dead_code, unused_imports)]
#[path = "../../src/execution.rs"]
pub mod execution;

#[allow(dead_code, unused_imports)]
#[path = "../../src/factories.rs"]
pub mod factories;

#[allow(dead_code, unused_imports)]
#[path = "../../src/http/mod.rs"]
pub mod http;

#[allow(dead_code, unused_imports)]
#[path = "../../src/outcome_settlement.rs"]
pub mod outcome_settlement;

#[allow(dead_code, unused_imports)]
#[path = "../../src/signing/mod.rs"]
pub mod signing;

#[allow(dead_code, unused_imports)]
#[path = "../../src/websocket/mod.rs"]
pub mod websocket;

#[allow(dead_code, unused_imports)]
#[path = "../../src/book/mod.rs"]
mod book;

#[cfg(feature = "python")]
#[allow(dead_code, unused_imports)]
#[path = "../../src/python/mod.rs"]
pub mod python;

pub use crate::{
    config::{HyperliquidDataClientConfig, HyperliquidExecutionClientConfig},
    data::HyperliquidDataClient,
    execution::HyperliquidExecutionClient,
    factories::{HyperliquidDataClientFactory, HyperliquidExecutionClientFactory},
    http::client::HyperliquidHttpClient,
    websocket::client::HyperliquidWebSocketClient,
};
