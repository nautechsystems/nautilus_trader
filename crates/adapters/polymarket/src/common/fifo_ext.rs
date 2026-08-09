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

//! Loud insertion helpers for bounded correction and identity caches.

use std::{fmt::Debug, hash::Hash};

use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};

/// Adds `id` to the cache, warning when the insertion evicts the oldest entry.
pub(crate) fn add_to_fifo_with_eviction_warn<T, const N: usize>(
    cache: &mut FifoCache<T, N>,
    id: T,
    what: &str,
) -> bool
where
    T: Clone + Debug + Eq + Hash,
{
    let will_evict = cache.len() == N && !cache.contains(&id);

    if will_evict {
        log::warn!("{what} cache is at capacity ({N}); evicting its oldest entry");
    }

    cache.add(id);
    will_evict
}

/// Inserts `key` into the cache, warning when the insertion evicts the oldest entry.
pub(crate) fn add_to_fifo_map_with_eviction_warn<K, V, const N: usize>(
    cache: &mut FifoCacheMap<K, V, N>,
    key: K,
    value: V,
    what: &str,
) -> bool
where
    K: Clone + Debug + Eq + Hash,
{
    let will_evict = cache.len() == N && !cache.contains_key(&key);

    if will_evict {
        log::warn!("{what} cache is at capacity ({N}); evicting its oldest entry");
    }

    cache.insert(key, value);
    will_evict
}

#[cfg(test)]
mod tests {
    use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
    use rstest::rstest;

    use super::{add_to_fifo_map_with_eviction_warn, add_to_fifo_with_eviction_warn};
    use crate::common::test_logger::{capture_start, records_since};

    #[rstest]
    fn fifo_insertions_warn_only_when_they_evict() {
        let log_start = capture_start();
        let mut cache = FifoCache::<u8, 1>::default();

        add_to_fifo_with_eviction_warn(&mut cache, 1, "test FIFO");
        add_to_fifo_with_eviction_warn(&mut cache, 1, "test FIFO");

        assert!(
            records_since(log_start)
                .iter()
                .all(|(_, message)| !message.contains("test FIFO cache"))
        );

        add_to_fifo_with_eviction_warn(&mut cache, 2, "test FIFO");

        let records = records_since(log_start)
            .into_iter()
            .filter(|(_, message)| message.contains("test FIFO cache"))
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, log::Level::Warn);
        assert!(records[0].1.contains("test FIFO cache is at capacity (1)"));
    }

    #[rstest]
    fn fifo_map_insertions_warn_only_when_they_evict() {
        let log_start = capture_start();
        let mut cache = FifoCacheMap::<u8, u8, 1>::default();

        add_to_fifo_map_with_eviction_warn(&mut cache, 1, 10, "test FIFO map");
        add_to_fifo_map_with_eviction_warn(&mut cache, 1, 20, "test FIFO map");

        assert!(
            records_since(log_start)
                .iter()
                .all(|(_, message)| !message.contains("test FIFO map cache"))
        );

        add_to_fifo_map_with_eviction_warn(&mut cache, 2, 30, "test FIFO map");

        let records = records_since(log_start)
            .into_iter()
            .filter(|(_, message)| message.contains("test FIFO map cache"))
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, log::Level::Warn);
        assert!(
            records[0]
                .1
                .contains("test FIFO map cache is at capacity (1)")
        );
    }
}
