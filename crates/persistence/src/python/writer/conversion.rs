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

use std::collections::HashSet;

use nautilus_core::{UnixNanos, datetime::get_timezone, python::to_pytype_err};
use nautilus_model::python::data::{PyNautilusDataType, PyNautilusRecordType};
use pyo3::{
    exceptions::PyIOError,
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{
    catalog::types::CatalogDataType,
    common::config::RotationMode,
    python::catalog::conversion::catalog_data_type_from_py,
    writer::{
        feather::RotationConfig,
        filter::{WriterRecordFilter, catalog_family},
    },
};

pub(crate) fn rotation_config_from_python(
    rotation_mode: RotationMode,
    max_file_size: u64,
    rotation_interval_ns: Option<u64>,
    rotation_time_ns: Option<u64>,
    rotation_timezone: &str,
) -> PyResult<RotationConfig> {
    match rotation_mode {
        RotationMode::Size => Ok(RotationConfig::Size {
            max_size: max_file_size,
        }),
        RotationMode::Interval => Ok(RotationConfig::Interval {
            interval_ns: rotation_interval_ns.unwrap_or(86_400_000_000_000),
        }),
        RotationMode::ScheduledDates => {
            let rotation_timezone = get_timezone(rotation_timezone).map_err(|e| {
                PyIOError::new_err(format!("Failed to parse rotation_timezone: {e}"))
            })?;
            Ok(RotationConfig::ScheduledDates {
                interval_ns: rotation_interval_ns.unwrap_or(86_400_000_000_000),
                rotation_time: UnixNanos::from(rotation_time_ns.unwrap_or(0)),
                rotation_timezone,
            })
        }
        RotationMode::NoRotation => Ok(RotationConfig::NoRotation),
    }
}

pub(crate) fn writer_types_from_py(
    values: Option<Vec<Bound<'_, PyAny>>>,
) -> PyResult<(Option<HashSet<CatalogDataType>>, Option<WriterRecordFilter>)> {
    let Some(values) = values else {
        return Ok((None, None));
    };
    let catalog_types = values
        .iter()
        .map(catalog_data_type_from_py)
        .collect::<PyResult<Vec<_>>>()?;
    let mut families = HashSet::new();
    let mut filter = WriterRecordFilter::new();

    for catalog_type in catalog_types {
        families.insert(catalog_family(&catalog_type));

        if let CatalogDataType::Instrument(instrument_type) = catalog_type {
            filter.insert_instrument_type(instrument_type);
        } else {
            filter.insert(catalog_type, None);
        }
    }

    Ok((Some(families), (!filter.is_empty()).then_some(filter)))
}

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

fn catalog_filter_family_from_py(value: &Bound<'_, PyAny>) -> PyResult<CatalogDataType> {
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
