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
};

const MAX_PENDING: usize = 65_536;
const MAX_KNOWN_BYTES: usize = 64 * 1024 * 1024;
const MAX_CHAIN: usize = 1_048_576;
const CHAIN_BYTES: usize = size_of::<Chain>() + 2 * size_of::<usize>();

thread_local! {
    static DISPATCH: RefCell<Dispatcher> = RefCell::new(Dispatcher::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchError {
    Overflow,
    InvalidDestination,
    SequenceExhausted,
    PublicationUnwound,
    DeliveryUnwound,
    Runaway,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DrainResult {
    pub(super) delivered: usize,
    pub(super) pending: bool,
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

pub(super) fn drain(budget: usize) -> Result<DrainResult, DispatchError> {
    let entered = DISPATCH
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if let Some(e) = state.error {
                return Err(e);
            }

            if state.draining || state.clearing || state.depth != 0 || super::access::is_active() {
                return Ok(false);
            }
            state.draining = true;
            Ok(true)
        })
        .unwrap_or(Ok(false))?;
    if !entered {
        return Ok(DrainResult {
            delivered: 0,
            pending: has_pending(),
        });
    }
    let _scope = DrainScope;
    let mut delivered = 0;
    let mut processed = 0;
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
            SlotState::Reserved => break,
            SlotState::Cancelled => {}
            SlotState::Ready(mut delivery) => {
                if slot.chain.delivered.get() >= MAX_CHAIN {
                    *slot.state.borrow_mut() = SlotState::Ready(delivery);
                    record_failure(DispatchError::Runaway);
                    return Err(DispatchError::Runaway);
                }

                if !delivery.run() {
                    *slot.state.borrow_mut() = SlotState::Ready(delivery);
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
            delivered,
            pending: !state.pending.is_empty(),
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
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::{
        messages::data::{DataCommand, SubscribeCommand, SubscribeQuotes},
        msgbus,
        runner::{
            DataCommandSender, SyncDataCommandSender, data_cmd_queue_is_empty, drain_data_cmd_queue,
        },
    };

    fn record(value: &mut (Rc<RefCell<Vec<u32>>>, u32)) -> bool {
        value.0.borrow_mut().push(value.1);
        true
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
            assert_eq!(
                drain(10),
                Ok(DrainResult {
                    delivered: 0,
                    pending: true
                })
            );
        }
        assert_eq!(
            drain(10),
            Ok(DrainResult {
                delivered: 3,
                pending: false
            })
        );
        assert_eq!(*received.borrow(), [11, 12, 21]);
    }

    #[rstest]
    fn reserved_head_blocks_delivery_and_teardown() {
        clear().unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let head = reserve(0).unwrap();
        reserve(0).unwrap().commit((received.clone(), 22), record);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                delivered: 0,
                pending: true
            })
        );
        assert_eq!(clear(), Err(DispatchError::Active));
        head.commit((received.clone(), 11), record);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                delivered: 2,
                pending: false
            })
        );
        assert_eq!(*received.borrow(), [11, 22]);
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
                delivered: 4,
                pending: false
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
                delivered: 0,
                pending: false
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
            assert_eq!(
                drain(1),
                Ok(DrainResult {
                    delivered: 0,
                    pending: true
                })
            );
            !busy.get()
        });
        let received = Rc::new(RefCell::new(Vec::new()));
        reserve(0).unwrap().commit((received.clone(), 22), record);
        assert_eq!(
            drain(2),
            Ok(DrainResult {
                delivered: 0,
                pending: true
            })
        );
        assert!(received.borrow().is_empty());
        busy.set(false);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                delivered: 1,
                pending: true
            })
        );
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                delivered: 1,
                pending: false
            })
        );
        assert_eq!(*received.borrow(), [22]);
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
            assert_eq!(drain(1).unwrap().delivered, 0);
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
                delivered: 1,
                pending: true
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
                delivered: 2,
                pending: false
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
                delivered: 1,
                pending: true
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
                delivered: 1,
                pending: false
            })
        );
        assert!(has_pending());
        assert_eq!(clear(), Err(DispatchError::Active));
        reserve(0).unwrap().commit((), |()| true);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                delivered: 1,
                pending: false
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
                delivered: 1,
                pending: false,
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
                delivered: 0,
                pending: true
            })
        );
        assert_eq!(cancelled_chain.strong_count(), 0);
        assert_eq!(chain.delivered.get(), 0);
        busy.set(false);
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                delivered: 1,
                pending: false
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
                delivered: 1,
                pending: true
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
                delivered: 0,
                pending: true
            })
        );
        assert_eq!(
            drain(1),
            Ok(DrainResult {
                delivered: 1,
                pending: false
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
                delivered: 1,
                pending: false
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
                delivered: 1,
                pending: false
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
                delivered: 1,
                pending: false
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
                    delivered: 1,
                    pending: false,
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
                delivered: 2,
                pending: false
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
                delivered: 3,
                pending: false
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
                        delivered: 0,
                        pending: false
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
}
