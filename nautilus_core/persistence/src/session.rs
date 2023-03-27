use std::vec::IntoIter;

use datafusion::error::Result;
use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use futures::executor::block_on;
use futures::{Stream, StreamExt};
use nautilus_core::cvec::CVec;
use nautilus_core::time::UnixNanos;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use pyo3::prelude::*;
use pyo3::types::PyCapsule;
use pyo3_asyncio::tokio::get_runtime;

use crate::kmerge_batch::{DataTsInit, KMerge, TsInitComparator};
use crate::parquet::DecodeFromRecordBatch;

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
pub struct PythonCatalog(PersistenceCatalog<QuoteTick>);

// Note: Intended to be used on a single python thread
unsafe impl<T> Send for PersistenceCatalog<T> {}

#[pymethods]
impl PythonCatalog {
    #[new]
    pub fn new_session() -> Self {
        // initialize runtime here
        get_runtime();
        Self::default()
    }

    pub fn add_file(mut slf: PyRefMut<'_, Self>, table_name: &str, file_path: &str) {
        let rt = get_runtime();
        let _guard = rt.enter();

        match block_on(slf.0.add_file(table_name, file_path)) {
            Ok(_) => (),
            Err(err) => panic!("failed new_query with error {}", err),
        }
    }

    pub fn add_file_with_query(
        mut slf: PyRefMut<'_, Self>,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
    ) {
        let rt = get_runtime();
        let _guard = rt.enter();

        match block_on(slf.0.add_file_with_query(table_name, file_path, sql_query)) {
            Ok(_) => (),
            Err(err) => panic!("failed new_query with error {}", err),
        }
    }

    pub fn to_query_result(mut slf: PyRefMut<'_, Self>) -> PythonQueryResult {
        let rt = get_runtime();
        let _guard = rt.enter();

        let query_result = slf.0.to_query_result();
        PythonQueryResult::new(query_result)
    }
}

#[pyclass]
pub struct PythonQueryResult {
    result: QueryResult<QuoteTick>,
    chunk: Option<CVec>,
}

#[pymethods]
impl PythonQueryResult {
    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.drop_chunk();

        let rt = get_runtime();
        let _guard = rt.enter();

        slf.result.next().map(|chunk| {
            let cvec = chunk.into();
            Python::with_gil(|py| PyCapsule::new::<CVec>(py, cvec, None).unwrap().into_py(py))
        })
    }
}

// Note: Intended to be used on a single python thread
unsafe impl Send for PythonQueryResult {}

impl PythonQueryResult {
    fn new(result: QueryResult<QuoteTick>) -> Self {
        Self {
            result,
            chunk: None,
        }
    }
    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory. Current chunk is held by the reader,
    /// drop if exists and reset the field.
    fn drop_chunk(&mut self) {
        if let Some(CVec { ptr, len, cap }) = self.chunk.take() {
            let data: Vec<QuoteTick> =
                unsafe { Vec::from_raw_parts(ptr as *mut QuoteTick, len, cap) };
            drop(data);
        }
    }
}

impl Drop for PythonQueryResult {
    fn drop(&mut self) {
        self.drop_chunk();
    }
}
