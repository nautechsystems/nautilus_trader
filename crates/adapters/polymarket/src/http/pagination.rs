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

use std::{collections::HashSet, future::Future};

use aws_lc_rs::digest::{self, Context};

pub(crate) const PAGINATION_PAGE_LIMIT: usize = 100;
pub(crate) const PAGINATION_ROW_LIMIT: usize = 100_000;
pub(crate) const PAGINATION_CURSOR_LIMIT: usize = 8 * 1024;

#[derive(Debug)]
pub(crate) enum FetchOutcome<T, W, S> {
    Page { rows: Vec<T>, wire: W },
    Stop(S),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Completion<S> {
    WireComplete,
    Stopped(S),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Completed<O, S> {
    pub(crate) output: O,
    pub(crate) completion: Completion<S>,
}

pub(crate) enum PageObservation<P, C, S> {
    Terminal,
    Continue { next: P, commit: C },
    Stop(S),
}

pub(crate) type ObservationResult<P, C, S> = Result<PageObservation<P, C, S>, PaginationError>;

pub(crate) trait PageProtocol<T> {
    type Position: Clone;
    type Wire;
    type Stop;
    type Commit;

    fn initial_position(&self) -> Self::Position;

    fn observe(
        &mut self,
        position: &Self::Position,
        page_index: usize,
        rows: &[T],
        wire: Self::Wire,
    ) -> ObservationResult<Self::Position, Self::Commit, Self::Stop>;

    fn commit_continue(&mut self, commit: Self::Commit);
}

pub(crate) trait PageReducer<T, E> {
    type Output;
    type Stop;

    fn consume(&mut self, rows: Vec<T>) -> Result<Option<Self::Stop>, E>;

    fn finish(self, completion: &Completion<Self::Stop>) -> Result<Self::Output, E>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PageFingerprint([u8; 32]);

pub(crate) fn encode_length_prefixed(output: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("fingerprint field length exceeds u64");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

pub(crate) fn fingerprint_multiset(version: u8, mut descriptors: Vec<Vec<u8>>) -> PageFingerprint {
    descriptors.sort_unstable();
    let mut context = Context::new(&digest::SHA256);
    context.update(&[version]);
    let row_count = u64::try_from(descriptors.len()).expect("fingerprint row count exceeds u64");
    context.update(&row_count.to_be_bytes());
    for descriptor in descriptors {
        encode_length_prefixed_digest(&mut context, &descriptor);
    }
    let digest = context.finish();
    let mut fingerprint = [0; 32];
    fingerprint.copy_from_slice(digest.as_ref());
    PageFingerprint(fingerprint)
}

fn encode_length_prefixed_digest(context: &mut Context, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("fingerprint row length exceeds u64");
    context.update(&length.to_be_bytes());
    context.update(value);
}

pub(crate) struct OffsetProtocol<T, S> {
    endpoint: &'static str,
    page_size: usize,
    fingerprint: fn(&[T]) -> PageFingerprint,
    local_ceiling: Option<(u32, S)>,
    seen: HashSet<PageFingerprint>,
}

impl<T, S> OffsetProtocol<T, S> {
    pub(crate) fn new(
        endpoint: &'static str,
        page_size: usize,
        fingerprint: fn(&[T]) -> PageFingerprint,
        local_ceiling: Option<(u32, S)>,
    ) -> Self {
        Self {
            endpoint,
            page_size,
            fingerprint,
            local_ceiling,
            seen: HashSet::new(),
        }
    }
}

impl<T, S> PageProtocol<T> for OffsetProtocol<T, S>
where
    S: Clone,
{
    type Commit = PageFingerprint;
    type Position = u32;
    type Stop = S;
    type Wire = ();

    fn initial_position(&self) -> Self::Position {
        0
    }

    fn observe(
        &mut self,
        position: &Self::Position,
        page_index: usize,
        rows: &[T],
        (): Self::Wire,
    ) -> Result<PageObservation<Self::Position, Self::Commit, Self::Stop>, PaginationError> {
        if rows.len() < self.page_size {
            return Ok(PageObservation::Terminal);
        }

        let fingerprint = (self.fingerprint)(rows);
        if self.seen.contains(&fingerprint) {
            return Err(PaginationError::RepeatedPage {
                endpoint: self.endpoint,
                page: page_index,
                offset: *position,
            });
        }

        let count = u32::try_from(rows.len()).map_err(|_| PaginationError::OffsetOverflow {
            endpoint: self.endpoint,
        })?;
        let next = position
            .checked_add(count)
            .ok_or(PaginationError::OffsetOverflow {
                endpoint: self.endpoint,
            })?;

        if let Some((ceiling, stop)) = &self.local_ceiling
            && next >= *ceiling
        {
            return Ok(PageObservation::Stop(stop.clone()));
        }

        Ok(PageObservation::Continue {
            next,
            commit: fingerprint,
        })
    }

    fn commit_continue(&mut self, commit: Self::Commit) {
        self.seen.insert(commit);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BoundedCursor(String);

impl BoundedCursor {
    fn new(endpoint: &'static str, value: String) -> Result<Self, PaginationError> {
        if value.len() > PAGINATION_CURSOR_LIMIT {
            return Err(PaginationError::CursorTooLong { endpoint });
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for BoundedCursor {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub(crate) struct CursorProtocol<S> {
    endpoint: &'static str,
    initial: Option<BoundedCursor>,
    dialect: CursorDialect,
    seen: HashSet<BoundedCursor>,
    stop: std::marker::PhantomData<fn() -> S>,
}

#[derive(Debug)]
enum CursorDialect {
    Clob { terminal_cursor: &'static str },
    Gamma,
}

impl<S> CursorProtocol<S> {
    pub(crate) fn clob(
        endpoint: &'static str,
        initial: String,
        terminal_cursor: &'static str,
    ) -> Result<Self, PaginationError> {
        let initial = BoundedCursor::new(endpoint, initial)?;
        let seen = HashSet::from([initial.clone()]);
        Ok(Self {
            endpoint,
            initial: Some(initial),
            dialect: CursorDialect::Clob { terminal_cursor },
            seen,
            stop: std::marker::PhantomData,
        })
    }

    pub(crate) fn gamma(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            initial: None,
            dialect: CursorDialect::Gamma,
            seen: HashSet::new(),
            stop: std::marker::PhantomData,
        }
    }
}

impl<T, S> PageProtocol<T> for CursorProtocol<S> {
    type Commit = BoundedCursor;
    type Position = Option<BoundedCursor>;
    type Stop = S;
    type Wire = Option<String>;

    fn initial_position(&self) -> Self::Position {
        self.initial.clone()
    }

    fn observe(
        &mut self,
        position: &Self::Position,
        _page_index: usize,
        _rows: &[T],
        wire: Self::Wire,
    ) -> Result<PageObservation<Self::Position, Self::Commit, Self::Stop>, PaginationError> {
        let raw = match (&self.dialect, wire) {
            (CursorDialect::Gamma, None) => return Ok(PageObservation::Terminal),
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
                return Ok(PageObservation::Terminal);
            }
            CursorDialect::Clob { .. } | CursorDialect::Gamma => {}
        }

        let next = BoundedCursor::new(self.endpoint, raw)?;
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

        Ok(PageObservation::Continue {
            next: Some(next.clone()),
            commit: next,
        })
    }

    fn commit_continue(&mut self, commit: Self::Commit) {
        self.seen.insert(commit);
    }
}

pub(crate) struct CollectAll<T, S = std::convert::Infallible> {
    rows: Vec<T>,
    stop: std::marker::PhantomData<fn() -> S>,
}

impl<T, S> CollectAll<T, S> {
    pub(crate) const fn new() -> Self {
        Self {
            rows: Vec::new(),
            stop: std::marker::PhantomData,
        }
    }
}

impl<T, E, S> PageReducer<T, E> for CollectAll<T, S> {
    type Output = Vec<T>;
    type Stop = S;

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

pub(crate) struct Paginator<P, R> {
    endpoint: &'static str,
    protocol: P,
    reducer: R,
    pages: usize,
    fetched_rows: usize,
}

impl<P, R> Paginator<P, R> {
    pub(crate) const fn new(endpoint: &'static str, protocol: P, reducer: R) -> Self {
        Self {
            endpoint,
            protocol,
            reducer,
            pages: 0,
            fetched_rows: 0,
        }
    }

    pub(crate) async fn run<T, E, F, Fut, M>(
        mut self,
        mut fetch: F,
        mut map_pagination_error: M,
    ) -> Result<Completed<R::Output, R::Stop>, E>
    where
        P: PageProtocol<T, Stop = R::Stop>,
        R: PageReducer<T, E>,
        F: FnMut(P::Position) -> Fut,
        Fut: Future<Output = Result<FetchOutcome<T, P::Wire, R::Stop>, E>>,
        M: FnMut(PaginationError) -> E,
    {
        let mut position = self.protocol.initial_position();

        loop {
            let (rows, wire) = match fetch(position.clone()).await? {
                FetchOutcome::Page { rows, wire } => (rows, wire),
                FetchOutcome::Stop(stop) => {
                    let completion = Completion::Stopped(stop);
                    let output = self.reducer.finish(&completion)?;
                    return Ok(Completed { output, completion });
                }
            };

            let prospective_pages = self.pages.checked_add(1).ok_or_else(|| {
                map_pagination_error(PaginationError::PageLimit {
                    endpoint: self.endpoint,
                })
            })?;
            let prospective_rows = self
                .fetched_rows
                .checked_add(rows.len())
                .filter(|total| *total <= PAGINATION_ROW_LIMIT)
                .ok_or_else(|| {
                    map_pagination_error(PaginationError::RowLimit {
                        endpoint: self.endpoint,
                    })
                })?;
            let observation = self
                .protocol
                .observe(&position, prospective_pages, &rows, wire)
                .map_err(&mut map_pagination_error)?;

            self.pages = prospective_pages;
            self.fetched_rows = prospective_rows;

            if let Some(stop) = self.reducer.consume(rows)? {
                let completion = Completion::Stopped(stop);
                let output = self.reducer.finish(&completion)?;
                return Ok(Completed { output, completion });
            }

            match observation {
                PageObservation::Terminal => {
                    let completion = Completion::WireComplete;
                    let output = self.reducer.finish(&completion)?;
                    return Ok(Completed { output, completion });
                }
                PageObservation::Stop(stop) => {
                    let completion = Completion::Stopped(stop);
                    let output = self.reducer.finish(&completion)?;
                    return Ok(Completed { output, completion });
                }
                PageObservation::Continue { next, commit } => {
                    if self.pages >= PAGINATION_PAGE_LIMIT {
                        return Err(map_pagination_error(PaginationError::PageLimit {
                            endpoint: self.endpoint,
                        }));
                    }

                    if self.fetched_rows >= PAGINATION_ROW_LIMIT {
                        return Err(map_pagination_error(PaginationError::RowLimit {
                            endpoint: self.endpoint,
                        }));
                    }
                    self.protocol.commit_continue(commit);
                    position = next;
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PaginationError {
    #[error("{endpoint} pagination cursor exceeds {PAGINATION_CURSOR_LIMIT} bytes")]
    CursorTooLong { endpoint: &'static str },
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
    #[error("{endpoint} pagination repeated a full page at page {page} offset {offset}")]
    RepeatedPage {
        endpoint: &'static str,
        page: usize,
        offset: u32,
    },
    #[error("{endpoint} pagination offset overflowed u32")]
    OffsetOverflow { endpoint: &'static str },
    #[error("{endpoint} pagination exceeded {PAGINATION_PAGE_LIMIT} pages")]
    PageLimit { endpoint: &'static str },
    #[error("{endpoint} pagination exceeded {PAGINATION_ROW_LIMIT} rows")]
    RowLimit { endpoint: &'static str },
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use rstest::rstest;

    use super::*;

    struct LinearProtocol;

    impl PageProtocol<usize> for LinearProtocol {
        type Commit = ();
        type Position = usize;
        type Stop = Infallible;
        type Wire = bool;

        fn initial_position(&self) -> Self::Position {
            0
        }

        fn observe(
            &mut self,
            position: &Self::Position,
            _page_index: usize,
            _rows: &[usize],
            terminal: Self::Wire,
        ) -> Result<PageObservation<Self::Position, Self::Commit, Self::Stop>, PaginationError>
        {
            if terminal {
                Ok(PageObservation::Terminal)
            } else {
                Ok(PageObservation::Continue {
                    next: position + 1,
                    commit: (),
                })
            }
        }

        fn commit_continue(&mut self, (): Self::Commit) {}
    }

    struct StopProtocol;

    impl PageProtocol<usize> for StopProtocol {
        type Commit = ();
        type Position = usize;
        type Stop = &'static str;
        type Wire = ();

        fn initial_position(&self) -> Self::Position {
            0
        }

        fn observe(
            &mut self,
            _position: &Self::Position,
            _page_index: usize,
            _rows: &[usize],
            (): Self::Wire,
        ) -> Result<PageObservation<Self::Position, Self::Commit, Self::Stop>, PaginationError>
        {
            Ok(PageObservation::Stop("local"))
        }

        fn commit_continue(&mut self, (): Self::Commit) {}
    }

    struct TrackingReducer {
        consume_calls: Arc<AtomicUsize>,
    }

    impl PageReducer<usize, PaginationError> for TrackingReducer {
        type Output = ();
        type Stop = Infallible;

        fn consume(&mut self, _rows: Vec<usize>) -> Result<Option<Self::Stop>, PaginationError> {
            self.consume_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        fn finish(
            self,
            _completion: &Completion<Self::Stop>,
        ) -> Result<Self::Output, PaginationError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_paginator_accepts_terminal_page_100_without_request_101() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_fetch = Arc::clone(&requests);
        let paginator = Paginator::new("test", LinearProtocol, CollectAll::new());

        let completed = paginator
            .run(
                move |position| {
                    requests_for_fetch.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Ok::<_, PaginationError>(FetchOutcome::Page {
                            rows: vec![position],
                            wire: position == PAGINATION_PAGE_LIMIT - 1,
                        })
                    }
                },
                std::convert::identity,
            )
            .await
            .unwrap();

        assert!(matches!(completed.completion, Completion::WireComplete));
        assert_eq!(completed.output.len(), PAGINATION_PAGE_LIMIT);
        assert_eq!(requests.load(Ordering::SeqCst), PAGINATION_PAGE_LIMIT);
    }

    #[tokio::test]
    async fn test_paginator_rejects_continuation_after_page_100_without_request_101() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_fetch = Arc::clone(&requests);
        let paginator = Paginator::new("test", LinearProtocol, CollectAll::new());

        let error = paginator
            .run(
                move |position| {
                    requests_for_fetch.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Ok::<_, PaginationError>(FetchOutcome::Page {
                            rows: vec![position],
                            wire: false,
                        })
                    }
                },
                std::convert::identity,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            PaginationError::PageLimit { endpoint: "test" }
        ));
        assert_eq!(requests.load(Ordering::SeqCst), PAGINATION_PAGE_LIMIT);
    }

    #[tokio::test]
    async fn test_paginator_accepts_exact_row_limit_when_terminal() {
        let paginator = Paginator::new("test", LinearProtocol, CollectAll::new());

        let completed = paginator
            .run(
                |_position| async {
                    Ok::<_, PaginationError>(FetchOutcome::Page {
                        rows: vec![0; PAGINATION_ROW_LIMIT],
                        wire: true,
                    })
                },
                std::convert::identity,
            )
            .await
            .unwrap();

        assert_eq!(completed.output.len(), PAGINATION_ROW_LIMIT);
    }

    #[tokio::test]
    async fn test_paginator_rejects_continuation_at_exact_row_limit() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_fetch = Arc::clone(&requests);
        let paginator = Paginator::new("test", LinearProtocol, CollectAll::new());

        let error = paginator
            .run(
                move |_position| {
                    requests_for_fetch.fetch_add(1, Ordering::SeqCst);
                    async {
                        Ok::<_, PaginationError>(FetchOutcome::Page {
                            rows: vec![0; PAGINATION_ROW_LIMIT],
                            wire: false,
                        })
                    }
                },
                std::convert::identity,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            PaginationError::RowLimit { endpoint: "test" }
        ));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_paginator_rejects_oversized_page_before_reducer_mutation() {
        let consume_calls = Arc::new(AtomicUsize::new(0));
        let reducer = TrackingReducer {
            consume_calls: Arc::clone(&consume_calls),
        };
        let paginator = Paginator::new("test", LinearProtocol, reducer);

        let error = paginator
            .run(
                |_position| async {
                    Ok::<_, PaginationError>(FetchOutcome::Page {
                        rows: vec![0; PAGINATION_ROW_LIMIT + 1],
                        wire: true,
                    })
                },
                std::convert::identity,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            PaginationError::RowLimit { endpoint: "test" }
        ));
        assert_eq!(consume_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_paginator_finalizes_remote_stop_without_fetching_again() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_fetch = Arc::clone(&requests);
        let paginator = Paginator::new(
            "test",
            StopProtocol,
            CollectAll::<usize, &'static str>::new(),
        );

        let completed = paginator
            .run(
                move |_position| {
                    requests_for_fetch.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, PaginationError>(FetchOutcome::Stop("remote")) }
                },
                std::convert::identity,
            )
            .await
            .unwrap();

        assert_eq!(completed.completion, Completion::Stopped("remote"));
        assert!(completed.output.is_empty());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_paginator_finalizes_protocol_stop_after_consuming_page() {
        let paginator = Paginator::new(
            "test",
            StopProtocol,
            CollectAll::<usize, &'static str>::new(),
        );

        let completed = paginator
            .run(
                |_position| async {
                    Ok::<_, PaginationError>(FetchOutcome::Page {
                        rows: vec![7],
                        wire: (),
                    })
                },
                std::convert::identity,
            )
            .await
            .unwrap();

        assert_eq!(completed.completion, Completion::Stopped("local"));
        assert_eq!(completed.output, vec![7]);
    }

    #[rstest]
    fn test_clob_cursor_protocol_rejects_overlong_initial_cursor() {
        let cursor = "x".repeat(PAGINATION_CURSOR_LIMIT + 1);

        let error = CursorProtocol::<Infallible>::clob("test", cursor, "END").unwrap_err();

        assert!(matches!(
            error,
            PaginationError::CursorTooLong { endpoint: "test" }
        ));
    }

    #[tokio::test]
    async fn test_clob_cursor_protocol_reports_immediate_echo_as_stalled() {
        let protocol =
            CursorProtocol::<Infallible>::clob("test", "seed".to_string(), "END").unwrap();
        let paginator = Paginator::new("test", protocol, CollectAll::new());

        let error = paginator
            .run(
                |_position| async {
                    Ok::<_, PaginationError>(FetchOutcome::Page {
                        rows: vec![1],
                        wire: Some("seed".to_string()),
                    })
                },
                std::convert::identity,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            PaginationError::StalledCursor {
                endpoint: "test",
                cursor
            } if cursor == "seed"
        ));
    }
}
