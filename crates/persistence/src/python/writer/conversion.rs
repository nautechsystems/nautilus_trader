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

//! Python conversions for writer bindings.

use nautilus_core::python::to_pytype_err;
use nautilus_model::python::data::{PyNautilusDataType, PyNautilusRecordType};
use pyo3::{
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{catalog::types::CatalogDataType, writer::filter::WriterRecordFilter};

pub(crate) fn writer_record_filter_from_py(
    record_types: Option<&Bound<'_, PyAny>>,
    record_filters: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<WriterRecordFilter>> {
    let mut filter = WriterRecordFilter::new();

    if let Some(record_types) = record_types {
        let record_types = record_types.cast::<PyList>()?;
        for item in record_types.iter() {
            filter.insert(catalog_filter_family_from_py(&item)?, None);
        }
    }

    if let Some(record_filters) = record_filters {
        let record_filters = record_filters.cast::<PyDict>()?;
        for (record_type, identifiers) in record_filters {
            let identifiers = if identifiers.is_none() {
                None
            } else if let Ok(identifier) = identifiers.extract::<String>() {
                Some(vec![identifier])
            } else {
                Some(identifiers.extract::<Vec<String>>()?)
            };

            filter.insert(catalog_filter_family_from_py(&record_type)?, identifiers);
        }
    }

    Ok((!filter.is_empty()).then_some(filter))
}

pub(crate) fn catalog_filter_family_from_py(value: &Bound<'_, PyAny>) -> PyResult<CatalogDataType> {
    if let Ok(record_type) = value.extract::<PyRef<'_, PyNautilusRecordType>>() {
        return Ok(CatalogDataType::Record(record_type.inner()));
    }

    if let Ok(data_type) = value.extract::<PyRef<'_, PyNautilusDataType>>() {
        return Ok(CatalogDataType::Data(data_type.inner()));
    }

    Err(to_pytype_err(
        "filter key must be NautilusRecordType or NautilusDataType",
    ))
}
