use std::collections::HashMap;
use std::ffi::c_void;
use std::ops::Deref;
use std::ptr::null_mut;

use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use datafusion::{arrow::record_batch::RecordBatch, error::Result};
use futures::executor::{block_on, block_on_stream, BlockingStream};
use nautilus_core::cvec::CVec;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use pyo3::prelude::*;
use pyo3::types::PyCapsule;

use crate::parquet::{DecodeFromRecordBatch, ParquetType};

/// Store the data fusion session context
#[pyclass]
pub struct PersistenceSession {
    session_ctx: SessionContext,
}

impl Deref for PersistenceSession {
    type Target = SessionContext;

    fn deref(&self) -> &Self::Target {
        &self.session_ctx
    }
}

/// Store the result stream created by executing a query
///
/// The async stream has been wrapped into a blocking stream. The nautilus
/// engine is a CPU intensive process so it will process the events in one
/// batch and then request more. We want to block the thread until it
/// receives more events to consume.
#[pyclass]
pub struct PersistenceQueryResult(pub BlockingStream<SendableRecordBatchStream>);

impl Iterator for PersistenceQueryResult {
    type Item = RecordBatch;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(result) = self.0.next() {
            match result {
                Ok(batch) => Some(batch),
                // TODO log or handle error here
                Err(_) => None,
            }
        } else {
            None
        }
    }
}

impl PersistenceSession {
    /// Create a new data fusion session
    ///
    /// This can register new files and data sources
    pub fn new() -> Self {
        PersistenceSession {
            session_ctx: SessionContext::new(),
        }
    }

    /// Takes an sql query and creates a data frame
    ///
    /// The data frame is the logical plan that can be executed on the
    /// data sources registered with the context. The async stream
    /// is wrapped into a blocking stream.
    pub async fn query(&self, sql: &str) -> Result<PersistenceQueryResult> {
        let df = self.sql(sql).await?;
        let stream = df.execute_stream().await?;
        Ok(PersistenceQueryResult(block_on_stream(stream)))
    }
}

/// Persistence session methods exposed to Python
///
/// session_ctx has all the methods needed to manipulate the session
/// context. However we expose only limited or relevant  methods
/// through python.
#[pymethods]
impl PersistenceSession {
    #[new]
    pub fn new_session() -> Self {
        Self::new()
    }

    pub fn new_query(
        slf: PyRef<'_, Self>,
        sql: String,
        metadata: HashMap<String, String>,
        parquet_type: ParquetType,
    ) -> PersistenceQuery {
        match block_on(slf.query(&sql)) {
            Ok(query_result) => {
                let boxed =
                    Box::leak(Box::new(query_result)) as *mut PersistenceQueryResult as *mut c_void;
                PersistenceQuery {
                    query_result: boxed,
                    metadata,
                    parquet_type,
                    current_chunk: None,
                }
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
}

#[pyclass]
pub struct PersistenceQuery {
    query_result: *mut c_void,
    metadata: HashMap<String, String>,
    parquet_type: ParquetType,
    current_chunk: Option<CVec>,
}

/// Empty derivation for Send to satisfy `pyclass` requirements,
/// however this is only designed for single threaded use for now.
unsafe impl Send for PersistenceQuery {}

#[pymethods]
impl PersistenceQuery {
    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.drop_chunk();
        let mut query_result =
            unsafe { Box::from_raw(slf.query_result as *mut PersistenceQueryResult) };

        let chunk: Option<CVec> = match slf.parquet_type {
            ParquetType::QuoteTick => {
                if let Some(batch) = query_result.next() {
                    Some(QuoteTick::decode_batch(&slf.metadata, batch).into())
                } else {
                    None
                }
            }
            // TODO implement decode batch for trade tick
            ParquetType::TradeTick => None,
        };

        // Leak reader value back otherwise it will be dropped after this function
        Box::into_raw(query_result);
        slf.current_chunk = chunk;
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
        let query_result = unsafe { Box::from_raw(self.query_result as *mut PersistenceQuery) };
        self.query_result = null_mut();
    }
}
