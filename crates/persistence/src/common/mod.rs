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

//! Shared persistence primitives used by catalog and writer backends.

pub const TABLE_PATH_COLUMN: &str = "_nautilus_table_path";
pub const DATA_TYPE_COLUMN: &str = "_nautilus_data_type";
pub const START_TS_COLUMN: &str = "_nautilus_start_ts";
pub const END_TS_COLUMN: &str = "_nautilus_end_ts";
pub const STATUS_COLUMN: &str = "_nautilus_status";
pub const ROW_COUNT_COLUMN: &str = "_nautilus_row_count";
pub const DATA_VERSION_COLUMN: &str = "_nautilus_data_version";
pub const SOURCE_COLUMN: &str = "_nautilus_source";
pub const CREATED_TS_COLUMN: &str = "_nautilus_created_ts";
pub const SCHEMA_VERSION_COLUMN: &str = "_nautilus_schema_version";
pub const METADATA_ID_COLUMN: &str = "_nautilus_metadata_id";
pub const METADATA_JSON_COLUMN: &str = "_nautilus_metadata_json";
pub const FORMAT_VERSION_COLUMN: &str = "_nautilus_format_version";
pub const CLUSTER_KEY_COLUMN: &str = "_nautilus_cluster_key";

pub mod conversion;
pub mod coverage;
pub mod custom;
pub mod datafusion;
pub mod paths;
pub mod storage;

pub(crate) mod arrow;
pub(crate) mod backend_name;
pub(crate) mod metadata;
