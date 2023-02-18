use std::collections::HashMap;

use std::ops::Deref;

use datafusion::error::Result;
use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use futures::executor::{block_on, block_on_stream, BlockingStream};
use nautilus_core::cvec::CVec;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use pyo3::prelude::*;
use pyo3::types::PyCapsule;
use pyo3_asyncio::tokio::re_exports::runtime::Runtime;

use crate::parquet::{DecodeFromRecordBatch, ParquetType};

/// Store the data fusion session context
#[pyclass]
#[derive(Default)]
pub struct PersistenceSession {
    session_ctx: SessionContext,
    runtime: Option<Runtime>,
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
    /// Create a new data fusion session with a runtime
    ///
    /// This is mainly when using the persistence session in Python.
    /// As it has to initialize it's own tokio runtime
    pub fn new_with_runtime() -> Self {
        let runtime = Runtime::new().expect("Unable to initialize tokio runtime in new session");
        let session_ctx = SessionContext::new();
        PersistenceSession {
            session_ctx,
            runtime: Some(runtime),
            query_result: None,
        }
    }

    pub fn new() -> Self {
        let session_ctx = SessionContext::new();
        PersistenceSession {
            session_ctx,
            runtime: None,
            query_result: None,
        }
    }

    /// Takes an sql query and creates a data frame
    ///
    /// The data frame is the logical plan that can be executed on the
    /// data sources registered with the context. The async stream
    /// is wrapped into a blocking stream.
    pub async fn query(&self, sql: &str) -> Result<BlockingStream<SendableRecordBatchStream>> {
        match self.runtime {
            // Use own runtime if it exists
            Some(ref rt) => {
                let df = rt.block_on(self.sql(sql))?;
                let stream = rt.block_on(df.execute_stream())?;
                Ok(block_on_stream(stream))
            }
            None => {
                let df = self.sql(sql).await?;
                let stream = df.execute_stream().await?;
                Ok(block_on_stream(stream))
            }
        }
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
        Self::new()
    }

    pub fn new_query(
        mut slf: PyRefMut<'_, Self>,
        sql: String,
        metadata: HashMap<String, String>,
        parquet_type: ParquetType,
    ) {
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
