use std::collections::HashMap;
use std::vec::IntoIter;

use std::ops::Deref;

use datafusion::error::Result;
use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use futures::executor::{block_on, block_on_stream, BlockingStream};
use futures::{Stream, StreamExt};
use nautilus_core::cvec::CVec;
use nautilus_core::time::UnixNanos;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use pyo3::prelude::*;
use pyo3::types::PyCapsule;
use pyo3_asyncio::tokio::get_runtime;

use crate::kmerge_batch::{DataTsInit, KMerge, TsInitComparator};
use crate::parquet::{DecodeFromRecordBatch, ParquetType};

#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
enum Data {
    Trade(TradeTick),
    Quote(QuoteTick),
}

impl DataTsInit for Data {
    fn get_ts_init(&self) -> UnixNanos {
        match self {
            Data::Trade(t) => t.get_ts_init(),
            Data::Quote(q) => q.get_ts_init(),
        }
    }
}

impl From<QuoteTick> for Data {
    fn from(value: QuoteTick) -> Self {
        Self::Quote(value)
    }
}

impl From<TradeTick> for Data {
    fn from(value: TradeTick) -> Self {
        Self::Trade(value)
    }
}

pub struct PersistenceCatalog<T> {
    session_ctx: SessionContext,
    batch_streams: Vec<Box<dyn Stream<Item = IntoIter<T>> + Unpin>>,
}

impl<T> Default for PersistenceCatalog<T> {
    fn default() -> Self {
        Self {
            session_ctx: Default::default(),
            batch_streams: Default::default(),
        }
    }
}

impl<T> PersistenceCatalog<T>
where
    T: DecodeFromRecordBatch + 'static,
{
    // query a file for all it's records
    pub async fn add_file(&mut self, table_name: &str, file_path: &str) -> Result<()> {
        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            ..Default::default()
        };
        self.session_ctx
            .register_parquet(table_name, file_path, parquet_options)
            .await?;

        let batch_stream = self
            .session_ctx
            .sql(&format!("SELECT * FROM {} ORDER BY ts_init", &table_name))
            .await?
            .execute_stream()
            .await?;

        self.add_batch_stream(batch_stream);
        Ok(())
    }

    // query a file for all it's records with a custom query
    // The query should ensure the records are ordered by the
    // ts_init field in ascending order
    pub async fn add_file_with_query(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
    ) -> Result<()> {
        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            ..Default::default()
        };
        self.session_ctx
            .register_parquet(table_name, file_path, parquet_options)
            .await?;

        let batch_stream = self
            .session_ctx
            .sql(sql_query)
            .await?
            .execute_stream()
            .await?;

        self.add_batch_stream(batch_stream);
        Ok(())
    }

    fn add_batch_stream(&mut self, stream: SendableRecordBatchStream) {
        let transform = stream.map(|result| match result {
            Ok(batch) => T::decode_batch(batch.schema().metadata(), batch).into_iter(),
            Err(_err) => panic!("Error getting next batch from RecordBatchStream"),
        });

        self.batch_streams.push(Box::new(transform));
    }

    pub fn to_query_result(&mut self) -> QueryResult<T>
    where
        T: DataTsInit,
    {
        // TODO: No need to kmerge if there is only one batch stream
        let mut kmerge: KMerge<_, _> = KMerge::new(TsInitComparator);

        Iterator::for_each(self.batch_streams.drain(..), |batch_stream| {
            block_on(kmerge.push_stream(batch_stream));
        });

        QueryResult {
            data: Box::new(kmerge.chunks(1000)),
        }
    }
}

pub struct QueryResult<T> {
    data: Box<dyn Stream<Item = Vec<T>> + Unpin>,
}

impl<T> Iterator for QueryResult<T> {
    type Item = Vec<T>;

    fn next(&mut self) -> Option<Self::Item> {
        block_on(self.data.next())
    }
}

/// Store the data fusion session context
#[pyclass]
#[derive(Default)]
pub struct PersistenceSession {
    session_ctx: SessionContext,
    query_result: Option<PersistenceQuery>,
}

/// Store the result stream created by executing a query
///
/// The async stream has been wrapped into a blocking stream. The nautilus
/// engine is a CPU intensive process so it will process the events in one
/// batch and then request more. We want to block the thread until it
/// receives more events to consume.
pub struct PersistenceQuery {
    result: BlockingStream<SendableRecordBatchStream>,
    metadata: HashMap<String, String>,
    parquet_type: ParquetType,
    current_chunk: Option<CVec>,
}

impl Deref for PersistenceSession {
    type Target = SessionContext;

    fn deref(&self) -> &Self::Target {
        &self.session_ctx
    }
}

impl PersistenceSession {
    pub fn new() -> Self {
        let session_ctx = SessionContext::new();
        PersistenceSession {
            session_ctx,
            query_result: None,
        }
    }

    /// Takes an sql query and creates a data frame
    ///
    /// The data frame is the logical plan that can be executed on the
    /// data sources registered with the context. The async stream
    /// is wrapped into a blocking stream.
    pub async fn query(&self, sql: &str) -> Result<BlockingStream<SendableRecordBatchStream>> {
        let df = self.sql(sql).await?;
        let stream = df.execute_stream().await?;
        Ok(block_on_stream(stream))
    }
}

/// Persistence session methods exposed to Python
///
/// session_ctx has all the methods needed to manipulate the session
/// context. However we expose only limited or relevant  methods
/// through python.
///
/// Creating a session also initialized a tokio runtime so that
/// the query solver can use the runtime. This can be moved to
/// a different entry point later.
#[pymethods]
impl PersistenceSession {
    #[new]
    pub fn new_session() -> Self {
        // initialize runtime here
        get_runtime();
        Self::new()
    }

    pub fn new_query(
        mut slf: PyRefMut<'_, Self>,
        sql: String,
        metadata: HashMap<String, String>,
        parquet_type: ParquetType,
    ) {
        let rt = get_runtime();
        let _guard = rt.enter();

        match block_on(slf.query(&sql)) {
            Ok(result) => {
                let query = PersistenceQuery {
                    result,
                    metadata,
                    parquet_type,
                    current_chunk: None,
                };
                slf.query_result = Some(query);
            }
            Err(err) => panic!("failed new_query with error {}", err),
        }
    }

    pub fn register_parquet_file(slf: PyRef<'_, Self>, table_name: String, path: String) {
        match block_on(slf.register_parquet(&table_name, &path, ParquetReadOptions::default())) {
            Ok(_) => (),
            Err(err) => panic!("failed register_parquet_file with error {}", err),
        }
    }

    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        let rt = get_runtime();
        let _guard = rt.enter();

        let query_result = slf
            .query_result
            .as_mut()
            .expect("Session needs a query to iterate");
        query_result.drop_chunk();

        let chunk: Option<CVec> = match query_result.parquet_type {
            ParquetType::QuoteTick => {
                if let Some(Ok(batch)) = query_result.result.next() {
                    Some(QuoteTick::decode_batch(&query_result.metadata, batch).into())
                } else {
                    None
                }
            }
            // TODO implement decode batch for trade tick
            ParquetType::TradeTick => None,
        };

        // Leak reader value back otherwise it will be dropped after this function
        query_result.current_chunk = chunk;
        match chunk {
            Some(cvec) => Python::with_gil(|py| {
                Some(PyCapsule::new::<CVec>(py, cvec, None).unwrap().into_py(py))
            }),
            None => None,
        }
    }
}

impl PersistenceQuery {
    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory. Current chunk is held by the reader,
    /// drop if exists and reset the field.
    fn drop_chunk(&mut self) {
        if let Some(CVec { ptr, len, cap }) = self.current_chunk {
            match self.parquet_type {
                ParquetType::QuoteTick => {
                    let data: Vec<QuoteTick> =
                        unsafe { Vec::from_raw_parts(ptr as *mut QuoteTick, len, cap) };
                    drop(data);
                }
                ParquetType::TradeTick => {
                    let data: Vec<TradeTick> =
                        unsafe { Vec::from_raw_parts(ptr as *mut TradeTick, len, cap) };
                    drop(data);
                }
            }

            // reset current chunk field
            self.current_chunk = None;
        };
    }
}

impl Drop for PersistenceQuery {
    fn drop(&mut self) {
        self.drop_chunk();
    }
}
