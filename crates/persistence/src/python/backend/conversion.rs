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

use std::fmt::Display;

use nautilus_core::{
    Params,
    params::from_pydict,
    python::{params::params_to_pydict, to_pytype_err},
};
use nautilus_model::{
    data::{NautilusDataType, NautilusRecordType},
    python::data::{PyNautilusDataType, PyNautilusRecordType},
};
use pyo3::{
    exceptions::PyIOError,
    prelude::*,
    types::{PyDict, PyList},
};
use serde_json::json;

use crate::{
    catalog::{
        traits::{CatalogMetadata, NautilusDataTypePrefix, NautilusRecordTypePrefix},
        types::data_type_from_data_path_prefix,
    },
    writer::filter::WriterRecordFilter,
};

pub(crate) fn to_pyio_err(error: impl Display) -> PyErr {
    PyIOError::new_err(error.to_string())
}

pub(crate) fn catalog_metadata_to_pydict(
    py: Python<'_>,
    metadata: Vec<CatalogMetadata>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for item in metadata {
        dict.set_item(
            item.first_ts_init.as_u64(),
            params_to_pydict(py, &item.metadata)?,
        )?;
    }
    Ok(dict.into())
}

pub(crate) fn catalog_record_type_from_py(
    record_type: &Bound<'_, PyAny>,
) -> PyResult<NautilusRecordType> {
    record_type
        .extract::<PyRef<'_, PyNautilusRecordType>>()
        .map(|record_type| record_type.inner())
        .map_err(|_| to_pytype_err("record_type must be NautilusRecordType"))
}

pub(crate) fn catalog_data_type_from_py(
    data_type: &Bound<'_, PyAny>,
) -> PyResult<NautilusDataType> {
    data_type
        .extract::<PyRef<'_, PyNautilusDataType>>()
        .map(|data_type| data_type.inner())
        .map_err(|_| to_pytype_err("data_type must be NautilusDataType"))
}

pub(crate) fn write_record_params_from_py(
    py: Python<'_>,
    identifier: Option<String>,
    params: Option<Py<PyDict>>,
) -> PyResult<Option<Params>> {
    let mut params = match params {
        Some(params) => from_pydict(py, &params)?.unwrap_or_default(),
        None => Params::new(),
    };

    if let Some(identifier) = identifier {
        params.insert("identifier".to_string(), json!(identifier));
    }

    Ok((!params.is_empty()).then_some(params))
}

pub(crate) fn writer_record_filter_from_py(
    record_types: Option<&Bound<'_, PyAny>>,
    record_filters: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<WriterRecordFilter>> {
    let mut filter = WriterRecordFilter::new();

    if let Some(record_types) = record_types {
        let record_types = record_types.cast::<PyList>()?;
        for item in record_types.iter() {
            filter.insert_prefix(catalog_filter_prefix_from_py(&item)?, None);
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
            filter.insert_prefix(catalog_filter_prefix_from_py(&record_type)?, identifiers);
        }
    }

    Ok((!filter.is_empty()).then_some(filter))
}

fn catalog_filter_prefix_from_py(value: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(record_type) = value.extract::<PyRef<'_, PyNautilusRecordType>>() {
        return Ok(record_type.inner().path_prefix().into_owned());
    }

    if let Ok(data_type) = value.extract::<PyRef<'_, PyNautilusDataType>>() {
        return Ok(data_type.inner().path_prefix().into_owned());
    }

    if let Ok(value) = value.extract::<String>() {
        if let Ok(record_type) = value.parse::<NautilusRecordType>() {
            return Ok(record_type.path_prefix().into_owned());
        }
        return data_type_from_data_path_prefix(&value)
            .map(|data_type| data_type.path_prefix().into_owned())
            .map_err(to_pytype_err);
    }
    Err(to_pytype_err(
        "filter key must be NautilusRecordType, NautilusDataType, or str",
    ))
}
