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

use crate::catalog::{factory as catalog_factory, traits as catalog_traits};

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
            let params = config.params.as_ref();
            Ok(Box::new(catalog::ParquetDataCatalog::from_uri(
                &config.uri,
                config.storage_options.clone(),
                params.and_then(|params| params.get_usize("batch_size")),
                params
                    .and_then(|params| params.get_u64("compression"))
                    .map(compression_from_code),
                params.and_then(|params| params.get_usize("max_row_group_size")),
            )?) as catalog_traits::DataCatalogBox)
        }),
    );
}

fn compression_from_code(code: u64) -> ::parquet::basic::Compression {
    match code {
        0 => ::parquet::basic::Compression::UNCOMPRESSED,
        2 => ::parquet::basic::Compression::GZIP(::parquet::basic::GzipLevel::default()),
        3 => ::parquet::basic::Compression::LZO,
        4 => ::parquet::basic::Compression::BROTLI(::parquet::basic::BrotliLevel::default()),
        5 => ::parquet::basic::Compression::LZ4,
        6 => ::parquet::basic::Compression::ZSTD(::parquet::basic::ZstdLevel::default()),
        _ => ::parquet::basic::Compression::SNAPPY,
    }
}
