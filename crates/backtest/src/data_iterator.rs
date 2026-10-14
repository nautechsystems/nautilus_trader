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

//! Static backtest replay using the shared ordered batch iterator.

use nautilus_data::replay::DataIterator;
use nautilus_model::data::{Data, DataBatch, DataRef};

/// Multi-stream, time-ordered data iterator used by the backtest engine.
#[derive(Debug, Default)]
pub struct BacktestDataIterator {
    iterator: DataIterator,
}

impl BacktestDataIterator {
    /// Creates a new empty [`BacktestDataIterator`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            iterator: DataIterator::new(),
        }
    }

    /// Adds (or replaces) a named data stream.
    ///
    /// When `append_data` is true the stream gets lower priority on equal replay keys;
    /// when false (prepend) it wins ties. Named replacements retain their original priority.
    pub fn add_data(&mut self, name: &str, data: Vec<Data>, append_data: bool) {
        self.iterator.add_data(name, data, append_data);
    }

    /// Adds (or replaces) a named typed data stream.
    ///
    /// Items are ordered by replay key before insertion. A batch that arrives out of order while
    /// sharing its backing allocation with another view is copied once, so the shared allocation
    /// is never reordered.
    pub fn add_data_batch(&mut self, name: &str, data: DataBatch, append_data: bool) {
        self.iterator.add_data_batch(name, data, append_data);
    }

    /// Removes a named data stream.
    pub fn remove_data(&mut self, name: &str, complete_remove: bool) {
        self.iterator.remove_data(name, complete_remove);
    }

    /// Sets the cursor of a named stream to `index` (0-based).
    pub fn set_index(&mut self, name: &str, index: usize) {
        self.iterator.set_index(name, index);
    }

    /// Resets all stream cursors to the beginning.
    pub fn reset_all_cursors(&mut self) {
        self.iterator.reset_all_cursors();
    }

    pub(crate) fn peek(&self) -> Option<DataRef<'_>> {
        self.iterator.peek()
    }

    pub(crate) fn advance(&mut self) {
        self.iterator.advance();
    }

    /// Returns the next market [`Data`] element across all streams in chronological order.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Data> {
        self.iterator.next()
    }

    /// Returns whether all streams have been fully consumed.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.iterator.is_done()
    }
}
