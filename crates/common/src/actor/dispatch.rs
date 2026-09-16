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

//! Private callback accounting and causal context; queued actor delivery remains inactive.

#![allow(
    dead_code,
    reason = "runtime activation requires safe native and Python drain boundaries"
)]

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::mpsc,
    thread::{self, ThreadId},
};

use ahash::AHashMap;

const MAX_PENDING: usize = 65_536;
const MAX_KNOWN_BYTES: usize = 64 * 1024 * 1024;
const MAX_CHAIN: usize = 1_048_576;
const CHAIN_BYTES: usize = size_of::<Chain>() + 2 * size_of::<usize>();

thread_local! {
    static COMMAND_CONTEXTS: RefCell<Option<CommandContexts>> = const { RefCell::new(None) };
    static DISPATCH: RefCell<Dispatcher> = RefCell::new(Dispatcher::default());
}

/// A callback admission, delivery, or boundary failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    #[error("Callback storage limit exceeded")]
    Overflow,
    #[error("Invalid callback destination")]
    InvalidDestination,
    #[error("Callback publication sequence exhausted")]
    SequenceExhausted,
    #[error("Callback publication unwound")]
    PublicationUnwound,
    #[error("Callback delivery unwound")]
    DeliveryUnwound,
    #[error("Callback chain delivery limit exceeded")]
    Runaway,
    #[error("Callback delivery stalled at a safe boundary")]
    Stalled,
    #[error("Callback work or access is still active")]
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DrainResult {
    pub(super) status: DrainStatus,
    pub(super) delivered: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DrainStatus {
    Empty,
    BudgetExhausted,
    Deferred,
    Reserved,
    Busy,
}

pub(super) struct PublicationScope {
    active: bool,
    previous: Option<u64>,
    previous_chain: Option<Rc<Chain>>,
    marker: std::marker::PhantomData<Rc<()>>,
}

impl PublicationScope {
    pub(super) fn enter() -> Self {
        let previous = DISPATCH.try_with(|state| {
            let mut state = state.borrow_mut();
            let ordinal = state.sequence();
            state.depth += 1;
            let previous_chain = state.current.clone();
            (
                std::mem::replace(&mut state.publication, ordinal),
                previous_chain,
            )
        });

        let active = previous.is_ok();
        let (previous, previous_chain) = previous.unwrap_or_default();

        Self {
            active,
            previous,
            previous_chain,
            marker: std::marker::PhantomData,
        }
    }
}

impl Drop for PublicationScope {
    fn drop(&mut self) {
        if self.active {
            let _ = DISPATCH.try_with(|state| {
                let mut state = state.borrow_mut();
                state.publication = self.previous;
                state.depth -= 1;

                if (state.depth == 0 && state.chain_depth == 0) || self.previous_chain.is_some() {
                    state.current = self.previous_chain.take();
                }

                if std::thread::panicking() {
                    state.fail(DispatchError::PublicationUnwound);
                }
            });
        }
    }
}

// Reserve before constructing owned captures; committing never rejects transferred ownership
pub(super) fn reserve<T: 'static>(heap_bytes: usize) -> Option<Admission<T>> {
    DISPATCH
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if state.error.is_some() || state.clearing {
                return None;
            }

            let chain = state.chain()?;
            let bytes = heap_bytes
                .checked_add(size_of::<Delivery<T>>())
                .and_then(|bytes| bytes.checked_add(size_of::<Slot>() + 2 * size_of::<usize>()));
            let capacity = state.pending.capacity().max(state.pending.len() + 1);

            let Some(bytes) = bytes.filter(|bytes| state.fits(*bytes, capacity)) else {
                state.fail(DispatchError::Overflow);
                return None;
            };

            let publication = match state.publication {
                Some(value) => value,
                None => state.sequence()?,
            };

            let key = (publication, state.sequence()?);
            if state.pending.try_reserve_exact(1).is_err()
                || !state.fits(bytes, state.pending.capacity())
            {
                state.fail(DispatchError::Overflow);
                return None;
            }

            let accounting = state.accounting.clone();
            accounting.count.set(accounting.count.get() + 1);
            accounting.bytes.set(accounting.bytes.get() + bytes);
            accounting
                .reservations
                .set(accounting.reservations.get() + 1);

            let slot = Rc::new(Slot {
                state: RefCell::new(SlotState::Reserved),
                chain,
                bytes,
                accounting,
            });

            let position = state.pending.partition_point(|(queued, _)| queued < &key);
            state.pending.insert(position, (key, slot.clone()));
            Some(Admission {
                slot,
                marker: std::marker::PhantomData,
            })
        })
        .ok()
        .flatten()
}

pub(super) struct Admission<T> {
    slot: Rc<Slot>,
    marker: std::marker::PhantomData<T>,
}

impl<T: 'static> Admission<T> {
    pub(super) fn commit(self, value: T, run: fn(&mut T) -> bool) {
        let delivery = Box::new(Delivery { value, run });
        *self.slot.state.borrow_mut() = SlotState::Ready(delivery);
    }
}

impl<T> Drop for Admission<T> {
    fn drop(&mut self) {
        let mut state = self.slot.state.borrow_mut();
        if matches!(*state, SlotState::Reserved) {
            *state = SlotState::Cancelled;
        }

        self.slot
            .accounting
            .reservations
            .set(self.slot.accounting.reservations.get() - 1);
    }
}

// Callers must release enclosing component, engine, and cache borrows before entering
pub(super) fn drain_at_boundary(budget: usize) -> Result<DrainResult, DispatchError> {
    let result = drain(budget)?;
    match result.status {
        DrainStatus::Deferred | DrainStatus::Reserved => Err(DispatchError::Active),
        DrainStatus::Busy => {
            record_failure(DispatchError::Stalled);
            Err(DispatchError::Stalled)
        }
        DrainStatus::Empty | DrainStatus::BudgetExhausted => Ok(result),
    }
}

pub(super) fn drain(budget: usize) -> Result<DrainResult, DispatchError> {
    let status = DISPATCH
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if let Some(e) = state.error {
                return Err(e);
            }

            if state.draining || state.clearing || state.depth != 0 || super::access::is_active() {
                return Ok(Some(DrainStatus::Deferred));
            }

            // Keep the guard's failure tracking when called during unwinding
            if state.pending.is_empty() && !std::thread::panicking() {
                return Ok(Some(DrainStatus::Empty));
            }

            state.draining = true;
            Ok(None)
        })
        .unwrap_or(Ok(Some(DrainStatus::Deferred)))?;

    if let Some(status) = status {
        return Ok(DrainResult {
            status,
            delivered: 0,
        });
    }

    let _scope = DrainScope;
    let mut delivered = 0;
    let mut processed = 0;
    let mut status = DrainStatus::BudgetExhausted;

    while processed < budget {
        let slot = DISPATCH.with_borrow_mut(|state| {
            if let Some(e) = state.error {
                return Err(e);
            }

            Ok(state.pending.front().map(|(_, slot)| slot.clone()))
        })?;

        let Some(slot) = slot else {
            break;
        };

        let _chain = ChainScope::enter(Some(slot.chain.clone()));
        let pending = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Reserved);
        match pending {
            SlotState::Reserved => {
                status = DrainStatus::Reserved;
                break;
            }
            SlotState::Cancelled => {}
            SlotState::Ready(mut delivery) => {
                if slot.chain.delivered.get() >= MAX_CHAIN {
                    *slot.state.borrow_mut() = SlotState::Ready(delivery);
                    record_failure(DispatchError::Runaway);
                    return Err(DispatchError::Runaway);
                }

                if !delivery.run() {
                    *slot.state.borrow_mut() = SlotState::Ready(delivery);
                    status = DrainStatus::Busy;
                    break;
                }

                // Callback-local guards end before the owned capture is destroyed
                drop(delivery);
                delivered += 1;
                slot.chain.delivered.set(slot.chain.delivered.get() + 1);
            }
        }

        processed += 1;
        let removed = DISPATCH.with_borrow_mut(|state| state.pending.pop_front());
        drop(removed);
        drop(slot);
    }

    DISPATCH.with_borrow_mut(|state| {
        if let Some(e) = state.error {
            return Err(e);
        }

        Ok(DrainResult {
            status: if state.pending.is_empty() {
                DrainStatus::Empty
            } else {
                status
            },
            delivered,
        })
    })
}

pub(super) fn retain(bytes: usize) -> Option<RetainedStorage> {
    DISPATCH
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if state.error.is_some() || state.clearing {
                return None;
            }

            let chain = state.chain()?;

            if !state.fits(bytes, state.pending.capacity()) {
                state.fail(DispatchError::Overflow);
                return None;
            }

            let accounting = state.accounting.clone();
            accounting.count.set(accounting.count.get() + 1);
            accounting.bytes.set(accounting.bytes.get() + bytes);
            Some(RetainedStorage {
                chain,
                accounting,
                bytes,
            })
        })
        .ok()
        .flatten()
}

pub(super) fn record_failure(error: DispatchError) {
    let _ = DISPATCH.try_with(|state| state.borrow_mut().fail(error));
}

pub(super) struct RetainedStorage {
    chain: Rc<Chain>,
    accounting: Rc<Accounting>,
    bytes: usize,
}

impl RetainedStorage {
    pub(super) fn with_chain<T>(&self, run: impl FnOnce() -> T) -> T {
        let _scope = ChainScope::enter(Some(self.chain.clone()));
        run()
    }

    pub(super) fn grow(&mut self, bytes: usize) -> Option<()> {
        if bytes == 0 {
            return Some(());
        }

        let mut extra = self.with_chain(|| retain(bytes))?;
        self.bytes += bytes;
        extra.bytes = 0;
        Some(())
    }
}

impl Drop for RetainedStorage {
    fn drop(&mut self) {
        self.accounting.count.set(self.accounting.count.get() - 1);
        self.accounting
            .bytes
            .set(self.accounting.bytes.get() - self.bytes);
    }
}

pub(super) fn failure() -> Option<DispatchError> {
    DISPATCH
        .try_with(|state| state.borrow().error)
        .ok()
        .flatten()
}

pub(super) fn has_pending() -> bool {
    collect_command_contexts();

    DISPATCH
        .try_with(|state| {
            let state = state.borrow();
            state.accounting.count.get() != 0
                || state.accounting.contexts.get() != 0
                || state.error.is_some()
        })
        .unwrap_or(false)
}

pub(super) fn clear() -> Result<(), DispatchError> {
    collect_command_contexts();

    let previous = DISPATCH
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if state.clearing {
                return Ok(None);
            }

            if state.current.is_some()
                || state.draining
                || state.depth != 0
                || state.chain_depth != 0
                || state.accounting.contexts.get() != 0
                || state.accounting.reservations.get() != 0
                || state.accounting.count.get() != state.pending.len()
                || super::access::is_active()
            {
                return Err(DispatchError::Active);
            }

            let previous = std::mem::take(&mut *state);
            state.clearing = true;
            Ok(Some(previous))
        })
        .unwrap_or(Ok(None))?;

    if let Some(previous) = previous {
        let _scope = ClearScope;
        drop(previous);
    }

    Ok(())
}

trait Callback {
    fn run(&mut self) -> bool;
}

struct Delivery<T> {
    value: T,
    run: fn(&mut T) -> bool,
}

impl<T> Callback for Delivery<T> {
    fn run(&mut self) -> bool {
        (self.run)(&mut self.value)
    }
}

enum SlotState {
    Reserved,
    Cancelled,
    Ready(Box<dyn Callback>),
}

struct Slot {
    chain: Rc<Chain>,
    state: RefCell<SlotState>,
    bytes: usize,
    accounting: Rc<Accounting>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        // Drop captures before returning their storage charge, without borrowing shared state
        let pending = std::mem::replace(self.state.get_mut(), SlotState::Cancelled);
        drop(pending);
        self.accounting.count.set(self.accounting.count.get() - 1);
        self.accounting
            .bytes
            .set(self.accounting.bytes.get() - self.bytes);
    }
}

#[derive(Default)]
struct Accounting {
    count: Cell<usize>,
    bytes: Cell<usize>,
    reservations: Cell<usize>,
    contexts: Cell<usize>,
}

#[derive(Default)]
struct Dispatcher {
    pending: VecDeque<((u64, u64), Rc<Slot>)>,
    accounting: Rc<Accounting>,
    sequence: u64,
    publication: Option<u64>,
    depth: usize,
    chain_depth: usize,
    draining: bool,
    clearing: bool,
    current: Option<Rc<Chain>>,
    error: Option<DispatchError>,
}

impl Dispatcher {
    fn chain(&mut self) -> Option<Rc<Chain>> {
        if self.error.is_some() || self.clearing {
            return None;
        }

        if let Some(chain) = &self.current {
            return Some(chain.clone());
        }

        if !self.fits(CHAIN_BYTES, self.pending.capacity()) {
            self.fail(DispatchError::Overflow);
            return None;
        }

        self.accounting
            .bytes
            .set(self.accounting.bytes.get() + CHAIN_BYTES);

        let chain = Rc::new(Chain {
            delivered: Cell::new(0),
            accounting: self.accounting.clone(),
        });

        if self.depth != 0 || self.chain_depth != 0 {
            self.current = Some(chain.clone());
        }

        Some(chain)
    }

    fn sequence(&mut self) -> Option<u64> {
        if let Some(value) = self.sequence.checked_add(1) {
            self.sequence = value;
            Some(value)
        } else {
            self.fail(DispatchError::SequenceExhausted);
            None
        }
    }

    fn fits(&self, bytes: usize, capacity: usize) -> bool {
        self.accounting.count.get() < MAX_PENDING
            && bytes
                <= MAX_KNOWN_BYTES.saturating_sub(
                    self.accounting.bytes.get().saturating_add(
                        capacity.saturating_mul(size_of::<((u64, u64), Rc<Slot>)>()),
                    ),
                )
    }

    fn fail(&mut self, error: DispatchError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
}

struct Chain {
    delivered: Cell<usize>,
    accounting: Rc<Accounting>,
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.accounting
            .bytes
            .set(self.accounting.bytes.get() - CHAIN_BYTES);
    }
}

pub(crate) struct ChainContext {
    chain: Option<Rc<Chain>>,
}

impl ChainContext {
    pub(crate) const fn independent() -> Self {
        Self { chain: None }
    }

    pub(crate) fn capture() -> Self {
        let chain = DISPATCH
            .try_with(|state| {
                let mut state = state.borrow_mut();

                let chain = state.current.clone().or_else(|| {
                    if state.depth != 0 || state.chain_depth != 0 {
                        state.chain()
                    } else {
                        None
                    }
                });

                if let Some(chain) = &chain {
                    chain
                        .accounting
                        .contexts
                        .set(chain.accounting.contexts.get() + 1);
                }

                chain
            })
            .ok()
            .flatten();

        Self { chain }
    }

    pub(crate) fn with_chain<T>(&self, run: impl FnOnce() -> T) -> T {
        let _scope = ChainScope::enter(self.chain.clone());
        run()
    }
}

impl Drop for ChainContext {
    fn drop(&mut self) {
        if let Some(chain) = &self.chain {
            chain
                .accounting
                .contexts
                .set(chain.accounting.contexts.get() - 1);
        }
    }
}

#[derive(Debug)]
pub(crate) struct SendChainContext {
    id: u64,
    owner: ThreadId,
    released: mpsc::Sender<u64>,
}

impl SendChainContext {
    pub(crate) fn capture() -> Option<Self> {
        collect_command_contexts();
        let context = ChainContext::capture();
        context.chain.as_ref()?;

        COMMAND_CONTEXTS
            .try_with(|contexts| {
                let mut contexts = contexts.borrow_mut();
                let contexts = contexts.get_or_insert_with(CommandContexts::default);
                contexts.next = contexts
                    .next
                    .checked_add(1)
                    .expect("command context IDs exhausted");
                let id = contexts.next;
                contexts.pending.insert(id, context);

                Self {
                    id,
                    owner: thread::current().id(),
                    released: contexts.released.clone(),
                }
            })
            .ok()
    }

    pub(crate) fn take(&self) -> Option<ChainContext> {
        assert_eq!(
            self.owner,
            thread::current().id(),
            "command context dispatched outside its owner thread"
        );
        COMMAND_CONTEXTS
            .try_with(|contexts| {
                contexts
                    .borrow_mut()
                    .as_mut()
                    .and_then(|contexts| contexts.pending.remove(&self.id))
            })
            .ok()
            .flatten()
    }

    pub(crate) fn is_owner(&self) -> bool {
        self.owner == thread::current().id()
    }
}

impl Drop for SendChainContext {
    fn drop(&mut self) {
        if self.is_owner() {
            drop(self.take());
        } else {
            // Foreign receivers release only an ID; the owner retains every Rc access
            let _ = self.released.send(self.id);
        }
    }
}

struct CommandContexts {
    next: u64,
    pending: AHashMap<u64, ChainContext>,
    released: mpsc::Sender<u64>,
    releases: mpsc::Receiver<u64>,
}

impl Default for CommandContexts {
    fn default() -> Self {
        let (released, releases) = mpsc::channel();

        Self {
            next: 0,
            pending: AHashMap::new(),
            released,
            releases,
        }
    }
}

pub(crate) fn collect_command_contexts() {
    let _ = COMMAND_CONTEXTS.try_with(|contexts| {
        let mut contexts = contexts.borrow_mut();

        let Some(contexts) = contexts.as_mut() else {
            return;
        };

        while let Ok(id) = contexts.releases.try_recv() {
            contexts.pending.remove(&id);
        }
    });
}

struct ChainScope {
    active: bool,
    previous: Option<Rc<Chain>>,
}

impl ChainScope {
    fn enter(chain: Option<Rc<Chain>>) -> Self {
        let previous = DISPATCH.try_with(|state| {
            let mut state = state.borrow_mut();
            state.chain_depth += 1;
            std::mem::replace(&mut state.current, chain)
        });

        Self {
            active: previous.is_ok(),
            previous: previous.unwrap_or_default(),
        }
    }
}

impl Drop for ChainScope {
    fn drop(&mut self) {
        if self.active {
            let _ = DISPATCH.try_with(|state| {
                let mut state = state.borrow_mut();
                state.current = self.previous.take();
                state.chain_depth -= 1;
            });
        }
    }
}

struct DrainScope;
impl Drop for DrainScope {
    fn drop(&mut self) {
        let _ = DISPATCH.try_with(|state| {
            let mut state = state.borrow_mut();
            state.draining = false;
            if std::thread::panicking() {
                state.fail(DispatchError::DeliveryUnwound);
            }
        });
    }
}

struct ClearScope;
impl Drop for ClearScope {
    fn drop(&mut self) {
        let _ = DISPATCH.try_with(|state| state.borrow_mut().clearing = false);
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::{
        actor::{callback_failure, clear_callbacks, drain_callbacks},
        messages::{
            data::{DataCommand, SubscribeCommand, SubscribeQuotes},
            execution::{QueryAccount, TradingCommand},
        },
        msgbus::{self, MessagingSwitchboard, TypedIntoHandler},
        runner::{
            DataCommandSender, SyncDataCommandSender, SyncTradingCommandSender,
            TradingCommandMessage, TradingCommandSender, capture_trading_cmd, clear_command_queues,
            data_cmd_queue_is_empty, drain_data_cmd_queue, drain_trading_cmd_queue,
            trading_cmd_is_dispatching, trading_cmd_queue_is_empty,
        },
    };

    fn record(value: &mut (Rc<RefCell<Vec<u32>>>, u32)) -> bool {
        value.0.borrow_mut().push(value.1);
        true
    }

    #[rstest]
    fn runtime_drain_reports_queued_slots_without_counting_retained_roots() {
        clear_callbacks().unwrap();
        let retained = retain(17).unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        drop(reserve::<()>(0).unwrap());
        reserve(0).unwrap().commit((received.clone(), 23), record);

        assert_eq!(drain_callbacks(0), Ok(true));
        assert_eq!(drain_callbacks(1), Ok(true));
        assert!(received.borrow().is_empty());
        assert_eq!(drain_callbacks(1), Ok(false));
        assert_eq!(*received.borrow(), [23]);
        assert!(has_pending());
        assert_eq!(drain_callbacks(1), Ok(false));
        assert_eq!(clear_callbacks(), Err(DispatchError::Active));
        drop(retained);
        assert_eq!(clear_callbacks(), Ok(()));
    }

    #[rstest]
    fn runtime_cleanup_releases_command_roots_before_clearing_failure() {
        clear_callbacks().unwrap();
        let storage = retain(17).unwrap();
        let chain = Rc::downgrade(&storage.chain);
        storage.with_chain(|| {
            SyncDataCommandSender.execute(data_command(1));
            SyncTradingCommandSender.execute(trading_message(2));
        });
        drop(storage);
        reserve(0).unwrap().commit((), |()| false);

        assert_eq!(drain_callbacks(1), Err(DispatchError::Stalled));
        assert_eq!(callback_failure(), Some(DispatchError::Stalled));
        assert_eq!(clear_callbacks(), Err(DispatchError::Active));
        assert!(!data_cmd_queue_is_empty());
        assert!(!trading_cmd_queue_is_empty());
        assert_eq!(chain.strong_count(), 2);
        clear_command_queues();
        assert!(data_cmd_queue_is_empty());
        assert!(trading_cmd_queue_is_empty());
        assert_eq!(chain.strong_count(), 0);
        assert_eq!(callback_failure(), Some(DispatchError::Stalled));
        assert_eq!(clear_callbacks(), Ok(()));
        assert_eq!(callback_failure(), None);
        assert_eq!(drain_callbacks(1), Ok(false));
    }

    #[rstest]
    fn retained_storage_growth_releases_exact_charge() {
        clear().unwrap();
        let mut storage = retain(17).unwrap();
        storage.grow(29).unwrap();
        let retained = DISPATCH
            .with_borrow(|state| (state.accounting.count.get(), state.accounting.bytes.get()));
        drop(storage);
        let released = DISPATCH
            .with_borrow(|state| (state.accounting.count.get(), state.accounting.bytes.get()));
        assert_eq!(retained, (1, 46 + CHAIN_BYTES));
        assert_eq!(released, (0, 0));
        assert_eq!(failure(), None);
        clear().unwrap();
    }

    #[rstest]
    fn nested_publication_reserves_all_outer_recipients() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        {
            let _outer = PublicationScope::enter();
            reserve(0).unwrap().commit((received.clone(), 11), record);
            {
                let _nested = PublicationScope::enter();
                reserve(0).unwrap().commit((received.clone(), 21), record);
            }

            reserve(0).unwrap().commit((received.clone(), 12), record);
            assert_eq!(drain_at_boundary(10), Err(DispatchError::Active));
            assert_eq!(failure(), None);
            assert_eq!(
                drain(10),
                Ok(DrainResult {
                    status: DrainStatus::Deferred,
                    delivered: 0
                })
            );
        }

        assert_eq!(
            drain(10),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 3
            })
        );
        assert_eq!(*received.borrow(), [11, 12, 21]);
    }

    #[rstest]
    #[case(false, false)]
    #[case(false, true)]
    #[case(true, false)]
    #[case(true, true)]
    fn reserved_head_blocks_delivery_and_teardown(
        #[case] preceding_delivery: bool,
        #[case] at_boundary: bool,
    ) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));

        if preceding_delivery {
            reserve(0).unwrap().commit((received.clone(), 7), record);
        }

        let head = reserve(0).unwrap();
        reserve(0).unwrap().commit((received.clone(), 22), record);

        let result = if at_boundary {
            drain_at_boundary(3)
        } else {
            drain(3)
        };

        let expected = if at_boundary {
            Err(DispatchError::Active)
        } else {
            Ok(DrainResult {
                status: DrainStatus::Reserved,
                delivered: usize::from(preceding_delivery),
            })
        };

        assert_eq!(result, expected);
        assert_eq!(
            *received.borrow(),
            if preceding_delivery { vec![7] } else { vec![] }
        );
        assert_eq!(drain_at_boundary(2), Err(DispatchError::Active));
        assert_eq!(failure(), None);
        assert_eq!(clear(), Err(DispatchError::Active));
        head.commit((received.clone(), 11), record);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 2
            })
        );
        assert_eq!(
            *received.borrow(),
            if preceding_delivery {
                vec![7, 11, 22]
            } else {
                vec![11, 22]
            }
        );
    }

    #[rstest]
    fn successful_reservation_commits_after_fatal_latch() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let admission = reserve(0).unwrap();
        assert!(reserve::<u8>(MAX_KNOWN_BYTES).is_none());
        admission.commit((received.clone(), 11), record);
        assert_eq!(drain(1), Err(DispatchError::Overflow));
        assert!(received.borrow().is_empty());
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.count.get()),
            1
        );
        clear().unwrap();
        assert!(!has_pending());
    }

    #[rstest]
    #[case("outer")]
    #[case("nested")]
    fn bus_fanout_preserves_same_and_cross_topic_order(#[case] nested_topic: &'static str) {
        use crate::msgbus::{self, MessageBus, ShareableMessageHandler};
        clear().unwrap();
        msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
        let received = Rc::new(RefCell::new(Vec::new()));
        for recipient in [1, 2] {
            let received = received.clone();
            msgbus::subscribe_any(
                "*".into(),
                ShareableMessageHandler::from_typed(move |value: &u32| {
                    reserve(0)
                        .unwrap()
                        .commit((received.clone(), value * 10 + recipient), record);
                    if *value == 1 && recipient == 1 {
                        let _nested = PublicationScope::enter();
                        msgbus::publish_any(nested_topic.into(), &2_u32);
                    }
                }),
                Some(3 - recipient),
            );
        }

        {
            let _publication = PublicationScope::enter();
            msgbus::publish_any("outer".into(), &1_u32);
        }

        assert_eq!(
            drain(10),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 4
            })
        );
        assert_eq!(*received.borrow(), [11, 12, 21, 22]);
        msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
    }

    #[rstest]
    fn cancelled_reservation_releases_storage_at_explicit_drain() {
        clear().unwrap();
        drop(reserve::<u64>(113).unwrap());
        assert!(has_pending());
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 0
            })
        );
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
            0
        );
    }

    #[rstest]
    fn event_count_limit_latches_after_exact_capacity() {
        clear().unwrap();

        for _ in 0..MAX_PENDING {
            reserve(0).unwrap().commit((), |()| true);
        }

        drop(PublicationScope::enter());
        assert_eq!(failure(), None);
        assert!(reserve::<()>(0).is_none());
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.count.get()),
            MAX_PENDING
        );
        assert_eq!(drain(1), Err(DispatchError::Overflow));
        clear().unwrap();
    }

    #[rstest]
    fn busy_head_and_recursive_drain_do_not_overtake() {
        clear().unwrap();
        let busy = Rc::new(Cell::new(true));
        reserve(0).unwrap().commit(busy.clone(), |busy| {
            assert_eq!(drain_at_boundary(1), Err(DispatchError::Active));
            assert_eq!(failure(), None);
            assert_eq!(
                drain(1),
                Ok(DrainResult {
                    status: DrainStatus::Deferred,
                    delivered: 0
                })
            );
            !busy.get()
        });

        let received = Rc::new(RefCell::new(Vec::new()));
        reserve(0).unwrap().commit((received.clone(), 22), record);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                status: DrainStatus::Busy,
                delivered: 0
            })
        );
        assert!(received.borrow().is_empty());
        busy.set(false);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 1
            })
        );
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(*received.borrow(), [22]);
    }

    #[rstest]
    fn empty_drain_preserves_unwind_failure() {
        struct DrainOnDrop(Rc<Cell<bool>>);

        impl Drop for DrainOnDrop {
            fn drop(&mut self) {
                assert_eq!(
                    drain_at_boundary(1),
                    Ok(DrainResult {
                        status: DrainStatus::Empty,
                        delivered: 0,
                    })
                );
                assert_eq!(failure(), Some(DispatchError::DeliveryUnwound));
                self.0.set(true);
            }
        }

        clear().unwrap();
        let dropped = Rc::new(Cell::new(false));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _probe = DrainOnDrop(dropped.clone());
            panic!("outer callback failed");
        }));

        assert!(result.is_err());
        assert!(dropped.get());
        assert_eq!(failure(), Some(DispatchError::DeliveryUnwound));
        clear().unwrap();
    }

    #[rstest]
    #[case(false, false, 0)]
    #[case(true, false, 0)]
    #[case(false, true, 0)]
    #[case(false, false, 1)]
    fn empty_drain_preserves_failure_and_boundary_checks(
        #[case] draining: bool,
        #[case] clearing: bool,
        #[case] depth: usize,
        #[values(None, Some(DispatchError::InvalidDestination))] error: Option<DispatchError>,
        #[values(0, 1)] budget: usize,
    ) {
        clear().unwrap();
        DISPATCH.with_borrow_mut(|state| {
            state.draining = draining;
            state.clearing = clearing;
            state.depth = depth;
            state.error = error;
        });

        let result = drain(budget);
        let boundary = drain_at_boundary(budget);
        let observed_error = failure();
        let observed_state =
            DISPATCH.with_borrow(|state| (state.draining, state.clearing, state.depth));
        DISPATCH.with_borrow_mut(|state| {
            state.draining = false;
            state.clearing = false;
            state.depth = 0;
        });
        clear().unwrap();

        let expected_status = if draining || clearing || depth != 0 {
            DrainStatus::Deferred
        } else {
            DrainStatus::Empty
        };
        let expected = error.map_or(
            Ok(DrainResult {
                status: expected_status,
                delivered: 0,
            }),
            Err,
        );
        assert_eq!(result, expected);
        assert_eq!(
            boundary,
            if expected_status == DrainStatus::Deferred {
                Err(error.unwrap_or(DispatchError::Active))
            } else {
                expected
            }
        );
        assert_eq!(observed_error, error);
        assert_eq!(observed_state, (draining, clearing, depth));
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    fn boundary_drain_ignores_retained_ownership_without_slots(#[case] budget: usize) {
        clear().unwrap();
        let storage = retain(17).unwrap();
        let context = storage.with_chain(ChainContext::capture);

        let expected = Ok(DrainResult {
            status: DrainStatus::Empty,
            delivered: 0,
        });

        assert!(has_pending());
        assert_eq!(drain_at_boundary(budget), expected);
        drop(storage);
        assert!(has_pending());
        assert_eq!(drain_at_boundary(budget), expected);
        drop(context);
        assert!(!has_pending());
        assert_eq!(failure(), None);
        clear().unwrap();
    }

    #[rstest]
    #[case(0, 0, DrainStatus::BudgetExhausted)]
    #[case(1, 0, DrainStatus::BudgetExhausted)]
    #[case(2, 1, DrainStatus::Empty)]
    fn boundary_drain_counts_cancelled_slots_toward_budget(
        #[case] budget: usize,
        #[case] delivered: usize,
        #[case] status: DrainStatus,
    ) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        drop(reserve::<()>(0).unwrap());
        reserve(0).unwrap().commit((received.clone(), 17), record);
        assert_eq!(
            drain_at_boundary(budget),
            Ok(DrainResult { status, delivered })
        );
        assert_eq!(
            drain_at_boundary(2),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1 - delivered,
            })
        );
        assert_eq!(*received.borrow(), [17]);
        assert_eq!(failure(), None);
        clear().unwrap();
    }

    #[rstest]
    fn boundary_drain_leaves_busy_head_beyond_budget_unattempted() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(Cell::new(0));
        reserve(0).unwrap().commit((received.clone(), 11), record);
        reserve(0).unwrap().commit(attempts.clone(), |attempts| {
            attempts.set(attempts.get() + 1);
            false
        });

        assert_eq!(
            drain_at_boundary(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 1,
            })
        );
        assert_eq!(*received.borrow(), [11]);
        assert_eq!(attempts.get(), 0);
        assert_eq!(failure(), None);
        assert_eq!(drain_at_boundary(1), Err(DispatchError::Stalled));
        assert_eq!(attempts.get(), 1);
        assert_eq!(failure(), Some(DispatchError::Stalled));
        clear().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn boundary_drain_rejects_active_access_without_latching_failure(#[case] queued: bool) {
        clear().unwrap();
        let allocation = Rc::new(std::cell::UnsafeCell::new(()));
        let guard = super::super::access::AllocationGuard::acquire(allocation).unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        if queued {
            reserve(0).unwrap().commit((received.clone(), 23), record);
        }

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Deferred,
                delivered: 0
            })
        );
        assert_eq!(drain_at_boundary(1), Err(DispatchError::Active));
        assert_eq!(failure(), None);
        assert!(received.borrow().is_empty());
        drop(guard);
        assert_eq!(
            drain_at_boundary(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: usize::from(queued)
            })
        );
        assert_eq!(*received.borrow(), if queued { vec![23] } else { vec![] });
        clear().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn boundary_drain_latches_busy_head_after_progress(#[case] preceding_delivery: bool) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));

        if preceding_delivery {
            reserve(0).unwrap().commit((received.clone(), 11), record);
        }

        let busy = reserve(0).unwrap();
        let chain = Rc::downgrade(&busy.slot.chain);
        busy.commit((), |()| false);
        reserve(0).unwrap().commit((received.clone(), 23), record);

        assert_eq!(drain_at_boundary(3), Err(DispatchError::Stalled));
        assert_eq!(failure(), Some(DispatchError::Stalled));
        assert_eq!(drain_at_boundary(3), Err(DispatchError::Stalled));
        assert_eq!(drain(3), Err(DispatchError::Stalled));
        assert!(reserve::<()>(0).is_none());
        assert_eq!(chain.upgrade().unwrap().delivered.get(), 0);
        assert_eq!(
            *received.borrow(),
            if preceding_delivery { vec![11] } else { vec![] }
        );
        assert_eq!(DISPATCH.with_borrow(|state| state.pending.len()), 2);
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert!(!has_pending());
        assert_eq!(failure(), None);
    }

    #[rstest]
    fn boundary_drain_preserves_first_delivery_failure() {
        clear().unwrap();
        reserve(0).unwrap().commit((), |()| {
            record_failure(DispatchError::InvalidDestination);
            false
        });

        assert_eq!(drain_at_boundary(1), Err(DispatchError::InvalidDestination));
        assert_eq!(failure(), Some(DispatchError::InvalidDestination));
        clear().unwrap();
    }

    struct DropProbe {
        allocation: Rc<std::cell::UnsafeCell<()>>,
        dropped: Rc<Cell<usize>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            let _guard =
                super::super::access::AllocationGuard::acquire(self.allocation.clone()).unwrap();
            self.dropped.set(self.dropped.get() + 1);
            assert_eq!(
                drain(1),
                Ok(DrainResult {
                    status: DrainStatus::Deferred,
                    delivered: 0,
                })
            );
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn delivery_and_unwind_release_capture_outside_access(#[case] unwind: bool) {
        clear().unwrap();
        let allocation = Rc::new(std::cell::UnsafeCell::new(()));
        let dropped = Rc::new(Cell::new(0));
        reserve(0).unwrap().commit(
            (
                DropProbe {
                    allocation,
                    dropped: dropped.clone(),
                },
                unwind,
            ),
            |value| {
                let _guard =
                    super::super::access::AllocationGuard::acquire(value.0.allocation.clone())
                        .unwrap();
                assert!(!value.1, "delivery failed");
                true
            },
        );

        let result = std::panic::catch_unwind(|| drain(1));
        assert_eq!(result.is_err(), unwind);
        assert_eq!(dropped.get(), 1);
        assert_eq!(failure(), unwind.then_some(DispatchError::DeliveryUnwound));
        clear().unwrap();
    }

    #[rstest]
    fn teardown_and_rejected_capture_construction_are_safe() {
        clear().unwrap();
        let allocation = Rc::new(std::cell::UnsafeCell::new(()));
        let dropped = Rc::new(Cell::new(0));
        reserve(0).unwrap().commit(
            DropProbe {
                allocation: allocation.clone(),
                dropped: dropped.clone(),
            },
            |_| true,
        );

        {
            let _guard = super::super::access::AllocationGuard::acquire(allocation).unwrap();
            let rejected = reserve::<DropProbe>(usize::MAX);
            assert!(rejected.is_none());
            assert_eq!(clear(), Err(DispatchError::Active));
            assert_eq!(dropped.get(), 0);
        }

        clear().unwrap();
        assert_eq!(dropped.get(), 1);
        assert_eq!(failure(), None);
    }

    #[rstest]
    fn nested_publication_unwind_restores_frames() {
        clear().unwrap();

        let result = std::panic::catch_unwind(|| {
            let _outer = PublicationScope::enter();
            let _inner = PublicationScope::enter();
            reserve(0).unwrap().commit((), |()| true);
            panic!("publication failed");
        });

        assert!(result.is_err());
        assert_eq!(failure(), Some(DispatchError::PublicationUnwound));
        assert_eq!(
            DISPATCH.with_borrow(|state| (state.depth, state.publication)),
            (0, None)
        );
        clear().unwrap();
    }

    #[rstest]
    fn progress_limit_persists_between_bounded_drains() {
        clear().unwrap();
        {
            let _publication = PublicationScope::enter();
            {
                let _nested = PublicationScope::enter();
                retain(0).unwrap().chain.delivered.set(MAX_CHAIN - 1);
            }

            reserve(0).unwrap().commit((), |()| true);
            reserve(0)
                .unwrap()
                .commit((), |()| panic!("runaway callback must not run"));
        }

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 1
            })
        );
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        clear().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn independent_roots_have_separate_budgets(#[case] publication: bool) {
        clear().unwrap();
        let scope = publication.then(PublicationScope::enter);
        let first = reserve(0).unwrap();
        let first_chain = Rc::downgrade(&first.slot.chain);
        first.slot.chain.delivered.set(MAX_CHAIN - 1);
        first.commit((), |()| true);
        drop(scope);
        let scope = publication.then(PublicationScope::enter);
        let second = reserve(0).unwrap();
        let second_chain = second.slot.chain.clone();
        second.commit((), |()| true);
        drop(scope);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 2
            })
        );
        assert_eq!(second_chain.delivered.get(), 1);
        assert_eq!(first_chain.strong_count(), 0);
        assert_eq!(failure(), None);
        drop(second_chain);
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
            0
        );
        clear().unwrap();
    }

    #[rstest]
    fn nested_callback_publication_inherits_root() {
        clear().unwrap();
        let root = reserve(0).unwrap();
        let chain = root.slot.chain.clone();
        chain.delivered.set(MAX_CHAIN - 1);
        root.commit((), |()| {
            let _publication = PublicationScope::enter();
            reserve(0)
                .unwrap()
                .commit((), |()| panic!("exhausted child must not run"));
            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 1
            })
        );
        assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(&state.pending[0].1.chain, &chain)));
        assert_eq!(chain.delivered.get(), MAX_CHAIN);
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        drop(chain);
        clear().unwrap();
        assert!(!has_pending());
    }

    #[rstest]
    #[case(MAX_CHAIN - 2, false)]
    #[case(MAX_CHAIN - 1, true)]
    fn retained_continuation_preserves_root_across_empty_queue(
        #[case] delivered: usize,
        #[case] exhausted: bool,
    ) {
        clear().unwrap();
        let continuation = Rc::new(RefCell::new(None));
        let root = reserve(0).unwrap();
        root.slot.chain.delivered.set(delivered);
        root.commit(continuation.clone(), |continuation| {
            *continuation.borrow_mut() = Some(retain(17).unwrap());
            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert!(has_pending());
        assert_eq!(clear(), Err(DispatchError::Active));
        reserve(0).unwrap().commit((), |()| true);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        let mut continuation = continuation.borrow_mut().take().unwrap();
        let chain = Rc::downgrade(&continuation.chain);
        continuation.grow(29).unwrap();
        assert_eq!(continuation.chain.delivered.get(), delivered + 1);
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
            46 + CHAIN_BYTES
        );
        let received = Rc::new(RefCell::new(Vec::new()));
        continuation.with_chain(|| {
            reserve(0).unwrap().commit((received.clone(), 23), record);
        });

        drop(continuation);
        assert_eq!(chain.strong_count(), 1);
        let result = drain(1);

        let expected = if exhausted {
            Err(DispatchError::Runaway)
        } else {
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1,
            })
        };

        assert_eq!(result, expected);
        assert_eq!(
            *received.borrow(),
            if exhausted { vec![] } else { vec![23] }
        );
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert!(!has_pending());
    }

    #[rstest]
    fn cancelled_and_busy_slots_do_not_charge_chain() {
        clear().unwrap();
        let cancelled = reserve::<()>(0).unwrap();
        let cancelled_chain = Rc::downgrade(&cancelled.slot.chain);
        cancelled.slot.chain.delivered.set(MAX_CHAIN);
        drop(cancelled);
        let busy = Rc::new(Cell::new(true));
        let admission = reserve(0).unwrap();
        let chain = admission.slot.chain.clone();
        admission.commit(busy.clone(), |busy| !busy.get());
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                status: DrainStatus::Busy,
                delivered: 0
            })
        );
        assert_eq!(cancelled_chain.strong_count(), 0);
        assert_eq!(chain.delivered.get(), 0);
        busy.set(false);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(chain.delivered.get(), 1);
        drop(chain);
        clear().unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn retained_scope_restores_parent_and_releases_after_failure(#[case] unwind: bool) {
        clear().unwrap();
        let retained = retain(17).unwrap();
        let chain = Rc::downgrade(&retained.chain);
        let accounting = retained.accounting.clone();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _publication = PublicationScope::enter();
            let parent = retain(0).unwrap().chain.clone();
            retained.with_chain(|| {
                assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &retained.chain
                )));
                reserve(0).unwrap().commit((), |()| true);
            });

            assert!(
                DISPATCH.with_borrow(|state| Rc::ptr_eq(state.current.as_ref().unwrap(), &parent))
            );
            retained.with_chain(|| {
                assert!(!unwind, "retained continuation failed");
                record_failure(DispatchError::InvalidDestination);
            });
        }));

        assert_eq!(result.is_err(), unwind);
        assert_eq!(
            failure(),
            Some(if unwind {
                DispatchError::PublicationUnwound
            } else {
                DispatchError::InvalidDestination
            })
        );
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(clear(), Err(DispatchError::Active));
        drop(retained);
        assert_eq!(chain.strong_count(), 1);
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    fn invocation_work_keeps_callback_root() {
        clear().unwrap();
        let root = reserve(0).unwrap();
        let chain = root.slot.chain.clone();
        root.slot.chain.delivered.set(MAX_CHAIN - 1);
        root.commit((), |()| {
            super::super::invocation::run(
                |batch| batch.reserve(0).unwrap().commit(()),
                |()| {
                    reserve(0)
                        .unwrap()
                        .commit((), |()| panic!("invocation must inherit exhausted root"));
                },
            );

            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 1
            })
        );
        assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(&state.pending[0].1.chain, &chain)));
        assert_eq!(chain.delivered.get(), MAX_CHAIN);
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        drop(chain);
        clear().unwrap();
    }

    #[rstest]
    fn invocation_restores_root_after_preparation_scope_ends() {
        clear().unwrap();
        super::super::invocation::run(
            |batch| {
                let _publication = PublicationScope::enter();
                batch.reserve(0).unwrap().commit(());
                DISPATCH
                    .with_borrow(|state| state.current.as_ref().unwrap().delivered.set(MAX_CHAIN));
            },
            |()| {
                reserve(0)
                    .unwrap()
                    .commit((), |()| panic!("invocation must restore preparation root"));
            },
        );

        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        clear().unwrap();
        assert!(!has_pending());
    }

    #[rstest]
    fn invocation_batch_keeps_independent_and_continued_roots_separate() {
        clear().unwrap();
        let parent = retain(17).unwrap();
        parent.chain.delivered.set(37);
        let ingress = ChainContext::independent();
        let mut roots = Vec::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        parent.with_chain(|| {
            super::super::invocation::run(
                |batch| {
                    for value in [11, 23, 31, 47] {
                        if value == 23 || value == 47 {
                            ingress.with_chain(|| batch.reserve(0).unwrap().commit(value));
                        } else {
                            batch.reserve(0).unwrap().commit(value);
                        }
                    }
                },
                |value| {
                    roots.push(ChainContext::capture());
                    reserve(0)
                        .unwrap()
                        .commit((received.clone(), value), record);
                },
            );

            assert!(
                DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &parent.chain,
                ))
            );
        });

        assert_eq!(roots.len(), 4);
        let chains = roots
            .iter()
            .map(|root| root.chain.as_ref().unwrap())
            .collect::<Vec<_>>();
        assert!(Rc::ptr_eq(chains[0], &parent.chain));
        assert!(Rc::ptr_eq(chains[2], &parent.chain));
        assert!(!Rc::ptr_eq(chains[1], &parent.chain));
        assert!(!Rc::ptr_eq(chains[3], &parent.chain));
        assert!(!Rc::ptr_eq(chains[1], chains[3]));
        assert_eq!(
            chains
                .iter()
                .map(|chain| chain.delivered.get())
                .collect::<Vec<_>>(),
            [37, 0, 37, 0]
        );
        assert_eq!(
            drain(4),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 4
            })
        );
        assert_eq!(*received.borrow(), [11, 23, 31, 47]);
        assert_eq!(
            chains
                .iter()
                .map(|chain| chain.delivered.get())
                .collect::<Vec<_>>(),
            [39, 1, 39, 1]
        );
        drop(chains);
        drop(roots);
        drop(parent);
        assert!(!has_pending());
        clear().unwrap();
    }

    #[rstest]
    fn uninvoked_capture_destruction_preserves_root() {
        struct Capture(bool);

        impl Drop for Capture {
            fn drop(&mut self) {
                if self.0 {
                    reserve(0)
                        .unwrap()
                        .commit((), |()| panic!("exhausted cleanup must not run"));
                }
            }
        }

        clear().unwrap();

        let result = std::panic::catch_unwind(|| {
            super::super::invocation::run(
                |batch| {
                    let _publication = PublicationScope::enter();
                    batch.reserve(0).unwrap().commit(Capture(false));
                    batch.reserve(0).unwrap().commit(Capture(true));
                    DISPATCH.with_borrow(|state| {
                        state.current.as_ref().unwrap().delivered.set(MAX_CHAIN);
                    });
                },
                |_| panic!("invocation failed"),
            );
        });

        assert!(result.is_err());
        assert_eq!(failure(), None);
        assert_eq!(DISPATCH.with_borrow(|state| state.pending.len()), 1);
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        clear().unwrap();
        assert!(!has_pending());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn capture_destruction_preserves_root_until_cleanup(#[case] unwind: bool) {
        struct Capture(std::rc::Weak<Chain>);

        impl Drop for Capture {
            fn drop(&mut self) {
                let chain = self.0.upgrade().unwrap();
                assert!(
                    DISPATCH
                        .with_borrow(|state| Rc::ptr_eq(state.current.as_ref().unwrap(), &chain))
                );
                reserve(0).unwrap().commit((), |()| true);
            }
        }

        clear().unwrap();
        let admission = reserve(0).unwrap();
        let chain = Rc::downgrade(&admission.slot.chain);
        admission.commit((Capture(chain.clone()), unwind), |value| {
            assert!(!value.1, "callback failed");
            true
        });

        let result = std::panic::catch_unwind(|| drain(1));
        assert_eq!(result.is_err(), unwind);
        assert_eq!(failure(), unwind.then_some(DispatchError::DeliveryUnwound));
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(chain.strong_count(), if unwind { 2 } else { 1 });
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert!(!has_pending());
    }

    #[rstest]
    fn cancelled_slots_consume_the_drain_budget() {
        clear().unwrap();
        drop(reserve::<()>(0).unwrap());
        reserve(0).unwrap().commit((), |()| true);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::BudgetExhausted,
                delivered: 0
            })
        );
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
    }

    #[rstest]
    fn byte_boundary_includes_inflight_and_variable_payload_storage() {
        clear().unwrap();
        let storage = retain(MAX_KNOWN_BYTES - CHAIN_BYTES).unwrap();
        drop(PublicationScope::enter());
        assert_eq!(failure(), None);
        assert!(reserve::<Vec<u8>>(0).is_none());
        assert_eq!(failure(), Some(DispatchError::Overflow));
        drop(storage);
        clear().unwrap();
        let payload = Vec::<u8>::with_capacity(137);
        let capacity = payload.capacity();
        reserve(capacity).unwrap().commit(payload, |payload| {
            assert!(
                DISPATCH.with_borrow(|state| state.accounting.bytes.get())
                    >= payload.capacity() + size_of::<Delivery<Vec<u8>>>()
            );
            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
            0
        );
    }

    #[rstest]
    fn sequence_exhaustion_preserves_previously_reserved_work() {
        clear().unwrap();
        let _scope = PublicationScope::enter();
        DISPATCH.with_borrow_mut(|state| state.sequence = u64::MAX - 1);
        reserve(0).unwrap().commit((), |()| true);
        assert!(reserve::<()>(0).is_none());
        assert_eq!(failure(), Some(DispatchError::SequenceExhausted));
        assert_eq!(DISPATCH.with_borrow(|state| state.pending.len()), 1);
        drop(_scope);
        clear().unwrap();
    }

    #[rstest]
    #[case(MAX_CHAIN - 2, false)]
    #[case(MAX_CHAIN - 1, true)]
    fn data_command_preserves_callback_root_across_empty_queue(
        #[case] delivered: usize,
        #[case] exhausted: bool,
    ) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let values = received.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                assert_eq!(command, data_command(1));
                reserve(0).unwrap().commit((values.clone(), 23), record);
            }),
        );

        let root = reserve(0).unwrap();
        root.slot.chain.delivered.set(delivered);
        let chain = Rc::downgrade(&root.slot.chain);
        let accounting = root.slot.accounting.clone();
        root.commit((), |()| {
            SyncDataCommandSender.execute(data_command(1));
            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(accounting.contexts.get(), 1);
        assert_eq!(accounting.bytes.get(), CHAIN_BYTES);
        assert!(has_pending());
        assert_eq!(clear(), Err(DispatchError::Active));
        reserve(0).unwrap().commit((), |()| true);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        drain_data_cmd_queue();
        assert_eq!(accounting.contexts.get(), 0);
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(chain.upgrade().unwrap().delivered.get(), delivered + 1);
        assert_eq!(
            drain(1),
            if exhausted {
                Err(DispatchError::Runaway)
            } else {
                Ok(DrainResult {
                    status: DrainStatus::Empty,
                    delivered: 1,
                })
            },
        );
        assert_eq!(
            *received.borrow(),
            if exhausted { vec![] } else { vec![23] }
        );
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    fn independent_data_command_blocks_dispatcher_clear() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                assert_eq!(clear(), Err(DispatchError::Active));
                observed.borrow_mut().push(command);
            }),
        );

        SyncDataCommandSender.execute(data_command(1));
        drain_data_cmd_queue();

        assert_eq!(*received.borrow(), vec![data_command(1)]);
        assert!(data_cmd_queue_is_empty());
        assert_eq!(clear(), Ok(()));
        assert_eq!(failure(), None);
    }

    #[rstest]
    fn independent_data_commands_restore_enclosing_root() {
        clear().unwrap();
        SyncDataCommandSender.execute(data_command(1));
        SyncDataCommandSender.execute(data_command(2));
        assert!(!has_pending());
        assert_eq!(
            DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
            0
        );
        let roots = Rc::new(RefCell::new(Vec::new()));
        let observed = roots.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                let callback = reserve(0).unwrap();
                if command == data_command(1) {
                    callback.slot.chain.delivered.set(MAX_CHAIN - 1);
                } else {
                    assert_eq!(command, data_command(2));
                }

                observed.borrow_mut().push(callback.slot.chain.clone());
                callback.commit((), |()| true);
            }),
        );

        let parent = retain(17).unwrap();
        parent.with_chain(|| {
            drain_data_cmd_queue();
            assert!(
                DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &parent.chain
                ))
            );
        });

        assert_eq!(
            drain(2),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 2
            })
        );
        let roots = roots.borrow();
        assert_eq!(roots.len(), 2);
        assert!(!Rc::ptr_eq(&roots[0], &roots[1]));
        assert!(!Rc::ptr_eq(&roots[0], &parent.chain));
        assert!(!Rc::ptr_eq(&roots[1], &parent.chain));
        assert_eq!(roots[0].delivered.get(), MAX_CHAIN);
        assert_eq!(roots[1].delivered.get(), 1);
        assert_eq!(parent.chain.delivered.get(), 0);
        assert_eq!(failure(), None);
    }

    #[rstest]
    fn data_command_batch_preserves_order_and_nested_root() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        let roots = Rc::new(RefCell::new(Vec::new()));
        let chains = roots.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                if command == data_command(1) {
                    // The nested command creates the root before any callback is admitted
                    SyncDataCommandSender.execute(data_command(3));
                }

                {
                    let _publication = PublicationScope::enter();
                    reserve(0).unwrap().commit((), |()| true);
                }

                chains
                    .borrow_mut()
                    .push(DISPATCH.with_borrow(|state| state.current.clone().unwrap()));
                observed.borrow_mut().push(command);
            }),
        );

        SyncDataCommandSender.execute(data_command(1));
        SyncDataCommandSender.execute(data_command(2));
        drain_data_cmd_queue();
        assert_eq!(*received.borrow(), vec![data_command(1), data_command(2)]);
        assert!(!data_cmd_queue_is_empty());
        drain_data_cmd_queue();
        assert_eq!(
            *received.borrow(),
            vec![data_command(1), data_command(2), data_command(3)]
        );
        assert!(data_cmd_queue_is_empty());
        assert_eq!(
            drain(3),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 3
            })
        );
        let roots = roots.borrow();
        assert_eq!(roots.len(), 3);
        assert!(Rc::ptr_eq(&roots[0], &roots[2]));
        assert!(!Rc::ptr_eq(&roots[0], &roots[1]));
        assert_eq!(roots[0].delivered.get(), 2);
        assert_eq!(roots[1].delivered.get(), 1);
        assert_eq!(failure(), None);
    }

    #[rstest]
    #[case(1)]
    #[case(2)]
    #[case(3)]
    fn data_command_unwind_releases_batch_and_preserves_new_commands(#[case] failing: u8) {
        clear().unwrap();
        let mut roots = Vec::new();

        for id in [1, 2, 3] {
            let _publication = PublicationScope::enter();
            SyncDataCommandSender.execute(data_command(id));
            roots
                .push(DISPATCH.with_borrow(|state| Rc::downgrade(state.current.as_ref().unwrap())));
        }

        let failed = usize::from(failing - 1);
        roots[failed].upgrade().unwrap().delivered.set(MAX_CHAIN);
        let accounting = DISPATCH.with_borrow(|state| state.accounting.clone());
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command: DataCommand| {
                observed.borrow_mut().push(command.clone());
                if command == data_command(failing) {
                    SyncDataCommandSender.execute(data_command(4));
                    panic!("command handler failed");
                }

                if command == data_command(4) {
                    reserve(0)
                        .unwrap()
                        .commit((), |()| panic!("exhausted callback must not run"));
                }
            }),
        );

        let parent = retain(17).unwrap();
        parent.with_chain(|| {
            let result = std::panic::catch_unwind(drain_data_cmd_queue);
            assert!(result.is_err());
            assert!(
                DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &parent.chain
                ))
            );
        });

        drop(parent);

        let mut expected: Vec<_> = (1..=failing).map(data_command).collect();
        assert_eq!(*received.borrow(), expected);

        for (index, root) in roots.iter().enumerate() {
            assert_eq!(root.strong_count(), usize::from(index == failed));
        }

        assert_eq!(accounting.contexts.get(), 1);
        assert_eq!(accounting.bytes.get(), CHAIN_BYTES);
        assert_eq!(failure(), None);
        assert!(!data_cmd_queue_is_empty());
        assert_eq!(clear(), Err(DispatchError::Active));
        drain_data_cmd_queue();
        expected.push(data_command(4));
        assert_eq!(*received.borrow(), expected);
        assert!(data_cmd_queue_is_empty());
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(drain(1), Err(DispatchError::Runaway));
        clear().unwrap();
        assert!(roots.iter().all(|root| root.strong_count() == 0));
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn data_command_context_does_not_reserve_callback_storage(#[case] admit_callback: bool) {
        clear().unwrap();
        let storage = retain(MAX_KNOWN_BYTES - CHAIN_BYTES).unwrap();
        let accounting = storage.accounting.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                if admit_callback && command == data_command(1) {
                    assert!(reserve::<()>(0).is_none());
                }

                observed.borrow_mut().push(command);
            }),
        );

        storage.with_chain(|| SyncDataCommandSender.execute(data_command(1)));
        SyncDataCommandSender.execute(data_command(2));
        assert_eq!(accounting.contexts.get(), 1);
        assert_eq!(accounting.count.get(), 1);
        assert_eq!(accounting.bytes.get(), MAX_KNOWN_BYTES);
        drain_data_cmd_queue();

        assert_eq!(*received.borrow(), vec![data_command(1), data_command(2)]);
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.count.get(), 1);
        assert_eq!(accounting.bytes.get(), MAX_KNOWN_BYTES);
        assert_eq!(failure(), admit_callback.then_some(DispatchError::Overflow));
        drop(storage);
        assert_eq!(accounting.bytes.get(), 0);
        clear().unwrap();
    }

    #[rstest]
    #[case(CHAIN_BYTES - 1, true)]
    #[case(CHAIN_BYTES, false)]
    #[case(CHAIN_BYTES + 1, false)]
    fn data_command_root_allocation_boundary(#[case] remaining: usize, #[case] overflow: bool) {
        clear().unwrap();
        let storage = retain(MAX_KNOWN_BYTES - CHAIN_BYTES - remaining).unwrap();
        let accounting = storage.accounting.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| observed.borrow_mut().push(command)),
        );
        {
            let _publication = PublicationScope::enter();
            SyncDataCommandSender.execute(data_command(1));
        }

        assert_eq!(failure(), overflow.then_some(DispatchError::Overflow));
        assert_eq!(accounting.contexts.get(), usize::from(!overflow));
        assert_eq!(
            accounting.bytes.get(),
            MAX_KNOWN_BYTES - remaining + if overflow { 0 } else { CHAIN_BYTES }
        );
        drain_data_cmd_queue();
        assert_eq!(*received.borrow(), vec![data_command(1)]);
        assert!(data_cmd_queue_is_empty());
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.bytes.get(), MAX_KNOWN_BYTES - remaining);
        assert_eq!(failure(), overflow.then_some(DispatchError::Overflow));
        drop(storage);
        clear().unwrap();
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn recursive_data_command_drain_restores_root(#[case] unwind: bool) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        let outer = retain(17).unwrap();
        let inner = retain(29).unwrap();
        let outer_chain = outer.chain.clone();
        let inner_chain = inner.chain.clone();
        msgbus::register_data_command_endpoint(
            msgbus::MessagingSwitchboard::data_engine_execute(),
            msgbus::TypedIntoHandler::from(move |command| {
                let id = (1..=3).find(|id| command == data_command(*id)).unwrap();
                observed.borrow_mut().push(id);
                if id == 1 {
                    inner.with_chain(|| SyncDataCommandSender.execute(data_command(3)));
                    let result = std::panic::catch_unwind(drain_data_cmd_queue);
                    assert_eq!(
                        result.err().map(|e| *e.downcast::<&str>().unwrap()),
                        unwind.then_some("nested command failed")
                    );
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &outer_chain
                    )));
                    observed.borrow_mut().push(4);
                } else if id == 3 {
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &inner_chain
                    )));

                    assert!(!unwind, "nested command failed");
                }
            }),
        );

        outer.with_chain(|| SyncDataCommandSender.execute(data_command(1)));
        SyncDataCommandSender.execute(data_command(2));
        drain_data_cmd_queue();

        assert_eq!(*received.borrow(), vec![1, 3, 4, 2]);
        assert!(data_cmd_queue_is_empty());
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(failure(), None);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn queued_data_commands_release_at_thread_exit(#[case] queue_first: bool) {
        struct Observe {
            accounting: Rc<Accounting>,
            root: std::rc::Weak<Chain>,
            result: std::sync::Arc<std::sync::Mutex<Option<(usize, usize, usize)>>>,
        }

        impl Drop for Observe {
            fn drop(&mut self) {
                *self.result.lock().unwrap() = Some((
                    self.accounting.contexts.get(),
                    self.accounting.bytes.get(),
                    self.root.strong_count(),
                ));
            }
        }

        thread_local! {
            static OBSERVE: RefCell<Option<Observe>> = const { RefCell::new(None) };
        }

        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let observed = result.clone();

        std::thread::spawn(move || {
            OBSERVE.with_borrow_mut(|observer| {
                if queue_first {
                    assert!(data_cmd_queue_is_empty());
                }

                let storage = retain(0).unwrap();
                storage.with_chain(|| {
                    SyncDataCommandSender.execute(data_command(1));
                    SyncDataCommandSender.execute(data_command(2));
                });

                assert_eq!(storage.accounting.contexts.get(), 2);
                *observer = Some(Observe {
                    accounting: storage.accounting.clone(),
                    root: Rc::downgrade(&storage.chain),
                    result: observed,
                });
            });
        })
        .join()
        .unwrap();

        assert_eq!(*result.lock().unwrap(), Some((0, 0, 0)));
    }

    proptest! {
        #[rstest]
        fn prop_data_command_ancestry(
            nodes in prop::collection::vec((any::<u8>(), any::<bool>()), 1..24),
        ) {
            std::thread::spawn(move || {
                let parents: Vec<_> = nodes.iter().enumerate()
                    .map(|(index, (parent, _))| usize::from(*parent) % (index + 1))
                    .collect();
                let mut ancestors = Vec::new();
                for (index, parent) in parents.iter().copied().enumerate() {
                    ancestors.push(if parent == index { index } else { ancestors[parent] });
                }
                let commands: Vec<_> = (1..=nodes.len() as u8).map(data_command).collect();
                let received = Rc::new(RefCell::new(vec![None; nodes.len()]));
                let observed = received.clone();
                let inputs = commands.clone();
                let links = parents.clone();
                msgbus::register_data_command_endpoint(
                    msgbus::MessagingSwitchboard::data_engine_execute(),
                    msgbus::TypedIntoHandler::from(move |command| {
                        let index = inputs.iter().position(|input| *input == command).unwrap();
                        let scope = nodes[index].1.then(PublicationScope::enter);
                        for (child, parent) in links.iter().enumerate() {
                            if *parent == index && child != index {
                                SyncDataCommandSender.execute(inputs[child].clone());
                            }
                        }
                        let callback = reserve(0).unwrap();
                        assert!(observed.borrow_mut()[index].replace(callback.slot.chain.clone()).is_none());
                        callback.commit((), |()| true);
                        drop(scope);
                    }),
                );

                for (index, parent) in parents.iter().enumerate() {
                    if *parent == index {
                        SyncDataCommandSender.execute(commands[index].clone());
                    }
                }

                for _ in 0..commands.len() {
                    drain_data_cmd_queue();
                }
                assert!(data_cmd_queue_is_empty());
                assert_eq!(drain(commands.len()).unwrap().delivered, commands.len());
                let roots = received.borrow();
                for (index, root) in roots.iter().enumerate() {
                    let root = root.as_ref().unwrap();
                    for (other, other_root) in roots.iter().enumerate() {
                        assert_eq!(Rc::ptr_eq(root, other_root.as_ref().unwrap()), ancestors[index] == ancestors[other]);
                    }
                    assert_eq!(root.delivered.get(), ancestors.iter().filter(|ancestor| **ancestor == ancestors[index]).count());
                }
                assert_eq!(failure(), None);
                assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                let accounting = DISPATCH.with_borrow(|state| state.accounting.clone());
                drop(roots);
                received.borrow_mut().clear();
                clear().unwrap();
                assert_eq!(accounting.contexts.get(), 0);
                assert_eq!(accounting.count.get(), 0);
                assert_eq!(accounting.bytes.get(), 0);
            }).join().unwrap();
        }

        #[rstest]
        fn prop_data_command_drain_scheduling(
            counts in prop::collection::vec(1usize..5, 1..9),
            budgets in prop::collection::vec(0usize..8, 1..17),
        ) {
            std::thread::spawn(move || {
                let commands: Vec<_> = (1..=counts.len() as u8 * 2).map(data_command).collect();
                let received = Rc::new(RefCell::new(Vec::new()));
                let observed = received.clone();
                let delivered = Rc::new(RefCell::new(Vec::new()));
                let values = delivered.clone();
                let inputs = commands.clone();
                let amounts = counts.clone();
                let roots = Rc::new(RefCell::new(Vec::new()));
                let chains = roots.clone();
                msgbus::register_data_command_endpoint(
                    msgbus::MessagingSwitchboard::data_engine_execute(),
                    msgbus::TypedIntoHandler::from(move |command| {
                        let index = inputs.iter().position(|input| *input == command).unwrap();
                        let root = index % amounts.len();
                        if index < amounts.len() {
                            SyncDataCommandSender.execute(inputs[index + amounts.len()].clone());
                        }

                        for _ in 0..amounts[root] {
                            let admission = reserve(0).unwrap();
                            chains.borrow_mut().push(admission.slot.chain.clone());
                            admission.commit((values.clone(), index as u32), record);
                        }
                        observed.borrow_mut().push(command);
                    }),
                );

                for command in &commands[..counts.len()] {
                    SyncDataCommandSender.execute(command.clone());
                }
                drain_data_cmd_queue();
                assert_eq!(*received.borrow(), commands[..counts.len()]);

                for budget in &budgets {
                    drain(*budget).unwrap();
                }
                drain_data_cmd_queue();

                for budget in &budgets {
                    drain(*budget).unwrap();
                }
                drain(usize::MAX).unwrap();
                let expected: Vec<_> = (0..commands.len())
                    .flat_map(|index| std::iter::repeat_n(index as u32, counts[index % counts.len()]))
                    .collect();
                assert_eq!(*received.borrow(), commands);
                assert_eq!(*delivered.borrow(), expected);
                for (root, index) in roots.borrow().iter().zip(&expected) {
                    assert_eq!(root.delivered.get(), counts[*index as usize % counts.len()] * 2);
                }
                assert!(data_cmd_queue_is_empty());
                assert_eq!(failure(), None);
                assert_eq!(DISPATCH.with_borrow(|state| state.accounting.contexts.get()), 0);
                let accounting = DISPATCH.with_borrow(|state| state.accounting.clone());
                roots.borrow_mut().clear();
                clear().unwrap();
                assert_eq!(accounting.count.get(), 0);
                assert_eq!(accounting.bytes.get(), 0);
            }).join().unwrap();
        }
    }

    fn data_command(id: u8) -> DataCommand {
        DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
            "AUD/USD.SIM".into(),
            Some("SIM".into()),
            None,
            nautilus_core::UUID4::from(format!("00000000-0000-4000-8000-{id:012}").as_str()),
            u64::from(id).into(),
            None,
            None,
        )))
    }

    #[rstest]
    fn captured_context_releases_after_dispatcher_teardown() {
        struct Context(Option<(ChainContext, Rc<Accounting>)>);

        impl Drop for Context {
            fn drop(&mut self) {
                let (context, accounting) = self.0.take().unwrap();
                assert!(DISPATCH.try_with(|_| ()).is_err());
                context.with_chain(|| assert!(ChainContext::capture().chain.is_none()));
                drop(context);
                assert_eq!(accounting.contexts.get(), 0);
                assert_eq!(accounting.bytes.get(), 0);
            }
        }

        thread_local! {
            static CONTEXT: RefCell<Context> = const { RefCell::new(Context(None)) };
        }

        std::thread::spawn(|| {
            CONTEXT.with_borrow_mut(|context| {
                let storage = retain(0).unwrap();
                let captured = storage.with_chain(ChainContext::capture);
                context.0 = Some((captured, storage.accounting.clone()));
            });
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn retained_storage_rejects_growth_after_dispatcher_teardown() {
        struct Retained(Option<RetainedStorage>);

        impl Drop for Retained {
            fn drop(&mut self) {
                let storage = self.0.as_mut().unwrap();
                assert!(DISPATCH.try_with(|_| ()).is_err());
                assert_eq!(storage.grow(29), None);
                assert_eq!(storage.bytes, 17);
            }
        }

        thread_local! {
            static RETAINED: RefCell<Retained> = const { RefCell::new(Retained(None)) };
        }

        std::thread::spawn(|| {
            RETAINED.with_borrow_mut(|retained| retained.0 = Some(retain(17).unwrap()));
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn thread_teardown_rejects_reentry_into_destroyed_dispatcher() {
        struct Teardown(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Teardown {
            fn drop(&mut self) {
                assert!(reserve::<()>(0).is_none());
                assert_eq!(
                    drain(1),
                    Ok(DrainResult {
                        status: DrainStatus::Deferred,
                        delivered: 0
                    })
                );
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = dropped.clone();
        std::thread::spawn(move || {
            reserve(0).unwrap().commit(Teardown(observed), |_| true);
        })
        .join()
        .unwrap();

        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[rstest]
    #[case(MAX_CHAIN - 2, false)]
    #[case(MAX_CHAIN - 1, true)]
    fn trading_command_preserves_callback_budget(
        #[case] delivered: usize,
        #[case] exhausted: bool,
    ) {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let values = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(1));
                capture_trading_cmd(trading_message(2));
            }),
        );

        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(2));
                reserve(0).unwrap().commit((values.clone(), 23), record);
            }),
        );

        let root = reserve(0).unwrap();
        root.slot.chain.delivered.set(delivered);
        let chain = Rc::downgrade(&root.slot.chain);
        let accounting = root.slot.accounting.clone();
        root.commit((), |()| {
            SyncTradingCommandSender.execute(trading_message(1));
            true
        });

        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(accounting.contexts.get(), 1);
        assert_eq!(accounting.bytes.get(), CHAIN_BYTES);
        assert_eq!(clear(), Err(DispatchError::Active));
        reserve(0).unwrap().commit((), |()| true);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );

        drain_trading_cmd_queue();

        assert!(trading_cmd_queue_is_empty());
        assert!(!trading_cmd_is_dispatching());
        assert_eq!(accounting.contexts.get(), 0);
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(chain.upgrade().unwrap().delivered.get(), delivered + 1);
        assert_eq!(
            drain(1),
            if exhausted {
                Err(DispatchError::Runaway)
            } else {
                Ok(DrainResult {
                    status: DrainStatus::Empty,
                    delivered: 1,
                })
            }
        );
        assert_eq!(
            *received.borrow(),
            if exhausted { vec![] } else { vec![23] }
        );
        clear().unwrap();
        assert_eq!(chain.strong_count(), 0);
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    fn trading_children_capture_nested_context_and_preserve_depth_first_order() {
        clear().unwrap();
        let nested = retain(17).unwrap();
        let nested_chain = nested.chain.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let roots = Rc::new(RefCell::new(Vec::new()));

        for endpoint in [
            MessagingSwitchboard::exec_engine_execute(),
            MessagingSwitchboard::risk_engine_execute(),
        ] {
            let observed = received.clone();
            let chains = roots.clone();
            let context = nested.with_chain(ChainContext::capture);
            msgbus::register_trading_command_endpoint(
                endpoint,
                TypedIntoHandler::from(move |command| {
                    let id = command_id(&command);
                    assert_eq!(trading_message(id).endpoint(), endpoint);
                    assert_eq!(clear(), Err(DispatchError::Active));
                    observed.borrow_mut().push(id);
                    if id == 1 {
                        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                        capture_trading_cmd(trading_message(3));
                        context.with_chain(|| capture_trading_cmd(trading_message(4)));
                        capture_trading_cmd(trading_message(5));
                        SyncTradingCommandSender.execute(trading_message(7));
                    } else if id == 3 {
                        capture_trading_cmd(trading_message(6));
                    }

                    let callback = reserve(0).unwrap();
                    chains.borrow_mut().push((id, callback.slot.chain.clone()));
                    callback.commit((), |()| true);
                }),
            );
        }

        SyncTradingCommandSender.execute(trading_message(1));
        SyncTradingCommandSender.execute(trading_message(2));
        nested.with_chain(|| {
            drain_trading_cmd_queue();
            assert!(
                DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &nested.chain
                ))
            );
        });

        assert_eq!(*received.borrow(), [1, 3, 6, 4, 5, 2]);
        assert!(!trading_cmd_queue_is_empty());
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3, 6, 4, 5, 2, 7]);
        assert_eq!(
            drain(7),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 7
            })
        );
        let roots = roots.borrow();
        for (id, root) in roots.iter() {
            let expected = match id {
                2 => 1,
                4 => 1,
                _ => 5,
            };

            assert_eq!(root.delivered.get(), expected);
            assert_eq!(Rc::ptr_eq(root, &nested_chain), *id == 4);

            for (other, other_root) in roots.iter() {
                assert_eq!(
                    Rc::ptr_eq(root, other_root),
                    id == other || (!matches!(id, 2 | 4) && !matches!(other, 2 | 4))
                );
            }
        }

        assert!(trading_cmd_queue_is_empty());
        assert!(!trading_cmd_is_dispatching());
        assert_eq!(failure(), None);
    }

    #[rstest]
    #[case(1)]
    #[case(3)]
    fn trading_command_unwind_releases_children_and_batch(#[case] failing: u8) {
        clear().unwrap();
        let parent = retain(11).unwrap();
        let abandoned = retain(23).unwrap();
        let accounting = parent.accounting.clone();
        let root = Rc::downgrade(&parent.chain);
        let abandoned_root = Rc::downgrade(&abandoned.chain);
        let child_root = Rc::new(RefCell::new(None::<std::rc::Weak<Chain>>));
        parent.with_chain(|| SyncTradingCommandSender.execute(trading_message(1)));
        abandoned.with_chain(|| SyncTradingCommandSender.execute(trading_message(2)));
        let received = Rc::new(RefCell::new(Vec::new()));

        for endpoint in [
            MessagingSwitchboard::exec_engine_execute(),
            MessagingSwitchboard::risk_engine_execute(),
        ] {
            let observed = received.clone();
            let expected_root = root.clone();
            let child_chain = child_root.clone();
            msgbus::register_trading_command_endpoint(
                endpoint,
                TypedIntoHandler::from(move |command| {
                    let id = command_id(&command);
                    assert_eq!(trading_message(id).endpoint(), endpoint);
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &expected_root.upgrade().unwrap()
                    )));
                    observed.borrow_mut().push(id);
                    if id == 1 {
                        capture_trading_cmd(trading_message(3));
                        let _scope = ChainScope::enter(None);
                        let child = retain(0).unwrap();
                        *child_chain.borrow_mut() = Some(Rc::downgrade(&child.chain));
                        child.with_chain(|| capture_trading_cmd(trading_message(4)));
                    }

                    if id == failing {
                        capture_trading_cmd(trading_message(5));
                        SyncTradingCommandSender.execute(trading_message(7));
                        panic!("trading handler failed");
                    }
                }),
            );
        }

        drop(parent);
        drop(abandoned);
        let enclosing = retain(37).unwrap();
        enclosing.with_chain(|| {
            let error = std::panic::catch_unwind(drain_trading_cmd_queue).unwrap_err();
            assert_eq!(*error.downcast::<&str>().unwrap(), "trading handler failed");
            assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                state.current.as_ref().unwrap(),
                &enclosing.chain
            )));
        });

        assert_eq!(
            *received.borrow(),
            if failing == 1 { vec![1] } else { vec![1, 3] }
        );
        assert_eq!(abandoned_root.strong_count(), 0);
        assert_eq!(child_root.borrow().as_ref().unwrap().strong_count(), 0);
        assert_eq!(root.strong_count(), 1);
        assert_eq!(accounting.contexts.get(), 1);
        assert_eq!(accounting.bytes.get(), 37 + 2 * CHAIN_BYTES);
        assert!(!trading_cmd_is_dispatching());
        assert!(!trading_cmd_queue_is_empty());
        drain_trading_cmd_queue();
        assert_eq!(
            *received.borrow(),
            if failing == 1 {
                vec![1, 7]
            } else {
                vec![1, 3, 7]
            }
        );
        assert!(trading_cmd_queue_is_empty());
        assert_eq!(root.strong_count(), 0);
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(failure(), None);
        drop(enclosing);
        clear().unwrap();
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn recursive_trading_drain_restores_context_and_capture_frame(#[case] unwind: bool) {
        clear().unwrap();
        let outer = retain(13).unwrap();
        let inner = retain(29).unwrap();
        let outer_chain = outer.chain.clone();
        let inner_chain = inner.chain.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                let id = command_id(&command);
                observed.borrow_mut().push(id);
                if id == 1 {
                    inner.with_chain(|| SyncTradingCommandSender.execute(trading_message(3)));
                    let result = std::panic::catch_unwind(drain_trading_cmd_queue);
                    assert_eq!(
                        result.err().map(|e| *e.downcast::<&str>().unwrap()),
                        unwind.then_some("inner failed")
                    );
                    assert!(trading_cmd_is_dispatching());
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &outer_chain
                    )));
                    capture_trading_cmd(trading_message(5));
                } else if id == 3 {
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &inner_chain
                    )));
                    assert!(!unwind, "inner failed");
                } else {
                    assert_eq!(id, 5);
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &outer_chain
                    )));
                }
            }),
        );

        outer.with_chain(|| SyncTradingCommandSender.execute(trading_message(1)));
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3, 5]);
        assert!(!trading_cmd_is_dispatching());
        assert!(trading_cmd_queue_is_empty());
        assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
        assert_eq!(failure(), None);
    }

    #[rstest]
    #[case(CHAIN_BYTES - 1, true)]
    #[case(CHAIN_BYTES, false)]
    #[case(CHAIN_BYTES + 1, false)]
    fn trading_command_root_allocation_does_not_reject_children(
        #[case] remaining: usize,
        #[case] overflow: bool,
    ) {
        clear().unwrap();
        let storage = retain(MAX_KNOWN_BYTES - CHAIN_BYTES - remaining).unwrap();
        let accounting = storage.accounting.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                let id = command_id(&command);
                observed.borrow_mut().push(id);
                if id == 1 {
                    capture_trading_cmd(trading_message(3));
                    capture_trading_cmd(trading_message(5));
                }
            }),
        );

        {
            let _publication = PublicationScope::enter();
            SyncTradingCommandSender.execute(trading_message(1));
        }

        assert_eq!(failure(), overflow.then_some(DispatchError::Overflow));
        assert_eq!(accounting.contexts.get(), usize::from(!overflow));
        assert_eq!(accounting.count.get(), 1);
        assert_eq!(
            accounting.bytes.get(),
            MAX_KNOWN_BYTES - remaining + if overflow { 0 } else { CHAIN_BYTES }
        );
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3, 5]);
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.count.get(), 1);
        assert_eq!(accounting.bytes.get(), MAX_KNOWN_BYTES - remaining);
        assert_eq!(failure(), overflow.then_some(DispatchError::Overflow));
        drop(storage);
        clear().unwrap();
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn trading_command_admission_ignores_callback_limits(#[case] limit_count: bool) {
        clear().unwrap();

        let storage = retain(if limit_count {
            0
        } else {
            MAX_KNOWN_BYTES - CHAIN_BYTES
        })
        .unwrap();

        let units = storage.with_chain(|| {
            (1..if limit_count { MAX_PENDING } else { 1 })
                .map(|_| retain(0).unwrap())
                .collect::<Vec<_>>()
        });

        let accounting = storage.accounting.clone();
        let count = accounting.count.get();
        let bytes = accounting.bytes.get();
        let root = Rc::downgrade(&storage.chain);
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                let id = command_id(&command);
                observed.borrow_mut().push(id);
                assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                    state.current.as_ref().unwrap(),
                    &root.upgrade().unwrap()
                )));

                if id == 1 {
                    capture_trading_cmd(trading_message(3));
                    assert!(reserve::<()>(0).is_none());
                    capture_trading_cmd(trading_message(5));
                    SyncTradingCommandSender.execute(trading_message(7));
                }
            }),
        );

        storage.with_chain(|| SyncTradingCommandSender.execute(trading_message(1)));
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3, 5]);
        assert_eq!(accounting.contexts.get(), 1);
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3, 5, 7]);
        assert!(trading_cmd_queue_is_empty());
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.count.get(), count);
        assert_eq!(accounting.bytes.get(), bytes);
        assert_eq!(failure(), Some(DispatchError::Overflow));
        drop(units);
        drop(storage);
        clear().unwrap();
        assert_eq!(accounting.count.get(), 0);
        assert_eq!(accounting.bytes.get(), 0);
    }

    #[rstest]
    fn trading_command_only_ingress_allocates_no_root() {
        clear().unwrap();
        let accounting = DISPATCH.with_borrow(|state| state.accounting.clone());
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(1));
                assert_eq!(clear(), Err(DispatchError::Active));
                assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
            }),
        );

        SyncTradingCommandSender.execute(trading_message(1));
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.bytes.get(), 0);
        drain_trading_cmd_queue();
        assert_eq!(accounting.contexts.get(), 0);
        assert_eq!(accounting.bytes.get(), 0);
        assert_eq!(failure(), None);
        clear().unwrap();
    }

    #[rstest]
    fn trading_and_data_commands_share_originating_root() {
        clear().unwrap();
        let storage = retain(0).unwrap();
        let root = storage.chain.clone();
        let expected = root.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(1));
                assert!(
                    DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &expected
                    ))
                );
                SyncDataCommandSender.execute(data_command(1));
            }),
        );

        let expected = root.clone();
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, data_command(1));
                assert!(
                    DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &expected
                    ))
                );
                SyncTradingCommandSender.execute(trading_message(2));
            }),
        );

        let expected = root.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(2));
                let callback = reserve(0).unwrap();
                assert!(Rc::ptr_eq(&callback.slot.chain, &expected));
                callback.commit((), |()| true);
            }),
        );

        storage.with_chain(|| SyncTradingCommandSender.execute(trading_message(1)));
        drop(storage);
        drain_trading_cmd_queue();
        drain_data_cmd_queue();
        drain_trading_cmd_queue();
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                status: DrainStatus::Empty,
                delivered: 1
            })
        );
        assert_eq!(root.delivered.get(), 1);
        assert_eq!(root.accounting.contexts.get(), 0);
        assert_eq!(root.accounting.bytes.get(), CHAIN_BYTES);
        assert_eq!(failure(), None);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn queued_trading_commands_release_at_thread_exit(#[case] queue_first: bool) {
        struct Observe {
            accounting: Rc<Accounting>,
            root: std::rc::Weak<Chain>,
            result: std::sync::Arc<std::sync::Mutex<Option<(usize, usize, usize)>>>,
        }
        impl Drop for Observe {
            fn drop(&mut self) {
                *self.result.lock().unwrap() = Some((
                    self.accounting.contexts.get(),
                    self.accounting.bytes.get(),
                    self.root.strong_count(),
                ));
            }
        }
        thread_local! {
            static OBSERVE: RefCell<Option<Observe>> = const { RefCell::new(None) };
        }
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let observed = result.clone();

        std::thread::spawn(move || {
            OBSERVE.with_borrow_mut(|observer| {
                if queue_first {
                    assert!(trading_cmd_queue_is_empty());
                }

                let storage = retain(0).unwrap();
                storage.with_chain(|| {
                    SyncTradingCommandSender.execute(trading_message(1));
                    SyncTradingCommandSender.execute(trading_message(2));
                });

                assert_eq!(storage.accounting.contexts.get(), 2);
                *observer = Some(Observe {
                    accounting: storage.accounting.clone(),
                    root: Rc::downgrade(&storage.chain),
                    result: observed,
                });
            });
        })
        .join()
        .unwrap();

        assert_eq!(*result.lock().unwrap(), Some((0, 0, 0)));
    }

    proptest! {
        #[rstest]
        fn prop_trading_command_ancestry_and_order(
            nodes in prop::collection::vec((any::<u8>(), any::<bool>(), any::<bool>(), any::<bool>()), 1..24),
            budgets in prop::collection::vec(0usize..8, 1..12),
            panic_at in prop::option::of(any::<u8>()),
        ) {
            std::thread::spawn(move || {
                let failing = panic_at.map(|index| usize::from(index) % nodes.len());
                let parents: Vec<_> = nodes.iter().enumerate().map(|(i, (parent, _, _, _))| usize::from(*parent) % (i + 1)).collect();
                let mut ancestors = Vec::new();
                for (i, parent) in parents.iter().copied().enumerate() {
                    ancestors.push(if parent == i || nodes[i].3 { i } else { ancestors[parent] });
                }
                let mut pending: VecDeque<_> = parents.iter().enumerate().filter_map(|(i, parent)| (i == *parent).then_some(i)).collect();
                let mut expected = Vec::new();
                let mut drains = Vec::new();
                while !pending.is_empty() {
                    let batch: Vec<_> = pending.drain(..).collect();
                    let mut panicked = false;

                    for index in batch {
                        if !visit_commands(index, &parents, &nodes, &mut expected, &mut pending, failing) {
                            panicked = true;
                            break;
                        }
                    }
                    drains.push((expected.clone(), pending.len(), panicked));
                }
                let received = Rc::new(RefCell::new(Vec::new()));
                let roots = Rc::new(RefCell::new(vec![None; nodes.len()]));

                for endpoint in [MessagingSwitchboard::exec_engine_execute(), MessagingSwitchboard::risk_engine_execute()] {
                    let observed = received.clone();
                    let chains = roots.clone();
                    let links = parents.clone();
                    let inputs = nodes.clone();
                    msgbus::register_trading_command_endpoint(endpoint, TypedIntoHandler::from(move |command| {
                        let id = command_id(&command);
                        assert_eq!(trading_message(id).endpoint(), endpoint);
                        let index = usize::from(id - 1);
                        let scope = inputs[index].2.then(PublicationScope::enter);
                        for (child, parent) in links.iter().enumerate() {
                            if *parent == index && child != index {
                                let _scope = inputs[child].3.then(|| ChainScope::enter(None));
                                let message = trading_message(child as u8 + 1);
                                if inputs[child].1 { capture_trading_cmd(message); } else { SyncTradingCommandSender.execute(message); }
                            }
                        }
                        let callback = reserve(0).unwrap();
                        assert!(chains.borrow_mut()[index].replace(callback.slot.chain.clone()).is_none());
                        observed.borrow_mut().push(index);
                        callback.commit((), |()| true);
                        drop(scope);

                        assert!(Some(index) != failing, "trading handler failed");
                    }));
                }

                for (i, parent) in parents.iter().enumerate() {
                    if i == *parent { SyncTradingCommandSender.execute(trading_message(i as u8 + 1)); }
                }
                assert_eq!(DISPATCH.with_borrow(|state| state.accounting.bytes.get()), 0);
                let enclosing = retain(17).unwrap();
                let accounting = enclosing.accounting.clone();
                let mut delivered = 0;

                for (i, (visited, queued, panicked)) in drains.iter().enumerate() {
                    enclosing.with_chain(|| {
                        let result = std::panic::catch_unwind(drain_trading_cmd_queue);
                        assert_eq!(result.err().map(|e| *e.downcast::<&str>().unwrap()), panicked.then_some("trading handler failed"));
                        assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(state.current.as_ref().unwrap(), &enclosing.chain)));
                    });
                    assert_eq!(*received.borrow(), *visited);
                    assert_eq!(trading_cmd_queue_is_empty(), *queued == 0);
                    assert!(!trading_cmd_is_dispatching());
                    assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                    assert_eq!(accounting.contexts.get(), *queued);
                    let remaining = visited.len() - delivered;
                    let count = remaining.min(budgets[i % budgets.len()]);
                    assert_eq!(drain(budgets[i % budgets.len()]), Ok(DrainResult { status: if remaining > count { DrainStatus::BudgetExhausted } else { DrainStatus::Empty }, delivered: count }));
                    delivered += count;
                    assert_eq!(accounting.count.get(), 1 + visited.len() - delivered);
                }
                assert_eq!(drain(usize::MAX), Ok(DrainResult { status: DrainStatus::Empty, delivered: expected.len() - delivered }));
                let chains = roots.borrow();
                for (i, root) in chains.iter().enumerate() {
                    assert_eq!(root.is_some(), expected.contains(&i));
                    if let Some(root) = root {
                        assert_eq!(root.delivered.get(), expected.iter().filter(|index| ancestors[**index] == ancestors[i]).count());
                        for (j, other) in chains.iter().enumerate() {
                            if let Some(other) = other {
                                assert_eq!(Rc::ptr_eq(root, other), ancestors[i] == ancestors[j]);
                            }
                        }
                    }
                }
                assert_eq!(enclosing.chain.delivered.get(), 0);
                assert_eq!(failure(), None);
                drop(chains);
                roots.borrow_mut().clear();
                drop(enclosing);
                clear().unwrap();
                assert_eq!((accounting.contexts.get(), accounting.count.get(), accounting.bytes.get()), (0, 0, 0));
            }).join().unwrap_or_else(|e| std::panic::resume_unwind(e));
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn public_trading_dispatch_restores_synchronous_capture_frame(#[case] unwind: bool) {
        clear().unwrap();
        let storage = retain(7).unwrap();
        let expected = storage.chain.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |command| {
                let id = command_id(&command);
                observed.borrow_mut().push(id);
                assert!(
                    DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &expected
                    ))
                );

                if id == 1 {
                    let contexts = expected.accounting.contexts.get();
                    let result = std::panic::catch_unwind(|| trading_message(2).dispatch());

                    if unwind {
                        assert_eq!(
                            *result.unwrap_err().downcast::<&str>().unwrap(),
                            "unscoped handler failed"
                        );
                    } else {
                        let children = result.unwrap();
                        assert_eq!(children.len(), 1);
                        assert_eq!(children[0].command(), &trading_command(4));
                        assert_eq!(children[0].endpoint(), trading_message(4).endpoint());
                        drop(children);
                    }

                    assert_eq!(expected.accounting.contexts.get(), contexts);
                    assert!(trading_cmd_is_dispatching());
                    capture_trading_cmd(trading_message(3));
                }
            }),
        );

        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |command| {
                assert_eq!(command, trading_command(2));
                capture_trading_cmd(trading_message(4));
                assert!(!unwind, "unscoped handler failed");
            }),
        );

        storage.with_chain(|| SyncTradingCommandSender.execute(trading_message(1)));
        drain_trading_cmd_queue();
        assert_eq!(*received.borrow(), [1, 3]);
        assert!(trading_cmd_queue_is_empty());
        assert!(!trading_cmd_is_dispatching());
        assert_eq!(storage.accounting.contexts.get(), 0);
        assert_eq!(failure(), None);
    }

    #[rstest]
    fn public_trading_dispatch_children_remain_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TradingCommandMessage>();
        let input = trading_message(1);

        let children = std::thread::spawn(move || {
            msgbus::register_trading_command_endpoint(
                input.endpoint(),
                TypedIntoHandler::from(|command| {
                    assert_eq!(command, trading_command(1));
                    capture_trading_cmd(trading_message(2));
                }),
            );

            let children = input.dispatch();
            assert!(!trading_cmd_is_dispatching());
            assert_eq!(
                DISPATCH.with_borrow(|state| state.accounting.contexts.get()),
                0
            );
            assert_eq!(
                DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
                0
            );
            children
        })
        .join()
        .unwrap();

        assert_eq!(children.len(), 1);
        let child = children.into_iter().next().unwrap();
        assert_eq!(child.command(), &trading_command(2));
        assert_eq!(child.endpoint(), trading_message(2).endpoint());

        std::thread::spawn(move || {
            let received = Rc::new(RefCell::new(Vec::new()));
            let observed = received.clone();
            msgbus::register_trading_command_endpoint(
                child.endpoint(),
                TypedIntoHandler::from(move |command| {
                    observed.borrow_mut().push(command);
                }),
            );

            assert!(child.dispatch().is_empty());
            assert_eq!(*received.borrow(), [trading_command(2)]);
            assert!(!trading_cmd_is_dispatching());
        })
        .join()
        .unwrap();
    }

    fn visit_commands(
        index: usize,
        parents: &[usize],
        nodes: &[(u8, bool, bool, bool)],
        visited: &mut Vec<usize>,
        pending: &mut VecDeque<usize>,
        failing: Option<usize>,
    ) -> bool {
        visited.push(index);
        let children: Vec<_> = parents
            .iter()
            .enumerate()
            .filter_map(|(i, parent)| (*parent == index && i != index).then_some(i))
            .collect();
        pending.extend(children.iter().copied().filter(|child| !nodes[*child].1));

        if failing == Some(index) {
            return false;
        }

        for child in children {
            if nodes[child].1 && !visit_commands(child, parents, nodes, visited, pending, failing) {
                return false;
            }
        }

        true
    }

    fn trading_message(id: u8) -> TradingCommandMessage {
        let endpoint = if id % 2 == 1 {
            MessagingSwitchboard::exec_engine_execute()
        } else {
            MessagingSwitchboard::risk_engine_execute()
        };

        TradingCommandMessage::new(endpoint, trading_command(id))
    }

    fn trading_command(id: u8) -> TradingCommand {
        TradingCommand::QueryAccount(QueryAccount::new(
            "TRADER-001".into(),
            Some("SIM".into()),
            "SIM-002".into(),
            UUID4::from(format!("00000000-0000-4000-8000-{id:012}").as_str()),
            u64::from(id).into(),
            None,
            None,
        ))
    }

    fn command_id(command: &TradingCommand) -> u8 {
        let TradingCommand::QueryAccount(command) = command else {
            panic!("expected account query")
        };

        let id = command.ts_init.as_u64() as u8;
        assert_eq!(
            TradingCommand::QueryAccount(command.clone()),
            trading_command(id)
        );
        id
    }
    #[cfg(feature = "live")]
    mod live_commands {
        use super::*;
        use crate::{
            live::{
                dispatch::DispatchMessage,
                sender::{DispatchSender, EventSender},
            },
            runner::{TimeEventMessage, register_time_event_callback},
            timer::{TimeEvent, TimeEventCallback},
        };

        #[rstest]
        fn independent_trading_leaf_allocates_no_root() {
            clear().unwrap();
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::exec_engine_execute(),
                TypedIntoHandler::from(|command| {
                    assert_eq!(command, trading_command(1));
                    assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                    assert_eq!(
                        DISPATCH.with_borrow(|state| state.accounting.bytes.get()),
                        0
                    );
                }),
            );

            DispatchMessage::new(trading_message(1), thread::current().id())
                .dispatch_trading(|_| {});
            assert!(!has_pending());
            clear().unwrap();
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn channel_command_keeps_budget_through_children(#[case] exhausted: bool) {
            clear().unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let root = reserve(0).unwrap();
            root.slot
                .chain
                .delivered
                .set(MAX_CHAIN - if exhausted { 1 } else { 2 });
            let chain = Rc::downgrade(&root.slot.chain);
            let accounting = root.slot.accounting.clone();
            root.commit(tx, |tx| {
                tx.send(DispatchMessage::new(
                    trading_message(1),
                    thread::current().id(),
                ))
                .unwrap();
                true
            });

            drain(1).unwrap();
            assert_eq!(accounting.contexts.get(), 1);
            assert_eq!(clear(), Err(DispatchError::Active));
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::exec_engine_execute(),
                TypedIntoHandler::from(|command| {
                    assert_eq!(command, trading_command(1));
                    capture_trading_cmd(trading_message(2));
                }),
            );

            let observed = Rc::new(Cell::new(false));
            let value = observed.clone();
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(move |command| {
                    assert_eq!(command, trading_command(2));
                    reserve(0).unwrap().commit(value.clone(), |value| {
                        value.set(true);
                        true
                    });
                }),
            );

            rx.try_recv().unwrap().dispatch_trading(|_| {});
            let result = drain(1);
            assert_eq!(
                result,
                if exhausted {
                    Err(DispatchError::Runaway)
                } else {
                    Ok(DrainResult {
                        status: DrainStatus::Empty,
                        delivered: 1,
                    })
                }
            );
            assert_eq!(observed.get(), !exhausted);
            assert_eq!(accounting.contexts.get(), 0);
            clear().unwrap();
            assert_eq!(chain.strong_count(), 0);
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn foreign_receiver_drop_releases_on_owner(#[case] closed: bool) {
            clear().unwrap();
            let storage = retain(0).unwrap();
            let accounting = storage.accounting.clone();
            let chain = Rc::downgrade(&storage.chain);
            let (tx, rx) = std::sync::mpsc::channel();
            if closed {
                drop(rx);
                storage.with_chain(|| {
                    drop(tx.send(DispatchMessage::new(17u32, thread::current().id())));
                });
            } else {
                storage.with_chain(|| {
                    tx.send(DispatchMessage::new(17u32, thread::current().id()))
                        .unwrap();
                });

                thread::spawn(move || drop(rx)).join().unwrap();
            }

            drop(storage);
            assert!(!has_pending());
            assert_eq!(accounting.contexts.get(), 0);
            assert_eq!(accounting.bytes.get(), 0);
            assert_eq!(chain.strong_count(), 0);
            clear().unwrap();
        }

        #[rstest]
        fn external_sender_does_not_capture_foreign_root() {
            clear().unwrap();
            let owner = thread::current().id();

            let message = thread::spawn(move || {
                let storage = retain(0).unwrap();
                let message = storage.with_chain(|| DispatchMessage::new(23u32, owner));
                assert_eq!(storage.accounting.contexts.get(), 0);
                message
            })
            .join()
            .unwrap();

            let enclosing = retain(0).unwrap();
            enclosing.with_chain(|| {
                message.dispatch(|command| {
                    assert_eq!(command, 23);
                    assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                    let next = reserve::<()>(0).unwrap();
                    assert!(!Rc::ptr_eq(&next.slot.chain, &enclosing.chain));
                });
            });

            drop(enclosing);
            clear().unwrap();
        }

        #[rstest]
        fn foreign_dispatch_panics_and_releases_context() {
            clear().unwrap();
            let storage = retain(0).unwrap();
            let accounting = storage.accounting.clone();
            let message =
                storage.with_chain(|| DispatchMessage::new(31u32, thread::current().id()));
            let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = called.clone();
            let result = thread::spawn(move || {
                message.dispatch(|_| observed.store(true, std::sync::atomic::Ordering::SeqCst));
            })
            .join();
            assert!(result.is_err());
            drop(storage);
            assert!(!has_pending());
            assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
            assert_eq!(accounting.contexts.get(), 0);
            clear().unwrap();
        }

        #[rstest]
        fn data_channel_resumes_root_and_nested_send() {
            clear().unwrap();
            let owner = thread::current().id();
            let root = retain(0).unwrap();
            let chain = root.chain.clone();
            let accounting = root.accounting.clone();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            root.with_chain(|| {
                tx.send(DispatchMessage::new(data_command(3), owner))
                    .unwrap();
            });

            msgbus::register_data_command_endpoint(
                MessagingSwitchboard::data_engine_execute(),
                TypedIntoHandler::from(move |command| {
                    assert!(
                        DISPATCH.with_borrow(|state| Rc::ptr_eq(
                            state.current.as_ref().unwrap(),
                            &chain
                        ))
                    );

                    if command == data_command(3) {
                        tx.send(DispatchMessage::new(data_command(5), owner))
                            .unwrap();
                    } else {
                        assert_eq!(command, data_command(5));
                    }
                }),
            );

            drop(root);

            for remaining in [1, 0] {
                rx.try_recv().unwrap().dispatch(|command| {
                    msgbus::send_data_command(MessagingSwitchboard::data_engine_execute(), command);
                });

                assert_eq!(rx.len(), remaining);
                assert_eq!(accounting.contexts.get(), remaining);
            }

            msgbus::register_data_command_endpoint(
                MessagingSwitchboard::data_engine_execute(),
                TypedIntoHandler::from(|_: DataCommand| {}),
            );
            assert_eq!(accounting.bytes.get(), 0);
            clear().unwrap();
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn payload_drop_resumes_root_outside_registry_borrow(#[case] unwind: bool) {
            struct Payload {
                chain: Rc<Chain>,
                dropped: Rc<Cell<bool>>,
            }
            impl Drop for Payload {
                fn drop(&mut self) {
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &self.chain
                    )));
                    let nested = DispatchMessage::new(17u32, thread::current().id());
                    drop(nested);
                    self.dropped.set(true);
                }
            }
            clear().unwrap();
            let root = retain(0).unwrap();
            let accounting = root.accounting.clone();
            let dropped = Rc::new(Cell::new(false));

            let message = root.with_chain(|| {
                DispatchMessage::new(
                    Payload {
                        chain: root.chain.clone(),
                        dropped: dropped.clone(),
                    },
                    thread::current().id(),
                )
            });

            drop(root);

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if unwind {
                    message.dispatch(|_payload| panic!("injected handler panic"));
                } else {
                    drop(message);
                }
            }));

            assert_eq!(result.is_err(), unwind);
            assert!(dropped.get());
            assert_eq!(accounting.contexts.get(), 0);
            assert_eq!(accounting.bytes.get(), 0);
            clear().unwrap();
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn message_outlives_owner_thread(#[case] registry_first: bool) {
            let message = thread::spawn(move || {
                if registry_first {
                    collect_command_contexts();
                }

                let root = retain(0).unwrap();
                root.with_chain(|| DispatchMessage::new(29u32, thread::current().id()))
            })
            .join()
            .unwrap();

            drop(message);
        }

        #[rstest]
        fn thread_teardown_can_capture_after_command_registry_drops() {
            struct Retained(Option<RetainedStorage>);
            impl Drop for Retained {
                fn drop(&mut self) {
                    self.0.as_ref().unwrap().with_chain(|| {
                        drop(DispatchMessage::new(41u32, thread::current().id()));
                    });
                }
            }
            thread_local! {
                static RETAINED: RefCell<Retained> = const { RefCell::new(Retained(None)) };
            }

            thread::spawn(|| {
                let storage = retain(0).unwrap();
                RETAINED.with_borrow_mut(|retained| retained.0 = Some(storage));
                RETAINED.with_borrow(|retained| {
                    retained
                        .0
                        .as_ref()
                        .unwrap()
                        .with_chain(|| drop(DispatchMessage::new(43u32, thread::current().id())));
                });
            })
            .join()
            .unwrap();
        }

        #[rstest]
        fn time_event_dispatch_preserves_root_and_budget() {
            clear().unwrap();
            let root = retain(0).unwrap();
            let chain = root.chain.clone();
            chain.delivered.set(37);
            let expected = chain.clone();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let sender = DispatchSender::new(tx);
            let event = TimeEvent::new("rooted-time".into(), UUID4::new(), 17.into(), 19.into());
            let expected_event = event.clone();

            let callback = TimeEventCallback::RustLocal(Rc::new(move |received| {
                assert_eq!(received, expected_event);
                DISPATCH.with_borrow(|state| {
                    assert!(Rc::ptr_eq(state.current.as_ref().unwrap(), &expected));
                    assert_eq!(expected.delivered.get(), 37);
                });
            }));

            root.with_chain(|| sender.send(TimeEventMessage::new(event, callback)).unwrap());
            let enclosing = retain(0).unwrap();
            enclosing.with_chain(|| {
                assert!(rx.try_recv().unwrap().dispatch(TimeEventMessage::dispatch));
                DISPATCH.with_borrow(|state| {
                    assert!(Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &enclosing.chain
                    ));
                });
            });

            assert_eq!(chain.delivered.get(), 37);
            assert_eq!(root.accounting.contexts.get(), 0);
            drop(enclosing);
            drop(root);
            drop(chain);
            clear().unwrap();
        }

        #[rstest]
        fn repeated_time_events_start_independent_roots() {
            clear().unwrap();
            let enclosing = retain(0).unwrap();
            enclosing.chain.delivered.set(MAX_CHAIN);
            let roots = Rc::new(RefCell::new(Vec::new()));
            let observed = roots.clone();
            let received = Rc::new(RefCell::new(Vec::new()));
            let delivered = received.clone();

            let callback = TimeEventCallback::RustLocal(Rc::new(move |event| {
                let storage = retain(0).unwrap();
                observed.borrow_mut().push(storage.chain.clone());
                reserve(0)
                    .unwrap()
                    .commit((delivered.clone(), event), |(received, event)| {
                        received.borrow_mut().push(event.clone());
                        true
                    });
            }));

            let token = enclosing.with_chain(|| register_time_event_callback(callback));
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let sender = DispatchSender::new(tx);
            let mut expected = Vec::new();

            for timestamp in [17, 23] {
                let event = TimeEvent::new(
                    "repeated-time".into(),
                    UUID4::new(),
                    timestamp.into(),
                    29.into(),
                );
                expected.push(event.clone());
                sender
                    .send(TimeEventMessage::registered(
                        event,
                        token.acquire().unwrap(),
                    ))
                    .unwrap();
                enclosing.with_chain(|| {
                    let message = rx.try_recv().unwrap();
                    assert!(!message.is_rooted());
                    assert!(message.dispatch(TimeEventMessage::dispatch));
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &enclosing.chain,
                    )));
                });

                assert_eq!(
                    drain(1),
                    Ok(DrainResult {
                        status: DrainStatus::Empty,
                        delivered: 1
                    })
                );
            }

            token.close();

            let captured = roots.borrow();
            assert_eq!(captured.len(), 2);
            assert!(!Rc::ptr_eq(&captured[0], &captured[1]));

            for root in captured.iter() {
                assert!(!Rc::ptr_eq(root, &enclosing.chain));
                assert_eq!(root.delivered.get(), 1);
            }

            assert_eq!(*received.borrow(), expected);
            assert_eq!(enclosing.chain.delivered.get(), MAX_CHAIN);
            assert!(rx.is_empty());
            drop(captured);
            roots.borrow_mut().clear();
            drop(enclosing);
            assert!(!has_pending());
            clear().unwrap();
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn event_sender_keeps_command_budget(#[case] exhausted: bool) {
            clear().unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let sender = EventSender::new(tx);
            let root = reserve(0).unwrap();
            root.slot
                .chain
                .delivered
                .set(MAX_CHAIN - if exhausted { 1 } else { 2 });
            let chain = Rc::downgrade(&root.slot.chain);
            let accounting = root.slot.accounting.clone();
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::exec_engine_execute(),
                TypedIntoHandler::from(move |command| {
                    assert_eq!(command, trading_command(1));
                    sender.send(17u32).unwrap();
                }),
            );

            root.commit((), |()| {
                SyncTradingCommandSender.execute(trading_message(1));
                true
            });

            drain(1).unwrap();
            drain_trading_cmd_queue();
            assert_eq!(clear(), Err(DispatchError::Active));

            let observed = Rc::new(Cell::new(0));
            let value = observed.clone();
            rx.try_recv().unwrap().dispatch(|event| {
                reserve(0)
                    .unwrap()
                    .commit((value, event), |(value, event)| {
                        value.set(*event);
                        true
                    });
            });

            let result = drain(1);

            assert_eq!(observed.get(), if exhausted { 0 } else { 17 });
            assert_eq!(
                result,
                if exhausted {
                    Err(DispatchError::Runaway)
                } else {
                    Ok(DrainResult {
                        status: DrainStatus::Empty,
                        delivered: 1,
                    })
                }
            );
            assert_eq!(accounting.contexts.get(), 0);
            clear().unwrap();
            assert_eq!(chain.strong_count(), 0);
        }

        #[rstest]
        fn event_sender_foreign_ingress_is_independent() {
            clear().unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let sender = EventSender::new(tx);

            thread::spawn(move || {
                let foreign = retain(0).unwrap();
                foreign.with_chain(|| sender.send(23u32).unwrap());
                drop(foreign);
                clear().unwrap();
            })
            .join()
            .unwrap();

            let enclosing = retain(0).unwrap();
            enclosing.with_chain(|| {
                rx.try_recv().unwrap().dispatch(|event| {
                    assert_eq!(event, 23);
                    assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                });
            });

            drop(enclosing);
            clear().unwrap();
        }

        #[rstest]
        #[case(false)]
        #[case(true)]
        fn event_sender_releases_abandoned_roots(#[case] closed: bool) {
            clear().unwrap();
            let root = retain(0).unwrap();
            let accounting = root.accounting.clone();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let sender = EventSender::new(tx);

            if closed {
                drop(rx);
                let error = root.with_chain(|| sender.send(29u32).unwrap_err());
                assert!(error.0.is_rooted());
                assert_eq!(accounting.contexts.get(), 1);
                error.0.dispatch(|event| {
                    assert_eq!(event, 29);
                    assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(
                        state.current.as_ref().unwrap(),
                        &root.chain
                    )));
                });
            } else {
                root.with_chain(|| sender.send(29u32).unwrap());
                assert_eq!(accounting.contexts.get(), 1);

                thread::spawn(move || drop(rx)).join().unwrap();
            }

            drop(root);
            assert!(!has_pending());
            assert_eq!(accounting.contexts.get(), 0);
            assert_eq!(accounting.bytes.get(), 0);
            clear().unwrap();
        }

        proptest::proptest! {
            #[rstest]
            fn prop_mixed_channel_hops_preserve_root(kinds in proptest::collection::vec(proptest::bool::ANY, 1..32)) {
                clear().unwrap();
                let root = retain(0).unwrap();
                let chain = Rc::downgrade(&root.chain);
                let accounting = root.accounting.clone();
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                let sender = EventSender::new(event_tx);
                let owner = thread::current().id();
                let mut message = root.with_chain(|| DispatchMessage::new(0usize, owner));
                drop(root);

                for (index, event) in kinds.iter().enumerate() {
                    message = message.dispatch(|value| {
                        assert_eq!(value, index);
                        assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(state.current.as_ref().unwrap(), &chain.upgrade().unwrap())));
                        if *event {
                            sender.send(index + 1).unwrap();
                            event_rx.try_recv().unwrap()
                        } else {
                            DispatchMessage::new(index + 1, owner)
                        }
                    });
                    assert!(DISPATCH.with_borrow(|state| state.current.is_none()));
                    assert_eq!(accounting.contexts.get(), 1);
                    assert_eq!(clear(), Err(DispatchError::Active));
                }
                assert_eq!(message.dispatch(|value| value), kinds.len());
                assert_eq!(chain.strong_count(), 0);
                assert_eq!(accounting.contexts.get(), 0);
                assert_eq!(accounting.bytes.get(), 0);
                clear().unwrap();
            }
        }

        proptest::proptest! {
            #[rstest]
            fn channel_contexts_restore_and_release(actions in proptest::collection::vec((0u8..4, proptest::bool::ANY), 0..64)) {
                clear().unwrap();
                let owner = thread::current().id();
                let roots = [retain(0).unwrap(), retain(0).unwrap()];
                let accounting = roots[0].accounting.clone();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

                for (i, &(root, _)) in actions.iter().enumerate() {
                    let message = if root < 2 {
                        roots[usize::from(root)].with_chain(|| DispatchMessage::new(i, owner))
                    } else { DispatchMessage::from(i) };
                    tx.send(message).unwrap();
                }

                for (i, &(root, unwind)) in actions.iter().enumerate() {
                    let message = rx.try_recv().unwrap();
                    roots[1].with_chain(|| {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| message.dispatch(|value| {
                            assert_eq!(value, i);
                            let current = DISPATCH.with_borrow(|state| state.current.clone());
                            if root < 2 { assert!(Rc::ptr_eq(current.as_ref().unwrap(), &roots[usize::from(root)].chain)); }
                            else { assert!(current.is_none()); }
                            assert!(!unwind, "injected handler panic");
                        })));
                        assert_eq!(result.is_err(), unwind);
                        assert!(DISPATCH.with_borrow(|state| Rc::ptr_eq(state.current.as_ref().unwrap(), &roots[1].chain)));
                    });
                }
                assert_eq!(accounting.contexts.get(), 0);
                drop(roots);
                assert_eq!(accounting.bytes.get(), 0);
                clear().unwrap();
            }
        }
    }
}
