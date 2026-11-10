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

use std::{collections::HashSet, convert::Infallible, future::Future};

#[derive(Debug)]
pub(crate) struct FetchOutcome<T> {
    pub(crate) rows: Vec<T>,
    pub(crate) wire: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Completion<S> {
    WireExhausted,
    Stopped(S),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Completed<O, S> {
    pub(crate) output: O,
    pub(crate) completion: Completion<S>,
}

pub(crate) trait PageReducer<T, E> {
    type Output;
    type Stop;

    fn consume(&mut self, rows: Vec<T>) -> Result<Option<Self::Stop>, E>;

    fn finish(self, completion: &Completion<Self::Stop>) -> Result<Self::Output, E>;
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Cursor(String);

impl AsRef<str> for Cursor {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub(crate) struct CursorProtocol {
    endpoint: &'static str,
    initial: Option<Cursor>,
    dialect: CursorDialect,
    seen: HashSet<Cursor>,
}

#[derive(Debug)]
enum CursorDialect {
    Clob { terminal_cursor: &'static str },
    Gamma,
}

impl CursorProtocol {
    pub(crate) fn clob(
        endpoint: &'static str,
        initial: String,
        terminal_cursor: &'static str,
    ) -> Self {
        let initial = Cursor(initial);
        let seen = HashSet::from([initial.clone()]);
        Self {
            endpoint,
            initial: Some(initial),
            dialect: CursorDialect::Clob { terminal_cursor },
            seen,
        }
    }

    pub(crate) fn gamma(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            initial: None,
            dialect: CursorDialect::Gamma,
            seen: HashSet::new(),
        }
    }

    fn observe(
        &self,
        position: &Option<Cursor>,
        wire: Option<String>,
    ) -> Result<Option<Cursor>, PaginationError> {
        let raw = match (&self.dialect, wire) {
            (CursorDialect::Gamma, None) => return Ok(None),
            (CursorDialect::Clob { .. }, None) => {
                return Err(PaginationError::MissingCursor {
                    endpoint: self.endpoint,
                });
            }
            (_, Some(raw)) => raw,
        };

        match &self.dialect {
            CursorDialect::Clob { terminal_cursor }
                if raw.is_empty() || raw == *terminal_cursor =>
            {
                return Ok(None);
            }
            CursorDialect::Clob { .. } | CursorDialect::Gamma => {}
        }

        let next = Cursor(raw);
        if matches!(&self.dialect, CursorDialect::Clob { .. }) && position.as_ref() == Some(&next) {
            return Err(PaginationError::StalledCursor {
                endpoint: self.endpoint,
                cursor: next.0,
            });
        }

        if self.seen.contains(&next) {
            return Err(PaginationError::RepeatedCursor {
                endpoint: self.endpoint,
                cursor: next.0,
            });
        }

        Ok(Some(next))
    }
}

pub(crate) struct CollectAll<T> {
    rows: Vec<T>,
}

impl<T> CollectAll<T> {
    pub(crate) const fn new() -> Self {
        Self { rows: Vec::new() }
    }
}

impl<T, E> PageReducer<T, E> for CollectAll<T> {
    type Output = Vec<T>;
    type Stop = Infallible;

    fn consume(&mut self, rows: Vec<T>) -> Result<Option<Self::Stop>, E> {
        self.rows.extend(rows);
        Ok(None)
    }

    fn finish(self, _completion: &Completion<Self::Stop>) -> Result<Self::Output, E> {
        Ok(self.rows)
    }
}

pub(crate) struct WindowedCollect<T, S> {
    remaining_skip: usize,
    maximum: Option<usize>,
    rows: Vec<T>,
    stop: S,
}

impl<T, S> WindowedCollect<T, S> {
    pub(crate) const fn new(remaining_skip: usize, maximum: Option<usize>, stop: S) -> Self {
        Self {
            remaining_skip,
            maximum,
            rows: Vec::new(),
            stop,
        }
    }
}

impl<T, E, S> PageReducer<T, E> for WindowedCollect<T, S>
where
    S: Clone,
{
    type Output = Vec<T>;
    type Stop = S;

    fn consume(&mut self, rows: Vec<T>) -> Result<Option<Self::Stop>, E> {
        let skipped = self.remaining_skip.min(rows.len());
        self.remaining_skip -= skipped;
        let retained = rows.into_iter().skip(skipped);

        if let Some(maximum) = self.maximum {
            let remaining = maximum.saturating_sub(self.rows.len());
            self.rows.extend(retained.take(remaining));
            if self.rows.len() >= maximum {
                return Ok(Some(self.stop.clone()));
            }
        } else {
            self.rows.extend(retained);
        }

        Ok(None)
    }

    fn finish(self, _completion: &Completion<Self::Stop>) -> Result<Self::Output, E> {
        Ok(self.rows)
    }
}

pub(crate) struct Paginator<R> {
    endpoint: &'static str,
    protocol: CursorProtocol,
    reducer: R,
    pages: usize,
}

impl<R> Paginator<R> {
    pub(crate) const fn new(endpoint: &'static str, protocol: CursorProtocol, reducer: R) -> Self {
        Self {
            endpoint,
            protocol,
            reducer,
            pages: 0,
        }
    }

    pub(crate) async fn run<T, E, F, Fut, M>(
        mut self,
        mut fetch: F,
        mut map_pagination_error: M,
    ) -> Result<Completed<R::Output, R::Stop>, E>
    where
        R: PageReducer<T, E>,
        F: FnMut(Option<Cursor>) -> Fut,
        Fut: Future<Output = Result<FetchOutcome<T>, E>>,
        M: FnMut(PaginationError) -> E,
    {
        let mut position = self.protocol.initial.clone();
        let completion = loop {
            let FetchOutcome { rows, wire } = fetch(position.clone()).await?;

            let page = self.pages.checked_add(1).ok_or_else(|| {
                map_pagination_error(PaginationError::PageCounterOverflow {
                    endpoint: self.endpoint,
                })
            })?;

            let next = self
                .protocol
                .observe(&position, wire)
                .map_err(&mut map_pagination_error)?;

            log::debug!("Fetched {} page {page}: {} rows", self.endpoint, rows.len(),);
            self.pages = page;

            if let Some(stop) = self.reducer.consume(rows)? {
                break Completion::Stopped(stop);
            }

            let Some(next) = next else {
                break Completion::WireExhausted;
            };

            self.protocol.seen.insert(next.clone());
            position = Some(next);
        };

        let output = self.reducer.finish(&completion)?;
        Ok(Completed { output, completion })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PaginationError {
    #[error("{endpoint} response omitted next_cursor")]
    MissingCursor { endpoint: &'static str },
    #[error("{endpoint} pagination cursor did not advance from {cursor:?}")]
    StalledCursor {
        endpoint: &'static str,
        cursor: String,
    },
    #[error("{endpoint} pagination repeated cursor {cursor:?}")]
    RepeatedCursor {
        endpoint: &'static str,
        cursor: String,
    },
    #[error("{endpoint} pagination page counter overflowed usize")]
    PageCounterOverflow { endpoint: &'static str },
}
