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
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for
//  the specific language governing permissions and limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Typed catalog query sessions.
//!
//! Every typed batch session yields rows in non-decreasing `ts_init` order. Pages must preserve
//! that order across the whole source; debug builds assert the invariant for every yielded row.

use std::collections::VecDeque;

use nautilus_core::UnixNanos;
use nautilus_model::data::{DataBatch, HasTsInit, IntoDataBatch};

pub const DEFAULT_DATA_BATCH_CHUNK_SIZE: usize = 10_000;

/// Streaming query session that yields ordered [`DataBatch`] chunks.
pub trait DataBatchQuery: Send {
    /// Returns the next typed batch, or `None` after the session is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying page source or typed conversion fails.
    fn next_batch(&mut self) -> anyhow::Result<Option<DataBatch>>;

    /// Resets the session when the backend supports replaying its source.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot reset its source state.
    fn reset(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }
}

pub type DataBatchQueryResult = Box<dyn DataBatchQuery>;

/// Adapts pages of ordered typed values into timestamp-aligned [`DataBatch`] chunks.
pub struct TypedDataBatchSession<T> {
    pages: Box<dyn Iterator<Item = anyhow::Result<Vec<T>>> + Send>,
    carry: VecDeque<T>,
    chunk_size: usize,
    previous_ts_init: Option<UnixNanos>,
}

impl<T> TypedDataBatchSession<T>
where
    T: IntoDataBatch + HasTsInit + Send,
{
    /// Creates a session over a page source.
    #[must_use]
    pub fn new(
        pages: Box<dyn Iterator<Item = anyhow::Result<Vec<T>>> + Send>,
        chunk_size: Option<usize>,
    ) -> Self {
        Self {
            pages,
            carry: VecDeque::new(),
            chunk_size: chunk_size.unwrap_or(DEFAULT_DATA_BATCH_CHUNK_SIZE).max(1),
            previous_ts_init: None,
        }
    }

    /// Creates a session over one collected, ordered vector.
    #[must_use]
    pub fn from_vec(data: Vec<T>, chunk_size: Option<usize>) -> Self
    where
        T: 'static,
    {
        Self::new(Box::new(std::iter::once(Ok(data))), chunk_size)
    }

    fn next_row(&mut self) -> anyhow::Result<Option<T>> {
        loop {
            if let Some(row) = self.carry.pop_front() {
                debug_assert!(
                    self.previous_ts_init
                        .is_none_or(|previous| previous <= row.ts_init()),
                    "TypedDataBatchSession ordering invariant violated: ts_init {} after {:?}",
                    row.ts_init(),
                    self.previous_ts_init,
                );
                self.previous_ts_init = Some(row.ts_init());
                return Ok(Some(row));
            }

            match self.pages.next() {
                Some(page) => self.carry.extend(page?),
                None => return Ok(None),
            }
        }
    }
}

impl<T> DataBatchQuery for TypedDataBatchSession<T>
where
    T: IntoDataBatch + HasTsInit + Send,
{
    fn next_batch(&mut self) -> anyhow::Result<Option<DataBatch>> {
        let Some(first) = self.next_row()? else {
            return Ok(None);
        };

        let mut chunk = Vec::with_capacity(self.chunk_size.min(1024));
        chunk.push(first);

        while chunk.len() < self.chunk_size {
            let Some(item) = self.next_row()? else {
                return Ok(Some(T::into_batch(chunk)));
            };
            chunk.push(item);
        }

        let boundary_ts = chunk.last().map(HasTsInit::ts_init);

        loop {
            match self.next_row()? {
                Some(item) if Some(item.ts_init()) == boundary_ts => chunk.push(item),
                Some(item) => {
                    self.carry.push_front(item);
                    break;
                }
                None => break,
            }
        }

        Ok(Some(T::into_batch(chunk)))
    }
}
