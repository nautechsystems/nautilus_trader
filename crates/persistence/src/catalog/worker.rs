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

//! Single-owner catalog worker for serialized query/write access.

use std::{
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread::{self, JoinHandle},
};

use ahash::AHashMap;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{DataBatch, NautilusDataType},
    instruments::InstrumentAny,
};

use super::{
    traits::DataCatalogBox,
    types::{CatalogInstrumentQuery, CatalogQuery},
};
use crate::{catalog::session::DataBatchQueryResult, common::coverage::CoverageIntervals};

type Reply<T> = Sender<T>;
type MissingIntervalsByIdentifier = AHashMap<String, Vec<(u64, u64)>>;
type CoverageIntervalsByIdentifier = AHashMap<String, CoverageIntervals>;

/// Commands buffered before senders block.
///
/// The queue is bounded so a producer that outruns the catalog backend applies backpressure
/// instead of growing without limit. Both `write_async` and `query_batch_async` block once it is
/// full.
const COMMAND_QUEUE_CAPACITY: usize = 10;

/// Query sessions the worker keeps open at once.
///
/// Each open session holds its backend query state until it is drained or closed, so the count is
/// bounded to turn a caller that never closes sessions into an error instead of an unbounded leak.
pub(crate) const MAX_OPEN_SESSIONS: usize = 64;

#[derive(Debug)]
pub struct CatalogWriteJob {
    pub data: DataBatch,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub params: Option<Params>,
}

pub enum CatalogCommand {
    QueryLastTimestamp {
        data_type: NautilusDataType,
        identifier: Option<String>,
        reply: Reply<anyhow::Result<Option<u64>>>,
    },
    GetMissingIntervals {
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifier: Option<String>,
        reply: Reply<anyhow::Result<Vec<(u64, u64)>>>,
    },
    GetMissingIntervalsForIdentifiers {
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: Vec<String>,
        reply: Reply<anyhow::Result<MissingIntervalsByIdentifier>>,
    },
    GetCoverageIntervalsForIdentifiers {
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: Vec<String>,
        reply: Reply<anyhow::Result<CoverageIntervalsByIdentifier>>,
    },
    QueryBatch {
        query: CatalogQuery,
        reply: Reply<anyhow::Result<DataBatch>>,
    },
    QueryBatchAsync {
        query: CatalogQuery,
        on_complete: Box<dyn FnOnce(anyhow::Result<DataBatch>) + Send>,
    },
    OpenSession {
        query: CatalogQuery,
        chunk_size: Option<usize>,
        reply: Reply<anyhow::Result<UUID4>>,
    },
    PullSession {
        session_id: UUID4,
        reply: Reply<anyhow::Result<Option<DataBatch>>>,
    },
    PullSessionAsync {
        session_id: UUID4,
        on_complete: Box<dyn FnOnce(anyhow::Result<Option<DataBatch>>) + Send>,
    },
    CloseSession {
        session_id: UUID4,
        reply: Reply<anyhow::Result<bool>>,
    },
    QueryInstruments {
        query: CatalogInstrumentQuery,
        reply: Reply<anyhow::Result<Vec<InstrumentAny>>>,
    },
    Write {
        job: CatalogWriteJob,
        reply: Reply<anyhow::Result<()>>,
    },
    WriteAsync {
        job: CatalogWriteJob,
    },
    WriteInstruments {
        instruments: Vec<InstrumentAny>,
        reply: Reply<anyhow::Result<()>>,
    },
    WriteInstrumentsAsync {
        instruments: Vec<InstrumentAny>,
    },
    Flush {
        reply: Reply<anyhow::Result<()>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct CatalogWorker {
    sender: SyncSender<CatalogCommand>,
    handle: Option<JoinHandle<()>>,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "Worker methods forward channel and catalog errors directly"
)]
impl CatalogWorker {
    #[must_use]
    pub fn start(mut catalog: DataCatalogBox) -> Self {
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let handle = thread::spawn(move || run_catalog_worker(&mut catalog, receiver));

        Self {
            sender,
            handle: Some(handle),
        }
    }

    fn send(&self, command: CatalogCommand) -> anyhow::Result<()> {
        self.sender.send(command).map_err(|_| {
            anyhow::anyhow!("Catalog worker thread has stopped, so the command cannot be sent")
        })
    }

    fn send_with_reply<T>(
        &self,
        command: impl FnOnce(Reply<anyhow::Result<T>>) -> CatalogCommand,
    ) -> anyhow::Result<T> {
        let (reply, receiver) = mpsc::channel();
        self.send(command(reply))?;
        receiver.recv().map_err(|_| {
            anyhow::anyhow!("Catalog worker thread stopped before replying to the command")
        })?
    }

    pub fn query_last_timestamp(
        &self,
        data_type: NautilusDataType,
        identifier: Option<String>,
    ) -> anyhow::Result<Option<u64>> {
        self.send_with_reply(|reply| CatalogCommand::QueryLastTimestamp {
            data_type,
            identifier,
            reply,
        })
    }

    pub fn get_missing_intervals(
        &self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifier: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        self.send_with_reply(|reply| CatalogCommand::GetMissingIntervals {
            start,
            end,
            data_type,
            identifier,
            reply,
        })
    }

    pub fn get_missing_intervals_for_identifiers(
        &self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: Vec<String>,
    ) -> anyhow::Result<MissingIntervalsByIdentifier> {
        self.send_with_reply(|reply| CatalogCommand::GetMissingIntervalsForIdentifiers {
            start,
            end,
            data_type,
            identifiers,
            reply,
        })
    }

    pub fn get_coverage_intervals_for_identifiers(
        &self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: Vec<String>,
    ) -> anyhow::Result<CoverageIntervalsByIdentifier> {
        self.send_with_reply(|reply| CatalogCommand::GetCoverageIntervalsForIdentifiers {
            start,
            end,
            data_type,
            identifiers,
            reply,
        })
    }

    pub fn query_batch(&self, query: CatalogQuery) -> anyhow::Result<DataBatch> {
        self.send_with_reply(|reply| CatalogCommand::QueryBatch { query, reply })
    }

    /// Enqueues a batch query and invokes `on_complete` with the result on the
    /// worker thread, without blocking the caller.
    pub fn query_batch_async(
        &self,
        query: CatalogQuery,
        on_complete: Box<dyn FnOnce(anyhow::Result<DataBatch>) + Send>,
    ) -> anyhow::Result<()> {
        self.send(CatalogCommand::QueryBatchAsync { query, on_complete })
    }

    pub fn open_session(
        &self,
        query: CatalogQuery,
        chunk_size: Option<usize>,
    ) -> anyhow::Result<UUID4> {
        self.send_with_reply(|reply| CatalogCommand::OpenSession {
            query,
            chunk_size,
            reply,
        })
    }

    pub fn pull_session(&self, session_id: UUID4) -> anyhow::Result<Option<DataBatch>> {
        self.send_with_reply(|reply| CatalogCommand::PullSession { session_id, reply })
    }

    /// Enqueues one session pull and invokes `on_complete` on the catalog worker.
    pub fn pull_session_async(
        &self,
        session_id: UUID4,
        on_complete: Box<dyn FnOnce(anyhow::Result<Option<DataBatch>>) + Send>,
    ) -> anyhow::Result<()> {
        self.send(CatalogCommand::PullSessionAsync {
            session_id,
            on_complete,
        })
    }

    /// Closes a session, returning whether it was still open.
    pub fn close_session(&self, session_id: UUID4) -> anyhow::Result<bool> {
        self.send_with_reply(|reply| CatalogCommand::CloseSession { session_id, reply })
    }

    pub fn query_instruments(
        &self,
        query: CatalogInstrumentQuery,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.send_with_reply(|reply| CatalogCommand::QueryInstruments { query, reply })
    }

    pub fn write(&self, job: CatalogWriteJob) -> anyhow::Result<()> {
        self.send_with_reply(|reply| CatalogCommand::Write { job, reply })
    }

    pub fn write_async(&self, job: CatalogWriteJob) -> anyhow::Result<()> {
        self.send(CatalogCommand::WriteAsync { job })
    }

    pub fn write_instruments(&self, instruments: Vec<InstrumentAny>) -> anyhow::Result<()> {
        self.send_with_reply(|reply| CatalogCommand::WriteInstruments { instruments, reply })
    }

    pub fn write_instruments_async(&self, instruments: Vec<InstrumentAny>) -> anyhow::Result<()> {
        self.send(CatalogCommand::WriteInstrumentsAsync { instruments })
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        self.send_with_reply(|reply| CatalogCommand::Flush { reply })
    }
}

impl Drop for CatalogWorker {
    fn drop(&mut self) {
        let _ = self.sender.send(CatalogCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    reason = "The worker thread owns the receiver for its whole lifetime"
)]
fn run_catalog_worker(catalog: &mut DataCatalogBox, receiver: Receiver<CatalogCommand>) {
    let mut async_errors: Vec<anyhow::Error> = Vec::new();
    let mut sessions: AHashMap<UUID4, DataBatchQueryResult> = AHashMap::new();

    while let Ok(command) = receiver.recv() {
        match command {
            CatalogCommand::QueryLastTimestamp {
                data_type,
                identifier,
                reply,
            } => {
                let result = catalog.query_last_timestamp(data_type, identifier.as_deref());
                let _ = reply.send(result);
            }
            CatalogCommand::GetMissingIntervals {
                start,
                end,
                data_type,
                identifier,
                reply,
            } => {
                let result = catalog.get_missing_intervals_for_request(
                    start,
                    end,
                    data_type,
                    identifier.as_deref(),
                );
                let _ = reply.send(result);
            }
            CatalogCommand::GetMissingIntervalsForIdentifiers {
                start,
                end,
                data_type,
                identifiers,
                reply,
            } => {
                let result = catalog.get_missing_intervals_for_identifiers(
                    start,
                    end,
                    data_type,
                    &identifiers,
                );
                let _ = reply.send(result);
            }
            CatalogCommand::GetCoverageIntervalsForIdentifiers {
                start,
                end,
                data_type,
                identifiers,
                reply,
            } => {
                let result = catalog.get_coverage_intervals_for_identifiers(
                    start,
                    end,
                    data_type,
                    &identifiers,
                );
                let _ = reply.send(result);
            }
            CatalogCommand::QueryBatch { query, reply } => {
                let result = catalog.query_batch(&query);
                let _ = reply.send(result);
            }
            CatalogCommand::QueryBatchAsync { query, on_complete } => {
                let result = catalog.query_batch(&query);
                on_complete(result);
            }
            CatalogCommand::OpenSession {
                query,
                chunk_size,
                reply,
            } => {
                let result = if sessions.len() >= MAX_OPEN_SESSIONS {
                    Err(anyhow::anyhow!(
                        "Catalog session limit of {MAX_OPEN_SESSIONS} reached; close finished sessions before opening more",
                    ))
                } else {
                    open_query_session(catalog, &query, chunk_size).map(|session| {
                        let session_id = UUID4::new();
                        sessions.insert(session_id, session);
                        session_id
                    })
                };
                let _ = reply.send(result);
            }
            CatalogCommand::PullSession { session_id, reply } => {
                let result = pull_session(&mut sessions, session_id);
                let _ = reply.send(result);
            }
            CatalogCommand::PullSessionAsync {
                session_id,
                on_complete,
            } => {
                on_complete(pull_session(&mut sessions, session_id));
            }
            CatalogCommand::CloseSession { session_id, reply } => {
                let _ = reply.send(Ok(sessions.remove(&session_id).is_some()));
            }
            CatalogCommand::QueryInstruments { query, reply } => {
                let result = catalog.instruments(&query);
                let _ = reply.send(result);
            }
            CatalogCommand::Write { job, reply } => {
                let result = catalog.write_data_batch(&job.data, job.start, job.end, job.params);
                let _ = reply.send(result);
            }
            CatalogCommand::WriteAsync { job } => {
                let len = job.data.len();
                match catalog.write_data_batch(&job.data, job.start, job.end, job.params) {
                    Ok(()) => log::info!("Catalog worker wrote {len} data rows"),
                    Err(e) => {
                        log::error!("Catalog worker failed to write {len} data rows: {e}");
                        async_errors.push(e);
                    }
                }
            }
            CatalogCommand::WriteInstruments { instruments, reply } => {
                let result = catalog.write_instruments(&instruments);
                let _ = reply.send(result);
            }
            CatalogCommand::WriteInstrumentsAsync { instruments } => {
                let len = instruments.len();
                match catalog.write_instruments(&instruments) {
                    Ok(()) => log::info!("Catalog worker wrote {len} instruments"),
                    Err(e) => {
                        log::error!("Catalog worker failed to write {len} instruments: {e}");
                        async_errors.push(e);
                    }
                }
            }
            CatalogCommand::Flush { reply } => {
                let _ = reply.send(drain_async_errors(&mut async_errors));
            }
            CatalogCommand::Shutdown => break,
        }
    }
}

fn open_query_session(
    catalog: &mut DataCatalogBox,
    query: &CatalogQuery,
    chunk_size: Option<usize>,
) -> anyhow::Result<DataBatchQueryResult> {
    if let Some(mut query_catalog) = catalog.fork_query_catalog()? {
        query_catalog.reset_session();
        return query_catalog.query_batch_session(query, chunk_size);
    }

    catalog.reset_session();
    catalog.query_batch_session(query, chunk_size)
}

fn drain_async_errors(errors: &mut Vec<anyhow::Error>) -> anyhow::Result<()> {
    let failures = errors.len();
    let details = errors
        .iter()
        .map(|e| format!("{e:#}"))
        .collect::<Vec<_>>()
        .join("; ");
    let Some(first) = errors.drain(..).next() else {
        return Ok(());
    };

    if failures == 1 {
        return Err(first);
    }

    Err(first.context(format!(
        "{failures} asynchronous catalog writes failed: {details}"
    )))
}

fn pull_session(
    sessions: &mut AHashMap<UUID4, DataBatchQueryResult>,
    session_id: UUID4,
) -> anyhow::Result<Option<DataBatch>> {
    let result = sessions
        .get_mut(&session_id)
        .ok_or_else(|| anyhow::anyhow!("Catalog session {session_id} is not open"))?
        .next_batch();

    if !matches!(result, Ok(Some(_))) {
        sessions.remove(&session_id);
    }
    result
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use nautilus_model::{
        data::{Data, NautilusRecordType, QuoteTick},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::catalog::{
        session::{DataBatchQuery, TypedDataBatchSession},
        traits::{CatalogMetadata, CatalogReader, CatalogRecordQuery, CatalogWriter, RecordBatch},
    };

    struct FailingSession;

    impl DataBatchQuery for FailingSession {
        fn next_batch(&mut self) -> anyhow::Result<Option<DataBatch>> {
            anyhow::bail!("session decode failure")
        }
    }

    fn stub_quote() -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("1987.0"),
            Price::from("1988.0"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        )
    }

    #[derive(Debug)]
    struct StubCatalog {
        fail: bool,
        query_thread: Arc<Mutex<Option<thread::ThreadId>>>,
    }

    impl CatalogReader for StubCatalog {
        fn reset_session(&mut self) {}

        fn instruments(
            &mut self,
            _query: &CatalogInstrumentQuery,
        ) -> anyhow::Result<Vec<InstrumentAny>> {
            Ok(Vec::new())
        }

        fn query_batch(&mut self, _query: &CatalogQuery) -> anyhow::Result<DataBatch> {
            *self.query_thread.lock().unwrap() = Some(thread::current().id());

            if self.fail {
                anyhow::bail!("stub query failure");
            }
            Ok(DataBatch::Quote(vec![stub_quote()].into()))
        }

        fn query_batch_session(
            &mut self,
            _query: &CatalogQuery,
            chunk_size: Option<usize>,
        ) -> anyhow::Result<DataBatchQueryResult> {
            *self.query_thread.lock().unwrap() = Some(thread::current().id());

            if self.fail {
                anyhow::bail!("stub query failure");
            }
            Ok(Box::new(TypedDataBatchSession::from_vec(
                vec![stub_quote()],
                chunk_size,
            )))
        }

        fn query_metadata(
            &mut self,
            _query: &CatalogQuery,
        ) -> anyhow::Result<Vec<CatalogMetadata>> {
            Ok(Vec::new())
        }

        fn get_missing_intervals_for_request(
            &mut self,
            _start: UnixNanos,
            _end: UnixNanos,
            _data_type: NautilusDataType,
            _identifier: Option<&str>,
        ) -> anyhow::Result<Vec<(u64, u64)>> {
            Ok(Vec::new())
        }

        fn query_last_timestamp(
            &mut self,
            _data_type: NautilusDataType,
            _identifier: Option<&str>,
        ) -> anyhow::Result<Option<u64>> {
            Ok(None)
        }

        fn query_display_record_batches(
            &mut self,
            _query: &CatalogQuery,
        ) -> anyhow::Result<Vec<RecordBatch>> {
            Ok(Vec::new())
        }

        fn query_record_batches(
            &mut self,
            _query: &CatalogRecordQuery,
        ) -> anyhow::Result<Vec<RecordBatch>> {
            Ok(Vec::new())
        }

        fn query_record_display_batches(
            &mut self,
            _query: &CatalogRecordQuery,
        ) -> anyhow::Result<Vec<RecordBatch>> {
            Ok(Vec::new())
        }
    }

    impl CatalogWriter for StubCatalog {
        fn write_instruments(&mut self, _instruments: &[InstrumentAny]) -> anyhow::Result<()> {
            Ok(())
        }

        fn write_data(
            &mut self,
            _data: &[Data],
            _start: Option<UnixNanos>,
            _end: Option<UnixNanos>,
            params: Option<Params>,
        ) -> anyhow::Result<()> {
            if self.fail {
                let message = params
                    .as_ref()
                    .and_then(|params| params.get_str("test_error"))
                    .unwrap_or("stub write failure");
                anyhow::bail!(message.to_string());
            }
            Ok(())
        }

        fn write_records(
            &mut self,
            _record_type: NautilusRecordType,
            _batches: &[RecordBatch],
            _params: Option<Params>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn record_empty_coverage(
            &mut self,
            _data_type: NautilusDataType,
            _identifier: Option<&str>,
            _start: UnixNanos,
            _end: UnixNanos,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn stub_query() -> CatalogQuery {
        CatalogQuery::new(NautilusDataType::QuoteTick)
    }

    #[rstest]
    fn test_query_batch_async_invokes_callback_on_worker_thread() {
        let query_thread = Arc::new(Mutex::new(None));
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: false,
            query_thread: query_thread.clone(),
        }));

        let (tx, rx) = mpsc::channel();
        worker
            .query_batch_async(
                stub_query(),
                Box::new(move |result| {
                    let _ = tx.send((thread::current().id(), result.map(|batch| batch.len())));
                }),
            )
            .unwrap();

        let (callback_thread, result) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("callback was not invoked");
        assert_eq!(result.unwrap(), 1);
        assert_ne!(callback_thread, thread::current().id());
        assert_eq!(Some(callback_thread), *query_thread.lock().unwrap());
    }

    #[rstest]
    fn test_query_batch_async_passes_query_error_to_callback() {
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: true,
            query_thread: Arc::new(Mutex::new(None)),
        }));

        let (tx, rx) = mpsc::channel();
        worker
            .query_batch_async(
                stub_query(),
                Box::new(move |result| {
                    let _ = tx.send(result.map(|batch| batch.len()));
                }),
            )
            .unwrap();

        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("callback was not invoked");
        assert_eq!(result.unwrap_err().to_string(), "stub query failure");
    }

    #[rstest]
    fn test_worker_owns_and_pulls_catalog_session() {
        let query_thread = Arc::new(Mutex::new(None));
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: false,
            query_thread: query_thread.clone(),
        }));

        let session_id = worker.open_session(stub_query(), Some(1)).unwrap();
        let batch = worker.pull_session(session_id).unwrap().unwrap();
        let complete = worker.pull_session(session_id).unwrap();
        let closed = worker.pull_session(session_id).unwrap_err();

        assert_eq!(batch.len(), 1);
        assert!(complete.is_none());
        assert_eq!(
            closed.to_string(),
            format!("Catalog session {session_id} is not open")
        );
        assert_ne!(*query_thread.lock().unwrap(), Some(thread::current().id()));
    }

    #[rstest]
    fn test_close_session_drops_open_session() {
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: false,
            query_thread: Arc::new(Mutex::new(None)),
        }));
        let session_id = worker.open_session(stub_query(), Some(1)).unwrap();

        let first_close = worker.close_session(session_id).unwrap();
        let second_close = worker.close_session(session_id).unwrap();

        assert!(first_close);
        assert!(!second_close);
    }

    #[rstest]
    fn errored_session_is_removed_after_pull() {
        let session_id = UUID4::new();
        let mut sessions = AHashMap::new();
        sessions.insert(session_id, Box::new(FailingSession) as DataBatchQueryResult);

        let first = pull_session(&mut sessions, session_id).unwrap_err();
        let second = pull_session(&mut sessions, session_id).unwrap_err();

        assert_eq!(first.to_string(), "session decode failure");
        assert_eq!(
            second.to_string(),
            format!("Catalog session {session_id} is not open"),
        );
        assert!(sessions.is_empty());
    }

    #[rstest]
    fn test_flush_reports_every_failed_async_write_then_clears() {
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: true,
            query_thread: Arc::new(Mutex::new(None)),
        }));

        for message in [
            "first write failed",
            "second write failed",
            "third write failed",
        ] {
            let mut params = Params::new();
            params.insert("test_error".to_string(), message.into());
            worker
                .write_async(CatalogWriteJob {
                    data: DataBatch::Quote(vec![stub_quote()].into()),
                    start: None,
                    end: None,
                    params: Some(params),
                })
                .unwrap();
        }

        let error = worker.flush().unwrap_err();

        assert_eq!(
            error.to_string(),
            "3 asynchronous catalog writes failed: first write failed; second write failed; \
             third write failed",
        );
        assert_eq!(error.root_cause().to_string(), "first write failed");
        assert!(worker.flush().is_ok(), "flush must clear reported failures");
    }

    #[rstest]
    fn test_open_session_rejects_beyond_the_session_limit() {
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: false,
            query_thread: Arc::new(Mutex::new(None)),
        }));
        let session_ids = (0..MAX_OPEN_SESSIONS)
            .map(|_| worker.open_session(stub_query(), Some(1)).unwrap())
            .collect::<Vec<_>>();

        let error = worker.open_session(stub_query(), Some(1)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "Catalog session limit of {MAX_OPEN_SESSIONS} reached; close finished sessions before opening more"
            ),
        );

        assert!(worker.close_session(session_ids[0]).unwrap());
        assert!(
            worker.open_session(stub_query(), Some(1)).is_ok(),
            "closing a session must free a slot",
        );
    }

    #[rstest]
    fn test_pull_session_async_invokes_callback_on_worker_thread() {
        let worker = CatalogWorker::start(Box::new(StubCatalog {
            fail: false,
            query_thread: Arc::new(Mutex::new(None)),
        }));
        let session_id = worker.open_session(stub_query(), Some(1)).unwrap();
        let (tx, rx) = mpsc::channel();

        worker
            .pull_session_async(
                session_id,
                Box::new(move |result| {
                    let _ = tx.send((
                        thread::current().id(),
                        result.map(|batch| batch.unwrap().len()),
                    ));
                }),
            )
            .unwrap();
        let (callback_thread, result) = rx.recv_timeout(Duration::from_secs(5)).unwrap();

        assert_eq!(result.unwrap(), 1);
        assert_ne!(callback_thread, thread::current().id());
    }
}
