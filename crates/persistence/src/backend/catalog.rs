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

//! Parquet catalog compatibility export.

pub use super::parquet::{
    catalog::ParquetDataCatalog,
    paths::{
        extract_path_components, extract_sql_safe_filename, local_to_object_store_path,
        make_local_path, make_object_store_path as make_object_store_path_owned,
        make_object_store_path, make_sql_safe_identifier, safe_directory_identifier,
    },
};

/// Returns the parent path component, or `unknown` for a path without one.
#[must_use]
pub fn extract_identifier_from_path(file_path: &str) -> String {
    super::parquet::paths::extract_identifier_from_path(file_path)
        .unwrap_or("unknown")
        .to_string()
}

pub use super::parquet::paths::timestamps_to_filename;
