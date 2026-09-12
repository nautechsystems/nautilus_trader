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

//! Inactive ordered callback infrastructure; production routes do not use this module.

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
    previous: Option<u64>,
    active: bool,
    marker: std::marker::PhantomData<Rc<()>>,
}

impl PublicationScope {
    pub(super) fn enter() -> Self {
        let previous = DISPATCH.try_with(|state| {
            let mut state = state.borrow_mut();
            let ordinal = state.sequence();
            state.depth += 1;
            std::mem::replace(&mut state.publication, ordinal)
        });
        Self {
            active: previous.is_ok(),
            previous: previous.unwrap_or(None),
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

            if !state.pending.is_empty() && state.chain >= MAX_CHAIN {
                state.fail(DispatchError::Runaway);
                return Err(DispatchError::Runaway);
            }
            Ok(state.pending.front().map(|(_, slot)| slot.clone()))
        })?;
        let Some(slot) = slot else {
            break;
        };
        let pending = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Reserved);
        match pending {
            SlotState::Reserved => break,
            SlotState::Cancelled => {}
            SlotState::Ready(mut delivery) => {
                if !delivery.run() {
                    *slot.state.borrow_mut() = SlotState::Ready(delivery);
                    break;
                }
                // Callback-local guards end before the owned capture is destroyed
                drop(delivery);
                delivered += 1;
                DISPATCH.with_borrow_mut(|state| state.chain += 1);
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

        if state.pending.is_empty() {
            state.chain = 0;
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

            if !state.fits(bytes, state.pending.capacity()) {
                state.fail(DispatchError::Overflow);
                return None;
            }
            let accounting = state.accounting.clone();
            accounting.count.set(accounting.count.get() + 1);
            accounting.bytes.set(accounting.bytes.get() + bytes);
            Some(RetainedStorage { accounting, bytes })
        })
        .ok()
        .flatten()
}

pub(super) fn record_failure(error: DispatchError) {
    let _ = DISPATCH.try_with(|state| state.borrow_mut().fail(error));
}

pub(super) struct RetainedStorage {
    accounting: Rc<Accounting>,
    bytes: usize,
}

impl RetainedStorage {
    pub(super) fn grow(&mut self, bytes: usize) -> Option<()> {
        if bytes == 0 {
            return Some(());
        }
        let mut extra = retain(bytes)?;
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
            state.accounting.count.get() != 0 || state.error.is_some()
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

            if state.draining
                || state.depth != 0
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
}

#[derive(Default)]
struct Dispatcher {
    pending: VecDeque<((u64, u64), Rc<Slot>)>,
    accounting: Rc<Accounting>,
    sequence: u64,
    publication: Option<u64>,
    depth: usize,
    draining: bool,
    clearing: bool,
    chain: usize,
    error: Option<DispatchError>,
}

impl Dispatcher {
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
    use rstest::rstest;

    use super::*;

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
        assert_eq!(retained, (1, 46));
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
        DISPATCH.with_borrow_mut(|state| state.chain = MAX_CHAIN - 1);
        reserve(0).unwrap().commit((), |()| true);
        reserve(0)
            .unwrap()
            .commit((), |()| panic!("runaway callback must not run"));
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
        let storage = retain(MAX_KNOWN_BYTES).unwrap();
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
