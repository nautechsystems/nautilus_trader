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

//! Wrappers around shared, interior-mutable cell pairs.
//!
//! NautilusTrader engines store many components as `Rc<RefCell<T>>` for shared ownership with
//! interior mutability. Spelling that type at every boundary is verbose and risks accidentally
//! holding a strong reference where a weak one is required, leading to reference cycles.
//!
//! [`SharedCell<T>`] and [`WeakCell<T>`] are zero-cost newtypes that name the intent and forward
//! the common operations (`new`, `borrow`, `borrow_mut`, `with`, `with_mut`, `downgrade`,
//! `upgrade`). They are `#[repr(transparent)]` and share the memory layout of the wrapped `Rc` /
//! `Weak`.
//!
//! ## Choosing between `SharedCell` and `WeakCell`
//!
//! - Use [`SharedCell<T>`] when the holder owns or co-owns the value, like a plain
//!   `Rc<RefCell<T>>`.
//! - Use [`WeakCell<T>`] for back-references that would otherwise form a cycle. The back-pointer
//!   does not keep the value alive; every access must first `upgrade()` to a strong
//!   [`SharedCell`]. This pattern breaks circular ownership: for an `Exchange` that owns an
//!   `ExecutionClient` which references the exchange, the exchange holds a [`SharedCell`] to the
//!   client and the client holds a [`WeakCell`] back to the exchange.

use std::{
    cell::{BorrowError, BorrowMutError, Ref, RefCell, RefMut},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

/// Strong, shared ownership of `T` with interior mutability.
#[repr(transparent)]
#[derive(Debug)]
pub struct SharedCell<T>(Rc<RefCell<T>>);

impl<T> Clone for SharedCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> SharedCell<T> {
    /// Wraps a value inside `Rc<RefCell<..>>`.
    #[inline]
    pub fn new(value: T) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    /// Creates a [`WeakCell`] pointing to the same allocation.
    #[inline]
    #[must_use]
    pub fn downgrade(&self) -> WeakCell<T> {
        WeakCell(Rc::downgrade(&self.0))
    }

    /// Immutable borrow of the inner value.
    #[inline]
    #[must_use]
    pub fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }

    /// Mutable borrow of the inner value.
    #[inline]
    #[must_use]
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }

    /// Attempts to immutably borrow the inner value.
    ///
    /// # Errors
    ///
    /// Returns [`BorrowError`] if the value is currently mutably borrowed.
    #[inline]
    pub fn try_borrow(&self) -> Result<Ref<'_, T>, BorrowError> {
        self.0.try_borrow()
    }

    /// Attempts to mutably borrow the inner value.
    ///
    /// # Errors
    ///
    /// Returns [`BorrowMutError`] if the value is currently borrowed
    /// (mutably or immutably).
    #[inline]
    pub fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, BorrowMutError> {
        self.0.try_borrow_mut()
    }

    /// Number of active strong references.
    #[inline]
    #[must_use]
    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    /// Number of active weak references.
    #[inline]
    #[must_use]
    pub fn weak_count(&self) -> usize {
        Rc::weak_count(&self.0)
    }

    /// Returns the raw pointer to the underlying cell, useful for identity diagnostics.
    #[inline]
    #[must_use]
    pub fn as_ptr(&self) -> *const RefCell<T> {
        Rc::as_ptr(&self.0)
    }

    /// Runs `f` against an immutable borrow of the inner value, returning its result.
    ///
    /// The borrow is dropped at the end of the closure, so callers can safely follow
    /// the call with operations that re-enter the same cell (e.g. event dispatch).
    #[inline]
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0.borrow())
    }

    /// Runs `f` against a mutable borrow of the inner value, returning its result.
    ///
    /// The borrow is dropped at the end of the closure, so callers can safely follow
    /// the call with operations that re-enter the same cell.
    #[inline]
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}

impl<T> PartialEq for SharedCell<T> {
    /// Identity equality: two handles compare equal when they point to the same cell.
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for SharedCell<T> {}

impl<T> Hash for SharedCell<T> {
    /// Hashes the cell's pointer address, consistent with the identity-based [`PartialEq`] impl.
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl<T> From<Rc<RefCell<T>>> for SharedCell<T> {
    fn from(inner: Rc<RefCell<T>>) -> Self {
        Self(inner)
    }
}

impl<T> From<SharedCell<T>> for Rc<RefCell<T>> {
    fn from(shared: SharedCell<T>) -> Self {
        shared.0
    }
}

impl<T> std::ops::Deref for SharedCell<T> {
    type Target = Rc<RefCell<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Weak counterpart to [`SharedCell`].
#[repr(transparent)]
#[derive(Debug)]
pub struct WeakCell<T>(Weak<RefCell<T>>);

impl<T> Clone for WeakCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> WeakCell<T> {
    /// Attempts to upgrade the weak reference to a strong [`SharedCell`].
    #[inline]
    pub fn upgrade(&self) -> Option<SharedCell<T>> {
        self.0.upgrade().map(SharedCell)
    }

    /// Returns `true` if the pointed-to value has been dropped.
    #[inline]
    #[must_use]
    pub fn is_dropped(&self) -> bool {
        self.0.strong_count() == 0
    }
}

impl<T> From<Weak<RefCell<T>>> for WeakCell<T> {
    fn from(inner: Weak<RefCell<T>>) -> Self {
        Self(inner)
    }
}

impl<T> From<WeakCell<T>> for Weak<RefCell<T>> {
    fn from(cell: WeakCell<T>) -> Self {
        cell.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_shared_cell_new_and_borrow() {
        let cell = SharedCell::new(42);
        assert_eq!(*cell.borrow(), 42);
    }

    #[rstest]
    fn test_shared_cell_borrow_mut() {
        let cell = SharedCell::new(0);
        *cell.borrow_mut() = 99;
        assert_eq!(*cell.borrow(), 99);
    }

    #[rstest]
    fn test_shared_cell_clone_shares_value() {
        let cell = SharedCell::new(10);
        let clone = cell.clone();
        *cell.borrow_mut() = 20;
        assert_eq!(*clone.borrow(), 20);
    }

    #[rstest]
    fn test_shared_cell_strong_weak_counts() {
        let cell = SharedCell::new(1);
        assert_eq!(cell.strong_count(), 1);
        assert_eq!(cell.weak_count(), 0);

        let weak = cell.downgrade();
        assert_eq!(cell.weak_count(), 1);
        assert_eq!(cell.strong_count(), 1);

        let clone = cell.clone();
        assert_eq!(cell.strong_count(), 2);
        drop(clone);
        assert_eq!(cell.strong_count(), 1);
        drop(weak);
        assert_eq!(cell.weak_count(), 0);
    }

    #[rstest]
    fn test_weak_cell_upgrade_succeeds_while_alive() {
        let cell = SharedCell::new(10);
        let weak = cell.downgrade();
        assert!(!weak.is_dropped());

        let upgraded = weak.upgrade();
        assert!(upgraded.is_some());
        assert_eq!(*upgraded.unwrap().borrow(), 10);
    }

    #[rstest]
    fn test_weak_cell_upgrade_fails_after_drop() {
        let weak = {
            let cell = SharedCell::new(10);
            cell.downgrade()
        };
        assert!(weak.is_dropped());
        assert!(weak.upgrade().is_none());
    }

    #[rstest]
    #[expect(clippy::redundant_clone, reason = "Clone is the behavior under test")]
    fn test_weak_cell_clone() {
        let cell = SharedCell::new(5);
        let weak1 = cell.downgrade();
        let weak2 = weak1.clone();
        assert_eq!(cell.weak_count(), 2);
        assert_eq!(*weak2.upgrade().unwrap().borrow(), 5);
    }

    #[rstest]
    fn test_try_borrow_fails_while_mutably_borrowed() {
        let cell = SharedCell::new(0);
        let _guard = cell.borrow_mut();
        assert!(cell.try_borrow().is_err());
    }

    #[rstest]
    fn test_try_borrow_mut_fails_while_borrowed() {
        let cell = SharedCell::new(0);
        let _guard = cell.borrow();
        assert!(cell.try_borrow_mut().is_err());
    }

    #[rstest]
    fn test_from_rc_refcell_roundtrip() {
        let rc = Rc::new(RefCell::new(5));
        let cell = SharedCell::from(rc);
        assert_eq!(*cell.borrow(), 5);

        let back: Rc<RefCell<i32>> = cell.into();
        assert_eq!(*back.borrow(), 5);
    }

    #[rstest]
    fn test_from_weak_refcell_roundtrip() {
        let rc = Rc::new(RefCell::new(7));
        let weak_cell = WeakCell::from(Rc::downgrade(&rc));
        assert_eq!(*weak_cell.upgrade().unwrap().borrow(), 7);

        let back: Weak<RefCell<i32>> = weak_cell.into();
        assert_eq!(*back.upgrade().unwrap().borrow(), 7);
    }

    #[rstest]
    fn test_partial_eq_is_pointer_identity() {
        let a = SharedCell::new(10);
        let b = a.clone();
        let c = SharedCell::new(10);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[rstest]
    #[expect(
        clippy::mutable_key_type,
        reason = "SharedCell hashes by pointer identity, not interior value"
    )]
    fn test_hash_matches_pointer_identity() {
        let a = SharedCell::new(10);
        let b = a.clone();
        let c = SharedCell::new(10);

        let mut set: HashSet<SharedCell<i32>> = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }

    #[rstest]
    fn test_as_ptr_matches_clone() {
        let cell = SharedCell::new(0);
        let cloned = cell.clone();
        assert_eq!(cell.as_ptr(), cloned.as_ptr());
    }

    #[rstest]
    fn test_with_drops_borrow_before_returning() {
        let cell = SharedCell::new(100);
        let value = cell.with(|v| *v);

        // Borrow released; subsequent borrow_mut works without panic.
        *cell.borrow_mut() = 1;

        assert_eq!(value, 100);
        assert_eq!(*cell.borrow(), 1);
    }

    #[rstest]
    fn test_with_mut_drops_borrow_before_returning() {
        let cell = SharedCell::new(0);
        let returned = cell.with_mut(|v| {
            *v = 7;
            *v
        });

        // Borrow released; we can read back through a fresh borrow.
        assert_eq!(returned, 7);
        assert_eq!(*cell.borrow(), 7);
    }
}
