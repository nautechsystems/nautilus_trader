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

use std::{hash::Hash, time::Duration};

use indexmap::IndexMap;
use nautilus_common::live::dst;

#[derive(Debug, Clone)]
pub(crate) struct RecencyMap<K> {
    inner: IndexMap<K, dst::time::Instant>,
}

impl<K> Default for RecencyMap<K> {
    fn default() -> Self {
        Self {
            inner: IndexMap::new(),
        }
    }
}

impl<K> RecencyMap<K>
where
    K: Eq + Hash,
{
    pub(crate) fn mark(&mut self, key: K) {
        self.inner.insert(key, dst::time::Instant::now());
    }

    #[must_use]
    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    pub(crate) fn remove(&mut self, key: &K) {
        self.inner.shift_remove(key);
    }

    #[must_use]
    pub(crate) fn last_marked(&self, key: &K) -> Option<dst::time::Instant> {
        self.inner.get(key).copied()
    }

    #[must_use]
    pub(crate) fn within(&self, key: &K, window: Duration) -> bool {
        self.elapsed(key).is_some_and(|elapsed| elapsed < window)
    }

    #[must_use]
    pub(crate) fn within_at(&self, key: &K, now: dst::time::Instant, window: Duration) -> bool {
        self.elapsed_at(key, now)
            .is_some_and(|elapsed| elapsed < window)
    }

    #[must_use]
    pub(crate) fn elapsed(&self, key: &K) -> Option<Duration> {
        self.elapsed_at(key, dst::time::Instant::now())
    }

    #[must_use]
    pub(crate) fn elapsed_at(&self, key: &K, now: dst::time::Instant) -> Option<Duration> {
        self.inner
            .get(key)
            .map(|marked_at| now.checked_duration_since(*marked_at).unwrap_or_default())
    }

    pub(crate) fn prune_older_than(&mut self, ttl: Duration) {
        let now = dst::time::Instant::now();
        self.inner.retain(|_, marked_at| {
            now.checked_duration_since(*marked_at)
                .is_none_or(|elapsed| elapsed <= ttl)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nautilus_common::live::dst;
    use rstest::rstest;

    use super::RecencyMap;

    #[cfg(all(feature = "simulation", madsim))]
    async fn advance_clock(d: Duration) {
        madsim::time::advance(d);
        madsim::task::yield_now().await;
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    async fn advance_clock(d: Duration) {
        tokio::time::advance(d).await;
    }

    #[rstest]
    fn test_mark_contains_and_remove() {
        let mut recency = RecencyMap::default();

        recency.mark("A");

        assert!(recency.contains_key(&"A"));

        recency.remove(&"A");

        assert!(!recency.contains_key(&"A"));
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_within_uses_monotonic_elapsed_time() {
        let mut recency = RecencyMap::default();

        recency.mark("A");

        assert!(recency.within(&"A", Duration::from_millis(100)));

        advance_clock(Duration::from_millis(100)).await;

        assert!(!recency.within(&"A", Duration::from_millis(100)));
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_prune_older_than_removes_expired_entries() {
        let mut recency = RecencyMap::default();

        recency.mark("OLD");
        advance_clock(Duration::from_mins(2)).await;
        recency.mark("NEW");

        recency.prune_older_than(Duration::from_mins(1));

        assert!(!recency.contains_key(&"OLD"));
        assert!(recency.contains_key(&"NEW"));
    }

    #[rstest]
    fn test_future_mark_counts_as_within_window() {
        let mut recency = RecencyMap::default();
        let now = dst::time::Instant::now();

        recency.inner.insert("A", now + Duration::from_secs(1));

        assert!(recency.within_at(&"A", now, Duration::from_millis(1)));
    }
}
