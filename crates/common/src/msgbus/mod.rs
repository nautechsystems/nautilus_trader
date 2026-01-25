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

//! In-memory message bus for intra-process communication.
//!
//! # Messaging patterns
//!
//! - **Point-to-point**: Send messages to named endpoints via `send_*` functions.
//! - **Pub/sub**: Publish messages to topics via `publish_*`, subscribers receive
//!   all messages matching their pattern.
//! - **Request/response**: Register correlation IDs for response sequence tracking.
//!
//! # Architecture
//!
//! The bus uses thread-local storage for single-threaded async runtimes. Each
//! thread gets its own `MessageBus` instance, avoiding synchronization overhead.
//!
//! Two routing mechanisms serve different needs:
//!
//! - **Typed routing** (`publish_quote`, `subscribe_quotes`): Zero-cost dispatch
//!   for known types. Handlers receive `&T` directly with no runtime type checking.
//! - **Any-based routing** (`publish_any`, `subscribe_any`): Flexible dispatch for
//!   custom types and Python interop. Handlers receive `&dyn Any`.
//!
//! See [`core`] module documentation for design decisions and performance details.

mod api;
pub mod core;
pub mod database;
pub mod matching;
pub mod message;
pub mod mstr;
pub mod stubs;
pub mod switchboard;
pub mod typed_endpoints;
pub mod typed_handler;
pub mod typed_router;

use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

#[cfg(feature = "defi")]
use nautilus_model::defi::{Block, Pool, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolSwap};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, GreeksData, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    events::{AccountState, OrderEventAny, PositionEvent},
    orderbook::OrderBook,
};
use smallvec::SmallVec;

pub use self::{
    api::*,
    core::{MessageBus, Subscription},
    message::BusMessage,
    mstr::{Endpoint, MStr, Pattern, Topic},
    switchboard::MessagingSwitchboard,
    typed_endpoints::{EndpointMap, IntoEndpointMap},
    typed_handler::{
        CallbackHandler, Handler, IntoHandler, ShareableMessageHandler, TypedHandler,
        TypedIntoHandler,
    },
    typed_router::{TopicRouter, TypedSubscription},
};

/// Inline capacity for handler buffers before heap allocation.
pub(super) const HANDLER_BUFFER_CAP: usize = 64;

// MessageBus is designed for single-threaded use within each async runtime.
// Thread-local storage ensures each thread gets its own instance, eliminating
// the need for unsafe Send/Sync implementations.
//
// Handler buffers provide zero-allocation publish on hot paths.
// Each buffer stores up to 64 handlers inline before spilling to heap.
// Publish functions use move-out/move-back to avoid holding RefCell borrows
// during handler calls (enabling re-entrant publishes).
thread_local! {
    pub(super) static MESSAGE_BUS: OnceCell<Rc<RefCell<MessageBus>>> = const { OnceCell::new() };

    pub(super) static ANY_HANDLERS: RefCell<SmallVec<[ShareableMessageHandler; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());

    pub(super) static DELTAS_HANDLERS: RefCell<SmallVec<[TypedHandler<OrderBookDeltas>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static DEPTH10_HANDLERS: RefCell<SmallVec<[TypedHandler<OrderBookDepth10>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static BOOK_HANDLERS: RefCell<SmallVec<[TypedHandler<OrderBook>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static QUOTE_HANDLERS: RefCell<SmallVec<[TypedHandler<QuoteTick>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static TRADE_HANDLERS: RefCell<SmallVec<[TypedHandler<TradeTick>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static BAR_HANDLERS: RefCell<SmallVec<[TypedHandler<Bar>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static MARK_PRICE_HANDLERS: RefCell<SmallVec<[TypedHandler<MarkPriceUpdate>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static INDEX_PRICE_HANDLERS: RefCell<SmallVec<[TypedHandler<IndexPriceUpdate>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static FUNDING_RATE_HANDLERS: RefCell<SmallVec<[TypedHandler<FundingRateUpdate>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static GREEKS_HANDLERS: RefCell<SmallVec<[TypedHandler<GreeksData>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static ACCOUNT_STATE_HANDLERS: RefCell<SmallVec<[TypedHandler<AccountState>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static ORDER_EVENT_HANDLERS: RefCell<SmallVec<[TypedHandler<OrderEventAny>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    pub(super) static POSITION_EVENT_HANDLERS: RefCell<SmallVec<[TypedHandler<PositionEvent>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());

    #[cfg(feature = "defi")]
    pub(super) static DEFI_BLOCK_HANDLERS: RefCell<SmallVec<[TypedHandler<Block>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    #[cfg(feature = "defi")]
    pub(super) static DEFI_POOL_HANDLERS: RefCell<SmallVec<[TypedHandler<Pool>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    #[cfg(feature = "defi")]
    pub(super) static DEFI_SWAP_HANDLERS: RefCell<SmallVec<[TypedHandler<PoolSwap>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    #[cfg(feature = "defi")]
    pub(super) static DEFI_LIQUIDITY_HANDLERS: RefCell<SmallVec<[TypedHandler<PoolLiquidityUpdate>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    #[cfg(feature = "defi")]
    pub(super) static DEFI_COLLECT_HANDLERS: RefCell<SmallVec<[TypedHandler<PoolFeeCollect>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
    #[cfg(feature = "defi")]
    pub(super) static DEFI_FLASH_HANDLERS: RefCell<SmallVec<[TypedHandler<PoolFlash>; HANDLER_BUFFER_CAP]>> =
        RefCell::new(SmallVec::new());
}

/// Sets the thread-local message bus.
///
/// # Panics
///
/// Panics if a message bus has already been set for this thread.
pub fn set_message_bus(msgbus: Rc<RefCell<MessageBus>>) {
    MESSAGE_BUS.with(|bus| {
        assert!(
            bus.set(msgbus).is_ok(),
            "Failed to set MessageBus: already initialized for this thread"
        );
    });
}

/// Gets the thread-local message bus.
///
/// If no message bus has been set for this thread, a default one is created and initialized.
pub fn get_message_bus() -> Rc<RefCell<MessageBus>> {
    MESSAGE_BUS.with(|bus| {
        bus.get_or_init(|| {
            let msgbus = MessageBus::default();
            Rc::new(RefCell::new(msgbus))
        })
        .clone()
    })
}
