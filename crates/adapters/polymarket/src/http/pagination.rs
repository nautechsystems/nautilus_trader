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

use std::{collections::HashSet, fmt::Debug, hash::Hash};

pub(crate) const PAGINATION_PAGE_LIMIT: usize = 100;
pub(crate) const PAGINATION_ROW_LIMIT: usize = 100_000;

#[derive(Eq, Hash, PartialEq)]
pub(crate) struct CursorKey(String);

impl CursorKey {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }
}

impl Debug for CursorKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cursor {:?}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PaginationError {
    #[error("{endpoint} pagination repeated {progress}")]
    Repeated {
        endpoint: &'static str,
        progress: String,
    },
    #[error("{endpoint} pagination exceeded {PAGINATION_PAGE_LIMIT} pages")]
    PageLimit { endpoint: &'static str },
    #[error("{endpoint} pagination exceeded {PAGINATION_ROW_LIMIT} rows")]
    RowLimit { endpoint: &'static str },
}

pub(crate) struct PaginationGuard<K> {
    endpoint: &'static str,
    pages: usize,
    rows: usize,
    seen: HashSet<K>,
}

impl<K> PaginationGuard<K>
where
    K: Debug + Eq + Hash,
{
    pub(crate) fn new(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            pages: 0,
            rows: 0,
            seen: HashSet::new(),
        }
    }

    pub(crate) fn seed(&mut self, key: K) {
        self.seen.insert(key);
    }

    pub(crate) fn advance(
        &mut self,
        rows: usize,
        progress: Option<K>,
    ) -> Result<(), PaginationError> {
        self.pages += 1;
        self.rows = match self.rows.checked_add(rows) {
            Some(total) if total <= PAGINATION_ROW_LIMIT => total,
            _ => {
                return Err(PaginationError::RowLimit {
                    endpoint: self.endpoint,
                });
            }
        };

        match progress {
            None => Ok(()),
            Some(key) if self.seen.contains(&key) => Err(PaginationError::Repeated {
                endpoint: self.endpoint,
                progress: format!("{key:?}"),
            }),
            Some(_) if self.pages >= PAGINATION_PAGE_LIMIT => Err(PaginationError::PageLimit {
                endpoint: self.endpoint,
            }),
            Some(key) => {
                self.seen.insert(key);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_pagination_guard_rejects_row_limit() {
        let mut guard = PaginationGuard::<CursorKey>::new("test");

        let error = guard.advance(100_001, None).unwrap_err();

        assert_eq!(error.to_string(), "test pagination exceeded 100000 rows");
    }
}
