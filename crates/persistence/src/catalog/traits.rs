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

//! Backend-neutral catalog trait declarations for object-safe runtime catalog APIs.

use std::{borrow::Cow, fmt::Debug};

use ahash::AHashMap;
pub use arrow::record_batch::RecordBatch;
use nautilus_core::{ClosedInterval, Params, UnixNanos};
use nautilus_model::{
    data::{Data, DataBatch, NautilusDataType, NautilusRecordType},
    instruments::InstrumentAny,
};

pub use super::types::{
    CatalogAsOf, CatalogCommit, CatalogInstrumentQuery, CatalogQuery, CatalogRecordQuery,
};
pub(crate) use super::types::{
    filter_instrument_query_result, filter_instruments_for_request_range,
};
use crate::{
    catalog::session::DataBatchQueryResult,
    common::coverage::{CoverageIntervals, missing_intervals},
    errors::PersistenceError,
};

/// Boxed runtime catalog backend.
pub type DataCatalogBox = Box<dyn DataCatalog>;

/// Builds the error a backend returns for a catalog capability it does not implement.
///
/// Callers distinguish these from genuine failures by downcasting to
/// [`PersistenceError::Unsupported`].
fn unsupported(operation: &str) -> anyhow::Error {
    anyhow::Error::from(PersistenceError::unsupported(operation))
}

/// Arrow schema metadata and the first queried timestamp where it is used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogMetadata {
    pub first_ts_init: UnixNanos,
    pub metadata: Params,
}

/// Persistence-specific paths for [`NautilusDataType`].
pub trait NautilusDataTypePrefix {
    /// Returns the catalog path prefix for this data type.
    ///
    /// For built-in variants the prefix is borrowed from the type's `CatalogPathPrefix`
    /// implementation. For [`NautilusDataType::Custom`] the prefix is owned because it
    /// embeds the user-supplied `type_name`.
    #[must_use]
    fn path_prefix(&self) -> Cow<'static, str>;
}

/// Persistence-specific paths for [`NautilusRecordType`].
pub trait NautilusRecordTypePrefix {
    /// Returns catalog path prefix for record type.
    #[must_use]
    fn path_prefix(&self) -> Cow<'static, str>;
}

/// Runtime catalog read API used by data loading code.
///
/// The methods are object-safe so callers in other crates can accept a catalog backend without
/// depending on a concrete table format.
/// Implementations must preserve the typed logical schema of built-in data, records, and
/// instruments. Only user-defined [`Data::Custom`] values may use an opaque, self-describing
/// payload.
pub trait CatalogReader: Debug + Send {
    /// Creates an independent catalog for a lazy query while sharing backend resources.
    ///
    /// Backends with mutable per-query state should return a catalog with isolated session state.
    /// The default keeps external catalogs on the existing one-instance-per-query path.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot create the query catalog.
    fn fork_query_catalog(&self) -> anyhow::Result<Option<DataCatalogBox>> {
        Ok(None)
    }

    /// Resets any per-query session state.
    fn reset_session(&mut self);

    /// Queries instruments known by the catalog.
    ///
    /// Applies `where_clause` as an additional SQL predicate when supported by the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot query instruments.
    fn instruments(&mut self, query: &CatalogInstrumentQuery)
    -> anyhow::Result<Vec<InstrumentAny>>;

    /// Queries catalog data as a typed batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend query fails.
    fn query_batch(&mut self, query: &CatalogQuery) -> anyhow::Result<DataBatch>;

    /// Queries catalog data as a typed batch session.
    ///
    /// `chunk_size` configures this query session only. `None` uses the default streaming chunk size;
    /// `Some(n)` yields timestamp-aligned typed batches no smaller than `n` when same-ts rows cross
    /// the boundary.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend query fails.
    fn query_batch_session(
        &mut self,
        query: &CatalogQuery,
        chunk_size: Option<usize>,
    ) -> anyhow::Result<DataBatchQueryResult>;

    /// Queries the concrete catalog row identifiers matched by a data query.
    ///
    /// Implementations should use backend-native projection (for example, DataFusion
    /// `SELECT DISTINCT identifier`) so callers can discover identifiers without
    /// materializing full market data rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend identifier query fails, or if the backend
    /// does not override this default implementation.
    fn query_identifiers(&mut self, _query: &CatalogQuery) -> anyhow::Result<Vec<String>> {
        Err(unsupported("query_identifiers"))
    }

    /// Queries Arrow schema metadata and the first queried timestamp where each metadata is used.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend query or metadata lookup fails.
    fn query_metadata(&mut self, _query: &CatalogQuery) -> anyhow::Result<Vec<CatalogMetadata>> {
        Err(unsupported("query_metadata"))
    }

    /// Returns request intervals not covered by catalog data or known-empty coverage.
    ///
    /// # Errors
    ///
    /// Returns an error if interval discovery fails.
    fn get_missing_intervals_for_request(
        &mut self,
        _start: UnixNanos,
        _end: UnixNanos,
        _data_type: NautilusDataType,
        _identifier: Option<&str>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        Err(unsupported("get_missing_intervals_for_request"))
    }

    /// Returns missing request intervals for each identifier.
    ///
    /// Backends should override this method when all identifiers can share one coverage scan.
    ///
    /// # Errors
    ///
    /// Returns an error if interval discovery fails.
    fn get_missing_intervals_for_identifiers(
        &mut self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: &[String],
    ) -> anyhow::Result<AHashMap<String, Vec<(u64, u64)>>> {
        identifiers
            .iter()
            .map(|identifier| {
                self.get_missing_intervals_for_request(
                    start,
                    end,
                    data_type.clone(),
                    Some(identifier),
                )
                .map(|missing| (identifier.clone(), missing))
            })
            .collect()
    }

    /// Returns effective data and known-empty coverage for each identifier.
    ///
    /// Backends should override this method when all identifiers can share one coverage scan.
    ///
    /// # Errors
    ///
    /// Returns an error if interval discovery fails.
    fn get_coverage_intervals_for_identifiers(
        &mut self,
        start: UnixNanos,
        end: UnixNanos,
        data_type: NautilusDataType,
        identifiers: &[String],
    ) -> anyhow::Result<AHashMap<String, CoverageIntervals>> {
        self.get_missing_intervals_for_identifiers(start, end, data_type, identifiers)?
            .into_iter()
            .map(|(identifier, missing)| {
                let missing = missing
                    .into_iter()
                    .filter_map(|(start, end)| ClosedInterval::new(start, end))
                    .collect::<Vec<_>>();
                let data = missing_intervals(start.as_u64(), end.as_u64(), &missing);
                Ok((
                    identifier,
                    CoverageIntervals {
                        data,
                        empty: Vec::new(),
                    },
                ))
            })
            .collect()
    }

    /// Returns the last timestamp covered by the catalog for a data type and optional identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if backend coverage discovery fails.
    fn query_last_timestamp(
        &mut self,
        _data_type: NautilusDataType,
        _identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        Err(unsupported("query_last_timestamp"))
    }

    /// Queries catalog data as display-friendly Arrow record batches.
    ///
    /// Display conversion normalizes fixed-point prices and quantities to floating-point columns
    /// and preserves catalog query semantics for the concrete backend. Implementations should
    /// accept multiple identifiers in one call; shared-table backends can use a single
    /// multi-identifier predicate, while file-oriented backends can concatenate matching results.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend query or display conversion fails.
    fn query_display_record_batches(
        &mut self,
        _query: &CatalogQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        Err(unsupported("query_display_record_batches"))
    }

    /// Queries catalog records as raw Arrow record batches.
    ///
    /// This supports record types outside the [`Data`] enum, such as account state,
    /// order/position events, snapshots, reports, and instruments.
    /// Backends must preserve the fixed schema selected by [`NautilusRecordType`] rather than
    /// returning an opaque serialized payload.
    ///
    /// # Errors
    ///
    /// Returns an error if backend query execution fails.
    fn query_record_batches(
        &mut self,
        _query: &CatalogRecordQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        Err(unsupported("query_record_batches"))
    }

    /// Queries catalog records as display-friendly Arrow record batches.
    ///
    /// Implementations may return raw batches for record types with no specialized
    /// display conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if backend query or display conversion fails.
    fn query_record_display_batches(
        &mut self,
        query: &CatalogRecordQuery,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        self.query_record_batches(query)
    }
}

/// Runtime catalog mutation API.
///
/// Implementations must preserve the typed logical schema of built-in data, records, and
/// instruments. Only user-defined [`Data::Custom`] values may use an opaque, self-describing
/// payload.
pub trait CatalogWriter: Debug + Send {
    /// Writes instrument definitions into the catalog.
    ///
    /// Backends must preserve typed instrument fields rather than storing an opaque serialized
    /// instrument payload.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend write fails.
    fn write_instruments(&mut self, instruments: &[InstrumentAny]) -> anyhow::Result<()>;

    /// Writes mixed built-in data values into the catalog.
    ///
    /// Pass a non-empty `data` vec. To record coverage for a known-empty interval,
    /// use [`Self::record_empty_coverage`] instead.
    /// Backends must preserve the fixed schema of built-in variants. An opaque, self-describing
    /// payload is permitted only for user-defined [`Data::Custom`] values.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend write fails.
    fn write_data(
        &mut self,
        data: &[Data],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
    ) -> anyhow::Result<()>;

    /// Writes a typed data batch into the catalog.
    ///
    /// Implementations can override this to avoid materializing compatibility [`Data`] rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend write fails.
    fn write_data_batch(
        &mut self,
        batch: &DataBatch,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let data = batch.to_data_vec_for_compat();
        self.write_data(&data, start, end, params)
    }

    /// Writes Arrow record batches for a record family into the catalog.
    ///
    /// Backends must preserve the fixed schema selected by [`NautilusRecordType`] rather than
    /// storing an opaque serialized payload.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot persist the record batches.
    fn write_records(
        &mut self,
        record_type: NautilusRecordType,
        batches: &[RecordBatch],
        params: Option<Params>,
    ) -> anyhow::Result<()>;

    /// Records request coverage for a known-empty interval.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot record empty coverage.
    fn record_empty_coverage(
        &mut self,
        data_type: NautilusDataType,
        identifier: Option<&str>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()>;
}

/// Full read-write catalog capability used by factories and catalog workers.
pub trait DataCatalog: CatalogReader + CatalogWriter {}

impl<T> DataCatalog for T where T: CatalogReader + CatalogWriter + ?Sized {}

#[cfg(test)]
mod tests {
    use nautilus_model::data::QuoteTick;
    use rstest::rstest;

    use super::*;
    use crate::catalog::session::TypedDataBatchSession;

    #[derive(Debug)]
    struct ReaderOnlyCatalog;

    impl CatalogReader for ReaderOnlyCatalog {
        fn reset_session(&mut self) {}

        fn instruments(
            &mut self,
            _query: &CatalogInstrumentQuery,
        ) -> anyhow::Result<Vec<InstrumentAny>> {
            Ok(Vec::new())
        }

        fn query_batch(&mut self, _query: &CatalogQuery) -> anyhow::Result<DataBatch> {
            Ok(DataBatch::Quote(Vec::new().into()))
        }

        fn query_batch_session(
            &mut self,
            _query: &CatalogQuery,
            chunk_size: Option<usize>,
        ) -> anyhow::Result<DataBatchQueryResult> {
            Ok(Box::new(TypedDataBatchSession::<QuoteTick>::from_vec(
                Vec::new(),
                chunk_size,
            )))
        }
    }

    #[rstest]
    fn reader_only_catalog_uses_optional_capability_defaults() {
        let mut catalog = ReaderOnlyCatalog;

        let error = catalog
            .query_metadata(&CatalogQuery::new(NautilusDataType::QuoteTick))
            .unwrap_err();

        match error.downcast_ref::<PersistenceError>() {
            Some(PersistenceError::Unsupported(operation)) => {
                assert_eq!(operation, "query_metadata");
            }
            other => panic!("Expected an unsupported capability error, received {other:?}"),
        }
    }

    #[rstest]
    fn reader_only_catalog_reports_every_unimplemented_capability_as_unsupported() {
        let mut catalog = ReaderOnlyCatalog;

        let errors = [
            catalog
                .query_identifiers(&CatalogQuery::new(NautilusDataType::QuoteTick))
                .unwrap_err(),
            catalog
                .get_missing_intervals_for_request(
                    UnixNanos::default(),
                    UnixNanos::default(),
                    NautilusDataType::QuoteTick,
                    None,
                )
                .unwrap_err(),
            catalog
                .query_last_timestamp(NautilusDataType::QuoteTick, None)
                .unwrap_err(),
            catalog
                .query_display_record_batches(&CatalogQuery::new(NautilusDataType::QuoteTick))
                .unwrap_err(),
            catalog
                .query_record_batches(&CatalogRecordQuery::new(NautilusRecordType::AccountState))
                .unwrap_err(),
        ];

        let operations = errors
            .iter()
            .map(|e| match e.downcast_ref::<PersistenceError>() {
                Some(PersistenceError::Unsupported(operation)) => operation.clone(),
                other => panic!("Expected an unsupported capability error, received {other:?}"),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            operations,
            vec![
                "query_identifiers",
                "get_missing_intervals_for_request",
                "query_last_timestamp",
                "query_display_record_batches",
                "query_record_batches",
            ],
        );
    }
}
