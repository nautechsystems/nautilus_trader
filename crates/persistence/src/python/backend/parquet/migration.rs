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

//! Python bindings for legacy Parquet catalog migration.

use pyo3::prelude::*;

use crate::{
    backend::parquet::migration::{ParquetCatalogSource, build_catalog_migration_plan},
    python::common::to_pyio_err,
};

pub(crate) fn parquet_migration_dry_run(
    py: Python<'_>,
    source: &dyn ParquetCatalogSource,
) -> PyResult<usize> {
    py.detach(|| {
        let plan = build_catalog_migration_plan(source)?;
        plan.ensure_ready()?;
        Ok::<usize, anyhow::Error>(0)
    })
    .map_err(to_pyio_err)
}
