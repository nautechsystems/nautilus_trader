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

//! Parquet catalog backend.

use std::sync::Arc;

use parquet::basic::{BrotliLevel, Compression, GzipLevel, ZstdLevel};

use crate::{
    catalog::{factory as catalog_factory, traits as catalog_traits},
    config::{CatalogCompression, validate_catalog_counts},
};

pub mod catalog;
pub mod consolidation;
pub mod delete;
pub mod feather_session;
pub mod file_admin;
pub mod intervals;
pub mod io;
pub mod migration;
pub mod paths;
pub mod writer;

pub(crate) mod metadata;

/// Default number of rows in a Parquet row group.
pub const DEFAULT_ROW_GROUP_SIZE: usize = 131_072;

pub(crate) fn register_catalog_factory(registry: &mut catalog_factory::CatalogFactoryRegistry) {
    registry.insert(
        catalog_factory::PARQUET_CATALOG_FACTORY_NAME.to_string(),
        Arc::new(|config: &catalog_factory::CatalogConnectConfig| {
            Ok(Box::new(open_catalog(config)?) as catalog_traits::DataCatalog)
        }),
    );
}

// Streaming promotion opens its catalog here too, so both honor the catalog settings
pub(crate) fn open_catalog(
    config: &catalog_factory::CatalogConnectConfig,
) -> anyhow::Result<catalog::ParquetDataCatalog> {
    // Parquet reads only the typed settings, so every `params` key is unknown
    catalog_factory::validate_catalog_params(
        catalog_factory::PARQUET_CATALOG_FACTORY_NAME,
        config.params.as_ref(),
        &[],
    )?;
    validate_catalog_counts(config.batch_size, config.max_row_group_size)?;

    catalog::ParquetDataCatalog::from_uri(
        &config.uri,
        config.storage_options.clone(),
        config.batch_size,
        config.compression.map(Compression::from),
        config.max_row_group_size,
    )
}

impl From<CatalogCompression> for Compression {
    fn from(compression: CatalogCompression) -> Self {
        match compression {
            CatalogCompression::Uncompressed => Self::UNCOMPRESSED,
            CatalogCompression::Snappy => Self::SNAPPY,
            CatalogCompression::Gzip => Self::GZIP(GzipLevel::default()),
            CatalogCompression::Brotli => Self::BROTLI(BrotliLevel::default()),
            CatalogCompression::Lz4 => Self::LZ4_RAW,
            CatalogCompression::Zstd => Self::ZSTD(ZstdLevel::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use arrow::record_batch::RecordBatch;
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::data::{NautilusDataType, QuoteTick, stubs::quote_audusd};
    use parquet::file::{
        metadata::{ParquetMetaData, RowGroupMetaData},
        reader::{FileReader, SerializedFileReader},
    };
    use rstest::rstest;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::config::DataCatalogConfig;

    #[rstest]
    fn open_catalog_writes_with_typed_settings() {
        let directory = TempDir::new().unwrap();
        let config = DataCatalogConfig::builder()
            .path(directory.path().to_string_lossy().to_string())
            .batch_size(2)
            .compression(CatalogCompression::Uncompressed)
            .max_row_group_size(3)
            .build()
            .unwrap();
        let catalog = open_catalog(&config.connect_config()).unwrap();
        let quotes = (1..=5).map(quote).collect::<Vec<_>>();

        catalog.write_to_parquet(&quotes, None, None, None).unwrap();

        let batches = catalog.data_to_record_batches(&quotes).unwrap();
        let metadata = quote_file_metadata(&catalog, &directory);

        assert_eq!(catalog.batch_size, 2);
        assert_eq!(
            batches
                .iter()
                .map(RecordBatch::num_rows)
                .collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
        assert_eq!(
            metadata
                .row_groups()
                .iter()
                .map(RowGroupMetaData::num_rows)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert!(
            metadata
                .row_groups()
                .iter()
                .flat_map(RowGroupMetaData::columns)
                .all(|column| column.compression() == Compression::UNCOMPRESSED)
        );
    }

    #[rstest]
    #[case::uncompressed(CatalogCompression::Uncompressed, Compression::UNCOMPRESSED)]
    #[case::snappy(CatalogCompression::Snappy, Compression::SNAPPY)]
    #[case::gzip(CatalogCompression::Gzip, Compression::GZIP(GzipLevel::default()))]
    #[case::brotli(
        CatalogCompression::Brotli,
        Compression::BROTLI(BrotliLevel::default())
    )]
    #[case::lz4(CatalogCompression::Lz4, Compression::LZ4_RAW)]
    #[case::zstd(CatalogCompression::Zstd, Compression::ZSTD(ZstdLevel::default()))]
    fn open_catalog_writes_each_compression(
        #[case] compression: CatalogCompression,
        #[case] expected: Compression,
    ) {
        let directory = TempDir::new().unwrap();
        let mut config =
            catalog_factory::CatalogConnectConfig::new(directory.path().to_string_lossy(), None);
        config.compression = Some(compression);
        let catalog = open_catalog(&config).unwrap();

        catalog
            .write_to_parquet(&[quote(1)], None, None, None)
            .unwrap();

        let metadata = quote_file_metadata(&catalog, &directory);

        assert_eq!(catalog.compression, expected);
        assert!(
            metadata
                .row_group(0)
                .columns()
                .iter()
                .all(|column| column.compression() == expected)
        );
    }

    #[rstest]
    #[case::moved_setting("batch_size")]
    #[case::unknown("no_such_param")]
    fn open_catalog_rejects_params_naming_key(#[case] key: &str) {
        let directory = TempDir::new().unwrap();
        let mut params = Params::new();
        params.insert(key.to_string(), json!(1024));
        let config = DataCatalogConfig::builder()
            .path(directory.path().to_string_lossy().to_string())
            .params(params)
            .build()
            .unwrap();

        let error = open_catalog(&config.connect_config()).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Unknown Parquet catalog param '{key}': this catalog takes no params")
        );
    }

    // A deserialized config skips `DataCatalogConfig::validate`, so the factory checks counts too
    #[rstest]
    #[case::batch_size(Some(0), None, "batch_size")]
    #[case::max_row_group_size(None, Some(0), "max_row_group_size")]
    fn open_catalog_rejects_zero_count(
        #[case] batch_size: Option<usize>,
        #[case] max_row_group_size: Option<usize>,
        #[case] field: &str,
    ) {
        let directory = TempDir::new().unwrap();
        let mut config =
            catalog_factory::CatalogConnectConfig::new(directory.path().to_string_lossy(), None);
        config.batch_size = batch_size;
        config.max_row_group_size = max_row_group_size;

        let error = open_catalog(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "invalid {field}: must be a positive number of rows; omit the field for the \
                 backend default"
            )
        );
    }

    fn quote_file_metadata(
        catalog: &catalog::ParquetDataCatalog,
        directory: &TempDir,
    ) -> ParquetMetaData {
        let files = catalog
            .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
            .unwrap();
        assert_eq!(files.len(), 1);

        SerializedFileReader::new(File::open(directory.path().join(&files[0])).unwrap())
            .unwrap()
            .metadata()
            .clone()
    }

    fn quote(ts: u64) -> QuoteTick {
        QuoteTick {
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..quote_audusd()
        }
    }
}
