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

//! Global runtime machinery and thread-local storage.
//!
//! This module provides global access to shared runtime resources including clocks,
//! message queues, and time event channels. It manages thread-local storage for
//! system-wide components that need to be accessible across threads.

use std::{
    cell::RefCell,
    fmt::Debug,
    num::NonZeroU64,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, ThreadId},
};

use ahash::AHashMap;

use crate::{
    messages::{data::DataCommand, execution::TradingCommand},
    msgbus::{self, MessagingSwitchboard},
    timer::{TimeEvent, TimeEventCallback, TimeEventHandler},
};

const CALLBACK_CLOSED: usize = 1 << (usize::BITS - 1);
const CALLBACK_LEASES: usize = CALLBACK_CLOSED - 1;
static NEXT_TIME_EVENT_CALLBACK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TimeEventCallbackId(NonZeroU64);

#[derive(Debug)]
struct TimeEventCallbackTokenInner {
    id: TimeEventCallbackId,
    owner: ThreadId,
    state: AtomicUsize,
}

struct TimeEventCallbackEntry {
    callback: TimeEventCallback,
    token: Weak<TimeEventCallbackTokenInner>,
}

/// A send-safe handle to a thread-local time event callback.
#[derive(Clone, Debug)]
pub(crate) struct TimeEventCallbackToken(Arc<TimeEventCallbackTokenInner>);

impl TimeEventCallbackToken {
    fn register(callback: TimeEventCallback) -> Self {
        debug_assert!(callback.is_local());
        purge_closed_time_event_callbacks();

        let raw_id = NEXT_TIME_EVENT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
        let id = TimeEventCallbackId(
            NonZeroU64::new(raw_id).expect("time event callback IDs exhausted"),
        );
        let token = Self(Arc::new(TimeEventCallbackTokenInner {
            id,
            owner: thread::current().id(),
            state: AtomicUsize::new(0),
        }));
        TIME_EVENT_CALLBACKS.with(|callbacks| {
            let previous = callbacks.borrow_mut().insert(
                id,
                TimeEventCallbackEntry {
                    callback,
                    token: Arc::downgrade(&token.0),
                },
            );
            debug_assert!(previous.is_none());
        });
        token
    }

    pub(crate) fn acquire(&self) -> Option<TimeEventCallbackLease> {
        let mut state = self.0.state.load(Ordering::Acquire);
        loop {
            if state & CALLBACK_CLOSED != 0 {
                return None;
            }
            let leases = state & CALLBACK_LEASES;
            assert!(
                leases < CALLBACK_LEASES,
                "time event callback lease count overflow"
            );

            match self.0.state.compare_exchange_weak(
                state,
                state + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(TimeEventCallbackLease(self.0.clone())),
                Err(actual) => state = actual,
            }
        }
    }

    #[cfg(any(feature = "live", test))]
    pub(crate) fn is_closed(&self) -> bool {
        self.0.state.load(Ordering::Acquire) & CALLBACK_CLOSED != 0
    }

    pub(crate) fn close(&self) {
        let previous = self.0.state.fetch_or(CALLBACK_CLOSED, Ordering::AcqRel);
        if previous & CALLBACK_LEASES == 0 {
            self.remove_on_owner_thread();
        }
    }

    fn remove_on_owner_thread(&self) {
        if self.0.owner == thread::current().id() {
            TIME_EVENT_CALLBACKS.with(|callbacks| {
                callbacks.borrow_mut().remove(&self.0.id);
            });
        }
    }

    #[cfg(test)]
    fn is_registered(&self) -> bool {
        TIME_EVENT_CALLBACKS.with(|callbacks| callbacks.borrow().contains_key(&self.0.id))
    }
}

/// A per-message hold on a registered callback entry.
///
/// The final lease of a closed token removes the TLS entry when it drops on
/// the owner thread. A final lease dropped on another thread (failed send,
/// receiver shutdown on a foreign thread) cannot touch the owner's TLS map;
/// the closed entry is then reclaimed lazily by the next owner-thread
/// registration or [`purge_closed_time_event_callbacks`] call (`LiveClock`
/// invokes the latter from `clear_expired_timers`). That is bounded
/// retention of the callback, never a leak across registrations and never
/// a cross-thread `Rc` access.
#[derive(Debug)]
pub(crate) struct TimeEventCallbackLease(Arc<TimeEventCallbackTokenInner>);

impl Drop for TimeEventCallbackLease {
    fn drop(&mut self) {
        let previous = self.0.state.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous & CALLBACK_LEASES > 0);
        if previous == CALLBACK_CLOSED | 1 && self.0.owner == thread::current().id() {
            TIME_EVENT_CALLBACKS.with(|callbacks| {
                callbacks.borrow_mut().remove(&self.0.id);
            });
        }
    }
}

#[derive(Clone)]
enum SendTimeEventCallback {
    #[cfg(feature = "python")]
    Python(Arc<crate::timer::PythonTimeEventCallback>),
    Rust(Arc<dyn Fn(TimeEvent) + Send + Sync>),
}

impl Debug for SendTimeEventCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "python")]
            Self::Python(_) => f.write_str("Python callback"),
            Self::Rust(_) => f.write_str("Rust callback (thread-safe)"),
        }
    }
}

impl SendTimeEventCallback {
    fn into_callback(self) -> TimeEventCallback {
        match self {
            #[cfg(feature = "python")]
            Self::Python(callback) => TimeEventCallback::Python(callback),
            Self::Rust(callback) => TimeEventCallback::Rust(callback),
        }
    }
}

#[derive(Clone, Debug)]
#[cfg(feature = "live")]
pub(crate) struct TimeEventMessageFactory(SendTimeEventCallback);

#[cfg(feature = "live")]
impl TimeEventMessageFactory {
    pub(crate) fn new(callback: &TimeEventCallback) -> Self {
        match callback {
            #[cfg(feature = "python")]
            TimeEventCallback::Python(callback) => {
                Self(SendTimeEventCallback::Python(callback.clone()))
            }
            TimeEventCallback::Rust(callback) => {
                Self(SendTimeEventCallback::Rust(callback.clone()))
            }
            TimeEventCallback::RustLocal(_) => {
                unreachable!("RustLocal callbacks require registered dispatch")
            }
        }
    }

    pub(crate) fn message(&self, event: TimeEvent) -> TimeEventMessage {
        TimeEventMessage {
            event,
            dispatch: TimeEventDispatch::Direct(self.0.clone()),
        }
    }
}

#[derive(Debug)]
enum TimeEventDispatch {
    Direct(SendTimeEventCallback),
    Registered(TimeEventCallbackLease),
    #[cfg(any(feature = "live", test))]
    Cleanup(TimeEventCallbackLease),
}

/// A send-safe live time event channel payload.
///
/// The dispatch representation is private so local callbacks can never be
/// embedded in a cross-thread message.
#[derive(Debug)]
pub struct TimeEventMessage {
    event: TimeEvent,
    dispatch: TimeEventDispatch,
}

impl TimeEventMessage {
    /// Creates a message from a time event and callback.
    ///
    /// # Panics
    ///
    /// Panics if the process-wide callback ID or lease count is exhausted.
    #[must_use]
    pub fn new(event: TimeEvent, callback: TimeEventCallback) -> Self {
        match callback {
            #[cfg(feature = "python")]
            TimeEventCallback::Python(callback) => Self {
                event,
                dispatch: TimeEventDispatch::Direct(SendTimeEventCallback::Python(callback)),
            },
            TimeEventCallback::Rust(callback) => Self {
                event,
                dispatch: TimeEventDispatch::Direct(SendTimeEventCallback::Rust(callback)),
            },
            callback @ TimeEventCallback::RustLocal(_) => {
                let token = TimeEventCallbackToken::register(callback);
                let lease = token
                    .acquire()
                    .expect("new time event callback token should be open");
                token.close();
                Self::registered(event, lease)
            }
        }
    }

    /// Returns the time event carried by this message.
    #[must_use]
    pub const fn event(&self) -> &TimeEvent {
        &self.event
    }

    pub(crate) const fn registered(event: TimeEvent, lease: TimeEventCallbackLease) -> Self {
        Self {
            event,
            dispatch: TimeEventDispatch::Registered(lease),
        }
    }

    #[cfg(any(feature = "live", test))]
    pub(crate) const fn cleanup(event: TimeEvent, lease: TimeEventCallbackLease) -> Self {
        Self {
            event,
            dispatch: TimeEventDispatch::Cleanup(lease),
        }
    }

    /// Resolves and runs this message on the receiving thread.
    ///
    /// Messages for a `RustLocal` callback must be dispatched on the thread
    /// where the callback was registered. Dispatching them elsewhere drops
    /// the event and returns `false`.
    ///
    /// Returns `true` when a callback was dispatched. Cleanup messages and
    /// wrong-thread registered messages return `false`.
    pub fn dispatch(self) -> bool {
        let Self { event, dispatch } = self;
        match dispatch {
            TimeEventDispatch::Direct(callback) => {
                TimeEventHandler::new(event, callback.into_callback()).run();
                true
            }
            TimeEventDispatch::Registered(lease) => {
                if lease.0.owner != thread::current().id() {
                    log::error!(
                        "Dropping time event '{}' drained outside its callback owner thread",
                        event.name
                    );
                    return false;
                }
                let callback = TIME_EVENT_CALLBACKS.with(|callbacks| {
                    callbacks
                        .borrow()
                        .get(&lease.0.id)
                        .map(|entry| entry.callback.clone())
                });

                if let Some(callback) = callback {
                    TimeEventHandler::new(event, callback).run();
                    true
                } else {
                    log::error!("Dropping time event with an unregistered callback token");
                    false
                }
            }
            #[cfg(any(feature = "live", test))]
            TimeEventDispatch::Cleanup(lease) => {
                if lease.0.owner != thread::current().id() {
                    log::error!("Dropping timer cleanup message outside its callback owner thread");
                }
                false
            }
        }
    }
}

#[cfg(any(feature = "live", test))]
pub(crate) fn register_time_event_callback(callback: TimeEventCallback) -> TimeEventCallbackToken {
    TimeEventCallbackToken::register(callback)
}

pub(crate) fn purge_closed_time_event_callbacks() {
    TIME_EVENT_CALLBACKS.with(|callbacks| {
        callbacks.borrow_mut().retain(|_, entry| {
            entry
                .token
                .upgrade()
                .is_some_and(|token| token.state.load(Ordering::Acquire) != CALLBACK_CLOSED)
        });
    });
}

/// Trait for data command sending that can be implemented for both sync and async runners.
pub trait DataCommandSender {
    /// Executes a data command.
    ///
    /// - **Sync runners** send the command to a queue for synchronous execution.
    /// - **Async runners** send the command to a channel for asynchronous execution.
    fn execute(&self, command: DataCommand);
}

/// Synchronous [`DataCommandSender`] for backtest environments.
///
/// Buffers commands in a thread-local queue for deferred execution,
/// avoiding `RefCell` re-entrancy when sent from event handler callbacks.
#[derive(Debug)]
pub struct SyncDataCommandSender;

impl DataCommandSender for SyncDataCommandSender {
    fn execute(&self, command: DataCommand) {
        DATA_CMD_QUEUE.with(|q| q.borrow_mut().push(command));
    }
}

/// Drain all buffered data commands, dispatching each to the data engine.
pub fn drain_data_cmd_queue() {
    DATA_CMD_QUEUE.with(|q| {
        let commands: Vec<DataCommand> = q.borrow_mut().drain(..).collect();
        let endpoint = MessagingSwitchboard::data_engine_execute();
        for cmd in commands {
            msgbus::send_data_command(endpoint, cmd);
        }
    });
}

/// Returns `true` if the data command queue is empty.
pub fn data_cmd_queue_is_empty() -> bool {
    DATA_CMD_QUEUE.with(|q| q.borrow().is_empty())
}

/// Gets the global data command sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_data_cmd_sender() -> Arc<dyn DataCommandSender> {
    DATA_CMD_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("Data command sender should be initialized by runner")
            .clone()
    })
}

/// Sets the global data command sender.
///
/// This should be called by the runner when it initializes.
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_data_cmd_sender(sender: Arc<dyn DataCommandSender>) {
    DATA_CMD_SENDER.with(|s| {
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "Data command sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global data command sender for the current thread.
pub fn replace_data_cmd_sender(sender: Arc<dyn DataCommandSender>) {
    DATA_CMD_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

/// Trait for time event sending that can be implemented for both sync and async runners.
///
/// Implementations may transfer messages across threads, but messages for
/// `RustLocal` callbacks must be dispatched on the callback's owner thread.
pub trait TimeEventSender: Debug + Send + Sync {
    /// Sends a live time event message.
    fn send(&self, message: TimeEventMessage);
}

/// Gets the global time event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_time_event_sender() -> Arc<dyn TimeEventSender> {
    TIME_EVENT_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("Time event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global time event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_time_event_sender() -> Option<Arc<dyn TimeEventSender>> {
    TIME_EVENT_SENDER.with(|sender| sender.borrow().as_ref().cloned())
}

/// Sets the global time event sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_time_event_sender(sender: Arc<dyn TimeEventSender>) {
    TIME_EVENT_SENDER.with(|s| {
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "Time event sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global time event sender for the current thread.
pub fn replace_time_event_sender(sender: Arc<dyn TimeEventSender>) {
    TIME_EVENT_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

/// Trait for trading command sending that can be implemented for both sync and async runners.
pub trait TradingCommandSender {
    /// Executes a trading command.
    ///
    /// - **Sync runners** send the command to a queue for synchronous execution.
    /// - **Async runners** send the command to a channel for asynchronous execution.
    fn execute(&self, command: TradingCommand);
}

/// Synchronous [`TradingCommandSender`] for backtest environments.
///
/// Buffers commands in a thread-local queue for deferred execution,
/// avoiding `RefCell` re-entrancy when sent from event handler callbacks.
#[derive(Debug)]
pub struct SyncTradingCommandSender;

impl TradingCommandSender for SyncTradingCommandSender {
    fn execute(&self, command: TradingCommand) {
        TRADING_CMD_QUEUE.with(|q| q.borrow_mut().push(command));
    }
}

/// Drain all buffered trading commands, dispatching each to the exec engine.
pub fn drain_trading_cmd_queue() {
    TRADING_CMD_QUEUE.with(|q| {
        let commands: Vec<TradingCommand> = q.borrow_mut().drain(..).collect();
        let endpoint = MessagingSwitchboard::exec_engine_execute();
        for cmd in commands {
            msgbus::send_trading_command(endpoint, cmd);
        }
    });
}

/// Returns `true` if the trading command queue is empty.
pub fn trading_cmd_queue_is_empty() -> bool {
    TRADING_CMD_QUEUE.with(|q| q.borrow().is_empty())
}

/// Gets the global trading command sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_trading_cmd_sender() -> Arc<dyn TradingCommandSender> {
    EXEC_CMD_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("Trading command sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global trading command sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_trading_cmd_sender() -> Option<Arc<dyn TradingCommandSender>> {
    EXEC_CMD_SENDER.with(|sender| sender.borrow().as_ref().cloned())
}

/// Sets the global trading command sender.
///
/// This should be called by the runner when it initializes.
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_exec_cmd_sender(sender: Arc<dyn TradingCommandSender>) {
    EXEC_CMD_SENDER.with(|s| {
        let mut slot = s.borrow_mut();
        assert!(
            slot.is_none(),
            "Trading command sender can only be set once"
        );
        *slot = Some(sender);
    });
}

/// Replaces the global trading command sender for the current thread.
pub fn replace_exec_cmd_sender(sender: Arc<dyn TradingCommandSender>) {
    EXEC_CMD_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

thread_local! {
    static TIME_EVENT_CALLBACKS: RefCell<AHashMap<TimeEventCallbackId, TimeEventCallbackEntry>> = RefCell::new(AHashMap::new());
    static TIME_EVENT_SENDER: RefCell<Option<Arc<dyn TimeEventSender>>> = const { RefCell::new(None) };
    static DATA_CMD_SENDER: RefCell<Option<Arc<dyn DataCommandSender>>> = const { RefCell::new(None) };
    static EXEC_CMD_SENDER: RefCell<Option<Arc<dyn TradingCommandSender>>> = const { RefCell::new(None) };
    static DATA_CMD_QUEUE: RefCell<Vec<DataCommand>> = const { RefCell::new(Vec::new()) };
    static TRADING_CMD_QUEUE: RefCell<Vec<TradingCommand>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use nautilus_core::{UUID4, UnixNanos};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[derive(Debug)]
    struct NoopTimeEventSender;

    impl TimeEventSender for NoopTimeEventSender {
        fn send(&self, _message: TimeEventMessage) {}
    }

    fn event(name: &str) -> TimeEvent {
        TimeEvent::new(
            Ustr::from(name),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
    }

    fn local_callback(count: Rc<Cell<usize>>) -> TimeEventCallback {
        TimeEventCallback::RustLocal(Rc::new(move |_| count.set(count.get() + 1)))
    }

    #[rstest]
    fn test_time_event_message_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<TimeEventMessage>();
    }

    #[rstest]
    fn test_registered_time_event_dispatches_on_owner_thread() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let lease = token.acquire().unwrap();
        let message = TimeEventMessage::registered(event("same-thread"), lease);

        assert!(message.dispatch());
        assert_eq!(count.get(), 1);
        assert!(token.is_registered());

        token.close();
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_registered_time_event_dropped_on_wrong_thread() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let lease = token.acquire().unwrap();
        let message = TimeEventMessage::registered(event("wrong-thread"), lease);

        let dispatched = std::thread::spawn(move || message.dispatch())
            .join()
            .unwrap();

        assert!(!dispatched);
        assert_eq!(count.get(), 0);
        assert!(token.is_registered());

        token.close();
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_closing_registered_callback_without_leases_removes_it_immediately() {
        let token = register_time_event_callback(local_callback(Rc::new(Cell::new(0))));
        assert!(!token.is_closed());

        token.close();

        assert!(token.is_closed());
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_closing_registered_callback_preserves_queued_leases_until_last_dispatch() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let first = TimeEventMessage::registered(event("first"), token.acquire().unwrap());
        let second = TimeEventMessage::registered(event("second"), token.acquire().unwrap());

        token.close();
        assert!(token.is_registered());

        assert!(first.dispatch());
        assert_eq!(count.get(), 1);
        assert!(token.is_registered());

        assert!(second.dispatch());
        assert_eq!(count.get(), 2);
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_replaced_registered_callbacks_have_distinct_lifecycles() {
        let old_count = Rc::new(Cell::new(0));
        let old = register_time_event_callback(local_callback(old_count.clone()));
        let old_message = TimeEventMessage::registered(event("same-name"), old.acquire().unwrap());
        old.close();

        let new_count = Rc::new(Cell::new(0));
        let new = register_time_event_callback(local_callback(new_count.clone()));

        assert_ne!(old.0.id, new.0.id);
        assert!(old_message.dispatch());
        assert_eq!(old_count.get(), 1);
        assert_eq!(new_count.get(), 0);
        assert!(new.is_registered());

        new.close();
        assert!(!new.is_registered());
    }

    #[rstest]
    fn test_one_shot_callback_can_rearm_same_name_without_old_lease_removing_new_callback() {
        let replacement = Rc::new(RefCell::new(None));
        let replacement_slot = replacement.clone();
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            let token = register_time_event_callback(TimeEventCallback::RustLocal(Rc::new(|_| {})));
            replacement_slot.replace(Some(token));
        }));
        let old = register_time_event_callback(callback);
        let message = TimeEventMessage::registered(event("rearm"), old.acquire().unwrap());
        old.close();

        assert!(message.dispatch());
        assert!(!old.is_registered());

        let new = replacement.borrow_mut().take().unwrap();
        assert_ne!(old.0.id, new.0.id);
        assert!(new.is_registered());
        new.close();
        assert!(!new.is_registered());
    }

    #[rstest]
    fn test_wrong_thread_final_lease_is_lazily_purged_on_owner_thread() {
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(|_| {});
        let callback_weak = Rc::downgrade(&callback);
        let token = register_time_event_callback(TimeEventCallback::RustLocal(callback));
        let message = TimeEventMessage::registered(event("lazy-purge"), token.acquire().unwrap());
        token.close();
        drop(token);

        let dispatched = std::thread::spawn(move || message.dispatch())
            .join()
            .unwrap();

        assert!(!dispatched);
        assert!(callback_weak.upgrade().is_some());

        let next = register_time_event_callback(local_callback(Rc::new(Cell::new(0))));
        assert!(callback_weak.upgrade().is_none());
        next.close();
    }

    #[rstest]
    #[cfg(any(feature = "live", test))]
    fn test_cleanup_message_removes_callback_without_dispatching() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let cleanup = TimeEventMessage::cleanup(event("cleanup"), token.acquire().unwrap());
        token.close();

        assert!(!cleanup.dispatch());
        assert_eq!(count.get(), 0);
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_purge_retains_closed_entry_while_final_lease_is_queued() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let message = TimeEventMessage::registered(event("purge-queued"), token.acquire().unwrap());
        token.close();

        purge_closed_time_event_callbacks();
        assert!(token.is_registered());

        assert!(message.dispatch());
        assert_eq!(count.get(), 1);
        assert!(!token.is_registered());
    }

    #[rstest]
    fn test_off_owner_final_lease_drop_is_reclaimed_by_owner_purge() {
        let count = Rc::new(Cell::new(0));
        let token = register_time_event_callback(local_callback(count.clone()));
        let lease = token.acquire().unwrap();
        token.close();

        std::thread::spawn(move || drop(lease)).join().unwrap();

        assert!(token.is_registered());

        purge_closed_time_event_callbacks();
        assert!(!token.is_registered());
        assert_eq!(count.get(), 0);
    }

    #[rstest]
    fn test_replace_data_cmd_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
            replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
            let _sender = get_data_cmd_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_replace_exec_cmd_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            let _sender = get_trading_cmd_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_replace_time_event_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_time_event_sender(Arc::new(NoopTimeEventSender));
            replace_time_event_sender(Arc::new(NoopTimeEventSender));
            let _sender = get_time_event_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_set_data_cmd_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_data_cmd_sender(Arc::new(SyncDataCommandSender));
            set_data_cmd_sender(Arc::new(SyncDataCommandSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_exec_cmd_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            set_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_time_event_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_time_event_sender(Arc::new(NoopTimeEventSender));
            set_time_event_sender(Arc::new(NoopTimeEventSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_try_get_time_event_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_time_event_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }

    #[rstest]
    fn test_try_get_trading_cmd_sender_returns_none_when_unset() {
        let is_none = std::thread::spawn(|| try_get_trading_cmd_sender().is_none())
            .join()
            .unwrap();
        assert!(is_none);
    }
}
