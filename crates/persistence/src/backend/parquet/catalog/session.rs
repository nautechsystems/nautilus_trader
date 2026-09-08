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

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

use nautilus_core::UnixNanos;
use nautilus_model::data::HasTsInit;

pub(super) type TypedPages<T> = Box<dyn Iterator<Item = anyhow::Result<Vec<T>>> + Send>;

pub(super) struct MergedPages<T> {
    sources: Vec<TypedPages<T>>,
    buffers: Vec<VecDeque<T>>,
    heads: BinaryHeap<Reverse<(UnixNanos, usize)>>,
    page_size: usize,
    initialized: bool,
    finished: bool,
}

impl<T: HasTsInit> MergedPages<T> {
    pub(super) fn new(sources: Vec<TypedPages<T>>, page_size: usize) -> Self {
        Self {
            buffers: (0..sources.len()).map(|_| VecDeque::new()).collect(),
            sources,
            heads: BinaryHeap::new(),
            page_size: page_size.max(1),
            initialized: false,
            finished: false,
        }
    }

    fn read_page(&mut self) -> anyhow::Result<Option<Vec<T>>> {
        if !self.initialized {
            self.initialized = true;

            for index in 0..self.sources.len() {
                self.advance(index)?;
            }
        }
        let mut page = Vec::with_capacity(self.page_size);
        while page.len() < self.page_size {
            let Some(Reverse((_, index))) = self.heads.pop() else {
                break;
            };
            page.push(
                self.buffers[index]
                    .pop_front()
                    .expect("queued source has a row"),
            );
            self.advance(index)?;
        }
        Ok((!page.is_empty()).then_some(page))
    }

    fn advance(&mut self, index: usize) -> anyhow::Result<()> {
        while self.buffers[index].is_empty() {
            match self.sources[index].next() {
                Some(page) => self.buffers[index].extend(page?),
                None => return Ok(()),
            }
        }
        let row = self.buffers[index].front().expect("source has a row");
        self.heads.push(Reverse((row.ts_init(), index)));
        Ok(())
    }
}

impl<T: HasTsInit> Iterator for MergedPages<T> {
    type Item = anyhow::Result<Vec<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.read_page() {
            Ok(Some(page)) => Some(Ok(page)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(e) => {
                self.finished = true;
                Some(Err(e))
            }
        }
    }
}
