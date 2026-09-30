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

use nautilus_core::Params;
use parquet::basic::{BrotliLevel, Compression, GzipLevel, ZstdLevel};

use crate::{
    catalog::{factory as catalog_factory, traits as catalog_traits},
    common::catalog_params::{
        CatalogParamKind, compression_name_from_params, validate_catalog_counts,
        validate_catalog_params,
    },
    config::storage_options_from_params,
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

/// The `params` keys the Parquet catalog accepts.
pub const PARQUET_PARAMS: &[(&str, CatalogParamKind)] = &[
    ("storage_options", CatalogParamKind::Object),
    ("batch_size", CatalogParamKind::Count),
    ("compression", CatalogParamKind::Text),
    ("max_row_group_size", CatalogParamKind::Count),
];

// Streaming promotion opens its catalog here too, so both honor the catalog parameters
pub(crate) fn open_catalog(
    config: &catalog_factory::CatalogConnectConfig,
) -> anyhow::Result<catalog::ParquetDataCatalog> {
    let params = config.params.as_ref();
    validate_catalog_params("Parquet", params, PARQUET_PARAMS)?;
    validate_catalog_counts("Parquet", params)?;
    catalog::ParquetDataCatalog::from_uri(
        &config.uri,
        storage_options_from_params(params)?,
        params.and_then(|params| params.get_usize("batch_size")),
        compression_from_params(params)?,
        params.and_then(|params| params.get_usize("max_row_group_size")),
    )
}

// `compression` names the codec of every file the catalog writes
fn compression_from_params(params: Option<&Params>) -> anyhow::Result<Option<Compression>> {
    Ok(
        compression_name_from_params(params)?.map(|name| match name.as_str() {
            "uncompressed" => Compression::UNCOMPRESSED,
            "snappy" => Compression::SNAPPY,
            "gzip" => Compression::GZIP(GzipLevel::default()),
            "brotli" => Compression::BROTLI(BrotliLevel::default()),
            // `lz4` writes the raw LZ4 block format, as `lz4_raw` does
            "lz4" | "lz4_raw" => Compression::LZ4_RAW,
            _ => Compression::ZSTD(ZstdLevel::default()),
        }),
    )
}
