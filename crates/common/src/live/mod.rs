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

//! Live (async/tokio) components for real-time trading.
//!
//! This module contains components that require the tokio async runtime and are
//! used for live trading scenarios. These are gated behind the `live` feature flag.

pub mod clock;
pub mod listener;
pub mod runner;
pub mod runtime;
pub mod timer;

pub use clock::{LiveClock, TimeEventStream};
pub use listener::MessageBusListener;
pub use runner::{
    get_data_event_sender, get_exec_event_sender, set_data_event_sender, set_exec_event_sender,
};
pub use runtime::{get_runtime, shutdown_runtime};
pub use timer::LiveTimer;
