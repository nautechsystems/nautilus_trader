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

//! Read-only view over the platform cache handed to adapter-facing code.

use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

use super::Cache;

// TODO: Reassess whether CacheView should consolidate with CacheApi once adapter and client
// construction no longer need a cache-handle facade.
/// Read-only view over the platform cache.
///
/// Adapter-facing code receives this type instead of the mutable cache handle so cache writes stay
/// owned by the data and execution engines.
#[derive(Clone, Debug)]
pub struct CacheView {
    inner: Rc<RefCell<Cache>>,
}

impl CacheView {
    /// Creates a new [`CacheView`] from a cache handle.
    #[must_use]
    pub fn new(inner: Rc<RefCell<Cache>>) -> Self {
        Self { inner }
    }

    /// Tries to borrow the cache without panicking when an engine owns a mutable borrow.
    ///
    /// # Errors
    ///
    /// Returns an error when the cache is mutably borrowed.
    pub fn try_borrow(&self) -> Result<Ref<'_, Cache>, std::cell::BorrowError> {
        self.inner.try_borrow()
    }

    /// Borrows the cache immutably.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    pub fn borrow(&self) -> Ref<'_, Cache> {
        self.inner.borrow()
    }
}

impl From<Rc<RefCell<Cache>>> for CacheView {
    fn from(inner: Rc<RefCell<Cache>>) -> Self {
        Self::new(inner)
    }
}
