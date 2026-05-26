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

//! Order command envelopes and their boundary-owned handle wrappers.
//!
//! Plug-in strategies invoke host-side execution slots (`submit_order`,
//! `cancel_order`, `modify_order`, etc.) by constructing a command struct
//! in this module, wrapping it in the matching `*Handle`, and handing the
//! host a `*const XHandle`. The host derefs the handle once and routes the
//! borrowed command into the execution engine. The plug-in owns the box
//! and frees it when the call returns.
//!
//! Mirrors the ownership contract that
//! [`OrderBookDeltasHandle`](crate::surfaces::book::OrderBookDeltasHandle)
//! and
//! [`InstrumentAnyHandle`](crate::surfaces::instrument::InstrumentAnyHandle)
//! use for incoming events, but in the opposite direction: the plug-in
//! constructs and owns the box, the host borrows for the call.

pub mod cancel;
pub mod close;
pub mod modify;
pub mod query;
pub mod submit;

pub use cancel::{
    CancelAllOrdersCommand, CancelAllOrdersHandle, CancelOrderCommand, CancelOrderHandle,
    CancelOrdersCommand, CancelOrdersHandle,
};
pub use close::{
    CloseAllPositionsCommand, CloseAllPositionsHandle, ClosePositionCommand, ClosePositionHandle,
};
pub use modify::{ModifyOrderCommand, ModifyOrderHandle};
pub use query::{QueryAccountCommand, QueryAccountHandle, QueryOrderCommand, QueryOrderHandle};
pub use submit::{
    SubmitOrderCommand, SubmitOrderHandle, SubmitOrderListCommand, SubmitOrderListHandle,
};
