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
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    hash::Hash,
};

use ustr::Ustr;

/// Represents a generic set-like container with members.
pub trait SetLike {
    /// The type of items stored in the set.
    type Item: Hash + Eq + Display + Clone;

    /// Returns `true` if the set contains the specified item.
    fn contains(&self, item: &Self::Item) -> bool;
    /// Returns `true` if the set is empty.
    fn is_empty(&self) -> bool;
}

impl<T, S> SetLike for HashSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        Self::contains(self, v)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }
}

impl<T, S> SetLike for indexmap::IndexSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        Self::contains(self, v)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }
}

impl<T, S> SetLike for ahash::AHashSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        self.get(v).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Represents a generic map-like container with key-value pairs.
pub trait MapLike {
    /// The type of keys stored in the map.
    type Key: Hash + Eq + Display + Clone;
    /// The type of values stored in the map.
    type Value: Debug;

    /// Returns `true` if the map contains the specified key.
    fn contains_key(&self, key: &Self::Key) -> bool;
    /// Returns `true` if the map is empty.
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

/// Convert any iterator of string-like items into a `Vec<Ustr>`.
#[must_use]
pub fn into_ustr_vec<I, T>(iter: I) -> Vec<Ustr>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let iter = iter.into_iter();
    let (lower, _) = iter.size_hint();
    let mut result = Vec::with_capacity(lower);

    for item in iter {
        result.push(Ustr::from(item.as_ref()));
    }

    result
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[allow(clippy::unnecessary_to_owned)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use ahash::{AHashMap, AHashSet};
    use indexmap::{IndexMap, IndexSet};
    use rstest::*;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_hashset_setlike() {
        let mut set: HashSet<String> = HashSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: HashSet<String> = HashSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_indexset_setlike() {
        let mut set: IndexSet<String> = IndexSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: IndexSet<String> = IndexSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_into_ustr_vec_from_strings() {
        let items = vec!["foo".to_string(), "bar".to_string()];
        let ustrs = super::into_ustr_vec(items);

        assert_eq!(ustrs.len(), 2);
        assert_eq!(ustrs[0], Ustr::from("foo"));
        assert_eq!(ustrs[1], Ustr::from("bar"));
    }

    #[rstest]
    fn test_into_ustr_vec_from_str_slices() {
        let items = ["alpha", "beta", "gamma"];
        let ustrs = super::into_ustr_vec(items);

        assert_eq!(ustrs.len(), 3);
        assert_eq!(ustrs[2], Ustr::from("gamma"));
    }

    #[rstest]
    fn test_ahashset_setlike() {
        let mut set: AHashSet<String> = AHashSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: AHashSet<String> = AHashSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_hashmap_maplike() {
        let mut map: HashMap<String, i32> = HashMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: HashMap<String, i32> = HashMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_indexmap_maplike() {
        let mut map: IndexMap<String, i32> = IndexMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: IndexMap<String, i32> = IndexMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_ahashmap_maplike() {
        let mut map: AHashMap<String, i32> = AHashMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: AHashMap<String, i32> = AHashMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_trait_object_setlike() {
        let mut hashset: HashSet<String> = HashSet::new();
        hashset.insert("test".to_string());

        let mut indexset: IndexSet<String> = IndexSet::new();
        indexset.insert("test".to_string());

        let sets: Vec<&dyn SetLike<Item = String>> = vec![&hashset, &indexset];

        for set in sets {
            assert!(set.contains(&"test".to_string()));
            assert!(!set.is_empty());
        }
    }

    #[rstest]
    fn test_trait_object_maplike() {
        let mut hashmap: HashMap<String, i32> = HashMap::new();
        hashmap.insert("key".to_string(), 42);

        let mut indexmap: IndexMap<String, i32> = IndexMap::new();
        indexmap.insert("key".to_string(), 42);

        let maps: Vec<&dyn MapLike<Key = String, Value = i32>> = vec![&hashmap, &indexmap];

        for map in maps {
            assert!(map.contains_key(&"key".to_string()));
            assert!(!map.is_empty());
        }
    }
}
