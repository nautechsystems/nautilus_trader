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

//! Owned invocation preparation outside callback-local access lifetimes.

#![allow(
    dead_code,
    reason = "runtime activation requires safe native and Python drain boundaries"
)]

use super::dispatch::{self, DispatchError, RetainedStorage};

pub(super) fn run<T>(prepare: impl FnOnce(&mut InvocationBatch<T>), mut invoke: impl FnMut(T)) {
    let mut batch = InvocationBatch {
        pending: Vec::new(),
        storage: None,
    };
    prepare(&mut batch);

    if dispatch::failure().is_some() {
        return;
    }
    let InvocationBatch {
        pending,
        storage: _storage,
    } = batch;

    for retained in pending {
        let Retained { value, _storage } = retained;
        invoke(value);

        if dispatch::failure().is_some() {
            break;
        }
    }
}

pub(super) struct InvocationBatch<T> {
    pending: Vec<Retained<T>>,
    storage: Option<RetainedStorage>,
}

impl<T> InvocationBatch<T> {
    pub(super) fn reserve(&mut self, heap_bytes: usize) -> Option<InvocationAdmission<'_, T>> {
        let storage = dispatch::retain(heap_bytes)?;

        if self.pending.len() == self.pending.capacity() {
            let element = size_of::<Retained<T>>();
            match &mut self.storage {
                Some(storage) => storage.grow(element)?,
                None => self.storage = Some(dispatch::retain(element)?),
            }
            let expected = self.pending.len() + 1;
            if self.pending.try_reserve_exact(1).is_err() {
                dispatch::record_failure(DispatchError::Overflow);
                return None;
            }
            let extra = self
                .pending
                .capacity()
                .saturating_sub(expected)
                .saturating_mul(element);
            self.storage
                .as_mut()
                .expect("capacity is charged")
                .grow(extra)?;
        }
        Some(InvocationAdmission {
            batch: self,
            storage,
        })
    }
}

pub(super) struct InvocationAdmission<'a, T> {
    batch: &'a mut InvocationBatch<T>,
    storage: RetainedStorage,
}

impl<T> InvocationAdmission<'_, T> {
    pub(super) fn commit(self, value: T) {
        let Self { batch, storage } = self;
        batch.pending.push(Retained {
            value,
            _storage: storage,
        });
    }
}

struct Retained<T> {
    value: T,
    _storage: RetainedStorage,
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, UnsafeCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use rstest::rstest;

    use super::*;
    use crate::actor::access::AllocationGuard;

    struct Probe {
        allocation: Rc<UnsafeCell<()>>,
        dropped: Rc<Cell<usize>>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let _guard = AllocationGuard::acquire(self.allocation.clone())
                .expect("capture drops outside access");
            self.dropped.set(self.dropped.get() + 1);
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn partial_batch_failure_releases_captures_after_guards(#[case] unwind: bool) {
        dispatch::clear().unwrap();
        let allocation = Rc::new(UnsafeCell::new(()));
        let dropped = Rc::new(Cell::new(0));
        let result = catch_unwind(AssertUnwindSafe(|| {
            run(
                |batch| {
                    let _guard = AllocationGuard::acquire(allocation.clone()).unwrap();
                    batch.reserve(0).unwrap().commit(Probe {
                        allocation: allocation.clone(),
                        dropped: dropped.clone(),
                    });
                    assert!(!unwind, "preparation failed");
                    assert!(batch.reserve(usize::MAX).is_none());
                    assert_eq!(dropped.get(), 0);
                },
                |_| panic!("failed batch must not invoke"),
            );
        }));
        assert_eq!(result.is_err(), unwind);
        assert_eq!(dropped.get(), 1);
        dispatch::clear().unwrap();
    }

    #[rstest]
    fn growing_batch_delivers_all_captures_and_releases_storage() {
        dispatch::clear().unwrap();
        let mut received = Vec::new();
        run(
            |batch| {
                batch.reserve(17).unwrap().commit(11);
                batch.reserve(29).unwrap().commit(23);
            },
            |value| received.push(value),
        );
        assert_eq!(received, [11, 23]);
        assert_eq!(dispatch::failure(), None);
        assert!(!dispatch::has_pending());
        dispatch::clear().unwrap();
    }

    #[rstest]
    fn successful_batch_invokes_after_preparation_access_ends() {
        dispatch::clear().unwrap();
        let allocation = Rc::new(UnsafeCell::new(()));
        let dropped = Rc::new(Cell::new(0));
        run(
            |batch| {
                let _guard = AllocationGuard::acquire(allocation.clone()).unwrap();
                batch.reserve(0).unwrap().commit(Probe {
                    allocation: allocation.clone(),
                    dropped: dropped.clone(),
                });
            },
            drop,
        );
        assert_eq!(dropped.get(), 1);
        dispatch::clear().unwrap();
    }
}
