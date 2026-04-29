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

use std::fmt::{Debug, Display};

use super::canceled::OrderCanceled;

/// Represents a batch of [`OrderCanceled`] events from a single cancel-all
/// or batch-cancel response. Transported as one message across the event loop,
/// then unpacked into individual [`OrderCanceled`] events for processing.
#[derive(Clone, PartialEq, Eq)]
pub struct OrderCanceledBatch {
    pub events: Vec<OrderCanceled>,
}

impl OrderCanceledBatch {
    /// Creates a new [`OrderCanceledBatch`] instance.
    #[must_use]
    pub fn new(events: Vec<OrderCanceled>) -> Self {
        Self { events }
    }

    /// Returns the number of events in the batch.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns whether the batch is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Debug for OrderCanceledBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderCanceledBatch))
            .field("len", &self.events.len())
            .finish()
    }
}

impl Display for OrderCanceledBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(len={})",
            stringify!(OrderCanceledBatch),
            self.events.len()
        )
    }
}

impl IntoIterator for OrderCanceledBatch {
    type Item = OrderCanceled;
    type IntoIter = std::vec::IntoIter<OrderCanceled>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_empty_batch() {
        let batch = OrderCanceledBatch::new(Vec::new());
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[rstest]
    fn test_batch_with_events() {
        let events = vec![OrderCanceled::default(), OrderCanceled::default()];
        let batch = OrderCanceledBatch::new(events);
        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 2);
    }

    #[rstest]
    fn test_debug_display() {
        let batch = OrderCanceledBatch::new(vec![OrderCanceled::default()]);
        assert_eq!(format!("{batch}"), "OrderCanceledBatch(len=1)");
        assert_eq!(format!("{batch:?}"), "OrderCanceledBatch { len: 1 }");
    }

    #[rstest]
    fn test_into_iter() {
        let events = vec![OrderCanceled::default(), OrderCanceled::default()];
        let batch = OrderCanceledBatch::new(events);
        assert_eq!(batch.into_iter().count(), 2);
    }
}
