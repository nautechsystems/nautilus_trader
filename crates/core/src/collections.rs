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

//! Abstraction layer over common hash-based containers.

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
};

/// Represents a generic map-like container with keys and values.
pub trait MapLike {
    type Key: Hash + Eq + Display + Clone;
    type Value: Debug;

    fn contains_key(&self, key: &Self::Key) -> bool;
    fn is_empty(&self) -> bool;
}

impl<K, V, S> MapLike for HashMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.contains_key(k)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V, S> MapLike for indexmap::IndexMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.get(k).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V, S> MapLike for ahash::AHashMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.get(k).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
