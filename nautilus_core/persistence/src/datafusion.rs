use std::ops::Deref;

use datafusion::physical_plan::SendableRecordBatchStream;
use datafusion::prelude::*;
use datafusion::{arrow::record_batch::RecordBatch, error::Result};
use futures::executor::{block_on_stream, BlockingStream};

/// Store the data fusion session context
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
    pub async fn new() -> Result<Self> {
        // create local session context
        let session_ctx = SessionContext::new();
        Ok(Self { session_ctx })
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
