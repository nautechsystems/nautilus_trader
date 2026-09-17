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

mod arrow;
mod conversion;

pub(crate) use arrow::{
    arrow_ipc_batches, arrow_ipc_data_schema, arrow_ipc_record_schema,
    arrow_record_batches_from_pybytes,
};
pub(crate) use conversion::{
    catalog_data_type_from_py, catalog_metadata_to_pydict, catalog_record_type_from_py,
    to_pyio_err, write_record_params_from_py, writer_record_filter_from_py,
};

pub mod feather;
pub mod parquet;
pub mod session;
pub mod writer;
