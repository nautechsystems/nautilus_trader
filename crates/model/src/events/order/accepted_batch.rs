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

use super::accepted::OrderAccepted;

/// Represents a batch of [`OrderAccepted`] events from a single batch-submit
/// response. Transported as one message across the event loop, then unpacked
/// into individual [`OrderAccepted`] events for processing.
#[derive(Clone, PartialEq, Eq)]
pub struct OrderAcceptedBatch {
    pub events: Vec<OrderAccepted>,
}

impl OrderAcceptedBatch {
    /// Creates a new [`OrderAcceptedBatch`] instance.
    #[must_use]
    pub fn new(events: Vec<OrderAccepted>) -> Self {
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

impl Debug for OrderAcceptedBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderAcceptedBatch))
            .field("len", &self.events.len())
            .finish()
    }
}

impl Display for OrderAcceptedBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(len={})",
            stringify!(OrderAcceptedBatch),
            self.events.len()
        )
    }
}

impl IntoIterator for OrderAcceptedBatch {
    type Item = OrderAccepted;
    type IntoIter = std::vec::IntoIter<OrderAccepted>;

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
        let batch = OrderAcceptedBatch::new(Vec::new());
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }

    #[rstest]
    fn test_batch_with_events() {
        let events = vec![OrderAccepted::default(), OrderAccepted::default()];
        let batch = OrderAcceptedBatch::new(events);
        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 2);
    }

    #[rstest]
    fn test_debug_display() {
        let batch = OrderAcceptedBatch::new(vec![OrderAccepted::default()]);
        assert_eq!(format!("{batch}"), "OrderAcceptedBatch(len=1)");
        assert_eq!(format!("{batch:?}"), "OrderAcceptedBatch { len: 1 }");
    }

    #[rstest]
    fn test_into_iter() {
        let events = vec![OrderAccepted::default(), OrderAccepted::default()];
        let batch = OrderAcceptedBatch::new(events);
        assert_eq!(batch.into_iter().count(), 2);
    }
}
