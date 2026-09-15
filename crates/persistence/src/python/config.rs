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

//! Python bindings for persistence configuration types.

use nautilus_core::{
    DurationNanos, UnixNanos,
    datetime::NANOSECONDS_IN_DAY,
    from_pydict,
    python::{params::params_to_pydict, to_pytype_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{NautilusDataType, NautilusRecordType},
    instruments::NautilusInstrumentType,
    python::{
        data::{PyNautilusDataType, PyNautilusRecordType},
        instruments::PyNautilusInstrumentType,
    },
};
use pyo3::{
    Bound, Py, PyAny, PyRef, PyResult, Python,
    types::{PyAnyMethods, PyDict},
};

use crate::{
    config::{
        CatalogBackendType, DataCatalogConfig, RotationConfig, StreamingConfig,
        StreamingRecordFilterConfig, default_fs_protocol,
    },
    writer::factory::WriterBackendType,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[pyo3::pyclass(
    frozen,
    name = "CatalogBackend",
    module = "nautilus_trader.persistence",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyCatalogBackend {
    inner: CatalogBackendType,
}

impl PyCatalogBackend {
    #[must_use]
    pub const fn new(inner: CatalogBackendType) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn inner(&self) -> CatalogBackendType {
        self.inner.clone()
    }
}

fn parse_catalog_backend(value: &str) -> PyResult<CatalogBackendType> {
    value.parse::<CatalogBackendType>().map_err(to_pyvalue_err)
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl PyCatalogBackend {
    #[classattr]
    #[expect(
        non_snake_case,
        clippy::use_self,
        reason = "PyO3 stub generation needs the concrete Python enum type"
    )]
    fn Parquet() -> PyCatalogBackend {
        Self::new(CatalogBackendType::Parquet)
    }

    #[staticmethod]
    fn from_str(value: &str) -> PyResult<Self> {
        parse_catalog_backend(value).map(Self::new)
    }

    #[staticmethod]
    #[pyo3(name = "External")]
    fn py_external(name: &str) -> PyResult<Self> {
        let backend = name.parse::<CatalogBackendType>().map_err(to_pyvalue_err)?;
        match backend {
            CatalogBackendType::External(_) => Ok(Self::new(backend)),
            CatalogBackendType::Parquet => Err(to_pyvalue_err(format!(
                "Catalog backend name '{name}' is reserved for a built-in backend"
            ))),
        }
    }

    #[getter]
    fn name(&self) -> &str {
        match self.inner {
            CatalogBackendType::Parquet => "Parquet",
            CatalogBackendType::External(_) => "External",
        }
    }

    #[getter]
    fn value(&self) -> String {
        self.inner.to_string()
    }

    #[getter]
    fn external_name(&self) -> Option<&str> {
        match &self.inner {
            CatalogBackendType::External(name) => Some(name),
            CatalogBackendType::Parquet => None,
        }
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            CatalogBackendType::Parquet => "CatalogBackend.Parquet".to_string(),
            CatalogBackendType::External(name) => {
                format!("CatalogBackend.External({name:?})")
            }
        }
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        use nautilus_core::python::IntoPyObjectNautilusExt;

        match op {
            pyo3::pyclass::CompareOp::Eq => (self.inner == other.inner).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => (self.inner != other.inner).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "Python hashes use the platform signed integer width"
    )]
    fn __hash__(&self) -> isize {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish() as isize
    }
}

#[derive(Clone, Debug)]
#[pyo3::pyclass(
    frozen,
    name = "RotationConfig",
    module = "nautilus_trader.persistence",
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyRotationConfig {
    inner: RotationConfig,
}

impl From<PyRotationConfig> for RotationConfig {
    fn from(config: PyRotationConfig) -> Self {
        config.inner
    }
}

impl From<RotationConfig> for PyRotationConfig {
    fn from(config: RotationConfig) -> Self {
        Self { inner: config }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl PyRotationConfig {
    #[staticmethod]
    fn no_rotation() -> Self {
        Self {
            inner: RotationConfig::NoRotation,
        }
    }

    #[staticmethod]
    fn size(max_size: u64) -> Self {
        Self {
            inner: RotationConfig::Size { max_size },
        }
    }

    #[staticmethod]
    fn interval(interval_ns: u64) -> Self {
        Self {
            inner: RotationConfig::Interval {
                interval_ns: nautilus_core::DurationNanos::new(interval_ns),
            },
        }
    }

    #[staticmethod]
    fn scheduled_dates(interval_ns: u64, schedule_ns: u64) -> Self {
        Self {
            inner: RotationConfig::ScheduledDates {
                interval_ns: nautilus_core::DurationNanos::new(interval_ns),
                schedule_ns: UnixNanos::from(schedule_ns),
            },
        }
    }

    #[getter]
    fn mode(&self) -> &'static str {
        match self.inner {
            RotationConfig::Size { .. } => "size",
            RotationConfig::Interval { .. } => "interval",
            RotationConfig::ScheduledDates { .. } => "scheduled_dates",
            RotationConfig::NoRotation => "no_rotation",
        }
    }

    #[getter]
    fn max_size(&self) -> Option<u64> {
        match self.inner {
            RotationConfig::Size { max_size } => Some(max_size),
            _ => None,
        }
    }

    #[getter]
    fn interval_ns(&self) -> Option<u64> {
        match self.inner {
            RotationConfig::Interval { interval_ns }
            | RotationConfig::ScheduledDates { interval_ns, .. } => Some(interval_ns.as_u64()),
            _ => None,
        }
    }

    #[getter]
    fn schedule_ns(&self) -> Option<u64> {
        match self.inner {
            RotationConfig::ScheduledDates { schedule_ns, .. } => Some(schedule_ns.as_u64()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[derive(Clone, Debug)]
#[pyo3::pyclass(
    frozen,
    name = "StreamingConfig",
    module = "nautilus_trader.persistence",
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyStreamingConfig {
    inner: StreamingConfig,
}

impl From<PyStreamingConfig> for StreamingConfig {
    fn from(config: PyStreamingConfig) -> Self {
        config.inner
    }
}

impl From<StreamingConfig> for PyStreamingConfig {
    fn from(config: StreamingConfig) -> Self {
        Self { inner: config }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl PyStreamingConfig {
    #[new]
    #[expect(
        clippy::too_many_arguments,
        reason = "the PyO3 constructor mirrors the public Python configuration signature"
    )]
    #[pyo3(signature = (
        catalog_path,
        fs_protocol = None,
        flush_interval_ms = 1000,
        replace_existing = false,
        rotation_config = None,
        writer_backend = None,
        data_types = None,
        record_types = None,
        instrument_types = None,
        record_filters = None,
        params = None,
        rotation_mode = None,
        max_file_size = None,
        rotation_interval_ns = None,
        schedule_ns = None,
    ))]
    fn py_new(
        catalog_path: String,
        fs_protocol: Option<String>,
        flush_interval_ms: u64,
        replace_existing: bool,
        rotation_config: Option<PyRotationConfig>,
        writer_backend: Option<String>,
        data_types: Option<&Bound<'_, PyAny>>,
        record_types: Option<&Bound<'_, PyAny>>,
        instrument_types: Option<&Bound<'_, PyAny>>,
        record_filters: Option<&Bound<'_, PyAny>>,
        params: Option<Py<PyDict>>,
        rotation_mode: Option<&str>,
        max_file_size: Option<u64>,
        rotation_interval_ns: Option<u64>,
        schedule_ns: Option<u64>,
    ) -> pyo3::PyResult<Self> {
        let rotation_config = if let Some(config) = rotation_config {
            if rotation_mode.is_some()
                || max_file_size.is_some()
                || rotation_interval_ns.is_some()
                || schedule_ns.is_some()
            {
                return Err(to_pyvalue_err(
                    "rotation_config cannot be combined with legacy rotation options",
                ));
            }
            config.into()
        } else {
            match rotation_mode
                .unwrap_or("NO_ROTATION")
                .to_ascii_uppercase()
                .as_str()
            {
                "SIZE" => {
                    let max_size = max_file_size.unwrap_or(1_073_741_824);
                    if max_size == 0 {
                        return Err(to_pyvalue_err("max_file_size must be positive"));
                    }
                    RotationConfig::Size { max_size }
                }
                "INTERVAL" => RotationConfig::Interval {
                    interval_ns: positive_interval(rotation_interval_ns)?,
                },
                "SCHEDULED_DATES" => RotationConfig::ScheduledDates {
                    interval_ns: positive_interval(rotation_interval_ns)?,
                    schedule_ns: UnixNanos::from(schedule_ns.unwrap_or(0)),
                },
                "NO_ROTATION" => RotationConfig::NoRotation,
                mode => return Err(to_pyvalue_err(format!("Invalid rotation_mode: '{mode}'"))),
            }
        };
        let mut inner = StreamingConfig::new(
            catalog_path,
            fs_protocol.unwrap_or_else(default_fs_protocol),
            flush_interval_ms,
            replace_existing,
            rotation_config,
        );
        inner.writer_backend = writer_backend
            .map(|backend| backend.parse::<WriterBackendType>())
            .transpose()
            .map_err(to_pyvalue_err)?
            .unwrap_or_default();
        let mut parsed_types = py_streaming_types_from_any(data_types)?;
        parsed_types
            .records
            .extend(py_record_types_from_any(record_types)?.unwrap_or_default());
        parsed_types
            .instruments
            .extend(py_instrument_types_from_any(instrument_types)?.unwrap_or_default());
        inner.data_types = (!parsed_types.data.is_empty()).then_some(parsed_types.data);
        inner.record_types = (!parsed_types.records.is_empty()).then_some(parsed_types.records);
        inner.instrument_types =
            (!parsed_types.instruments.is_empty()).then_some(parsed_types.instruments);
        inner.record_filters = py_record_filters_from_any(record_filters)?;
        inner.params = Python::attach(|py| match params {
            Some(params) => from_pydict(py, &params),
            None => Ok(None),
        })?;

        Ok(Self { inner })
    }

    #[getter]
    fn catalog_path(&self) -> &str {
        &self.inner.catalog_path
    }

    #[getter]
    fn fs_protocol(&self) -> &str {
        &self.inner.fs_protocol
    }

    #[getter]
    const fn flush_interval_ms(&self) -> u64 {
        self.inner.flush_interval_ms
    }

    #[getter]
    const fn replace_existing(&self) -> bool {
        self.inner.replace_existing
    }

    #[getter]
    fn rotation_config(&self) -> PyRotationConfig {
        self.inner.rotation_config.clone().into()
    }

    #[getter]
    fn rotation_mode(&self) -> String {
        self.rotation_config().mode().to_ascii_uppercase()
    }

    #[getter]
    fn max_file_size(&self) -> Option<u64> {
        self.rotation_config().max_size()
    }

    #[getter]
    fn rotation_interval_ns(&self) -> Option<u64> {
        self.rotation_config().interval_ns()
    }

    #[getter]
    fn schedule_ns(&self) -> Option<u64> {
        self.rotation_config().schedule_ns()
    }

    #[getter]
    fn writer_backend(&self) -> String {
        self.inner.writer_backend.to_string()
    }

    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.inner
            .params
            .as_ref()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    fn data_types(&self) -> Option<Vec<String>> {
        self.inner
            .data_types
            .clone()
            .map(|values| values.into_iter().map(|value| value.to_string()).collect())
    }

    #[getter]
    fn record_types(&self) -> Option<Vec<String>> {
        self.inner
            .record_types
            .clone()
            .map(|values| values.into_iter().map(|value| value.to_string()).collect())
    }

    #[getter]
    fn instrument_types(&self) -> Option<Vec<String>> {
        self.inner
            .instrument_types
            .clone()
            .map(|values| values.into_iter().map(|value| value.to_string()).collect())
    }

    #[getter]
    fn record_filters(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        let Some(filters) = &self.inner.record_filters else {
            return Ok(None);
        };
        let result = PyDict::new(py);
        for filter in filters {
            result.set_item(filter.record_type.to_string(), filter.identifiers.clone())?;
        }
        Ok(Some(result.unbind()))
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl DataCatalogConfig {
    /// Configuration for a catalog available to request-time historical data loading.
    #[new]
    #[pyo3(signature = (path, fs_protocol = None, catalog_backend = None, params = None, name = None, read_only = false, fs_rust_storage_options = None))]
    fn py_new(
        path: String,
        fs_protocol: Option<String>,
        catalog_backend: Option<PyRef<'_, PyCatalogBackend>>,
        params: Option<Py<PyDict>>,
        name: Option<String>,
        read_only: bool,
        fs_rust_storage_options: Option<std::collections::HashMap<String, String>>,
    ) -> pyo3::PyResult<Self> {
        let catalog_backend = catalog_backend.map(|backend| backend.inner());
        let params = Python::attach(|py| match params {
            Some(params) => from_pydict(py, &params),
            None => Ok(None),
        })?;
        Ok(Self::new(path, fs_protocol, catalog_backend)
            .with_params(params)
            .with_name(name)
            .with_read_only(read_only)
            .with_storage_options(
                fs_rust_storage_options.map(|options| options.into_iter().collect()),
            ))
    }

    /// Returns the path to the data catalog.
    #[getter]
    #[pyo3(name = "path")]
    fn py_path(&self) -> &str {
        self.path()
    }

    /// Returns the catalog registration name.
    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> Option<&str> {
        self.name()
    }

    /// Returns whether the catalog rejects response write-back.
    #[getter]
    #[pyo3(name = "read_only")]
    fn py_read_only(&self) -> bool {
        self.read_only()
    }

    /// Returns the fsspec file system protocol for the data catalog.
    #[getter]
    #[pyo3(name = "fs_protocol")]
    fn py_fs_protocol(&self) -> &str {
        self.fs_protocol()
    }

    /// Returns the catalog backend implementation to use.
    #[getter]
    #[pyo3(name = "catalog_backend")]
    fn py_catalog_backend(&self) -> PyCatalogBackend {
        PyCatalogBackend::new(self.catalog_backend().clone())
    }

    /// Returns backend-specific catalog parameters.
    #[getter]
    #[pyo3(name = "params")]
    fn py_params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.params()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }

    #[getter]
    fn fs_rust_storage_option_keys(&self) -> Option<Vec<String>> {
        self.fs_rust_storage_options().map(|options| {
            let mut keys = options.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys
        })
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[derive(Default)]
struct ParsedStreamingTypes {
    data: Vec<NautilusDataType>,
    records: Vec<NautilusRecordType>,
    instruments: Vec<NautilusInstrumentType>,
}

fn py_streaming_type_from_any(
    value: &Bound<'_, PyAny>,
    parsed: &mut ParsedStreamingTypes,
) -> pyo3::PyResult<()> {
    if let Ok(data_type) = value.extract::<PyRef<'_, PyNautilusDataType>>() {
        parsed.data.push(data_type.inner());
        return Ok(());
    }

    if let Ok(record_type) = value.extract::<PyRef<'_, PyNautilusRecordType>>() {
        parsed.records.push(record_type.inner());
        return Ok(());
    }

    if let Ok(instrument_type) = value.extract::<PyRef<'_, PyNautilusInstrumentType>>() {
        parsed.instruments.push(instrument_type.inner());
        return Ok(());
    }

    if let Ok(value) = value.extract::<String>() {
        if let Ok(data_type) = value.parse::<NautilusDataType>() {
            parsed.data.push(data_type);
            return Ok(());
        }

        if let Ok(record_type) = value.parse::<NautilusRecordType>() {
            parsed.records.push(record_type);
            return Ok(());
        }

        if let Ok(instrument_type) = value.parse::<NautilusInstrumentType>() {
            parsed.instruments.push(instrument_type);
            return Ok(());
        }
    }

    Err(to_pytype_err(
        "streaming type must be NautilusDataType, NautilusRecordType, NautilusInstrumentType, or str",
    ))
}

fn py_streaming_types_from_any(
    values: Option<&Bound<'_, PyAny>>,
) -> pyo3::PyResult<ParsedStreamingTypes> {
    let mut parsed = ParsedStreamingTypes::default();

    if let Some(values) = values {
        for item in values.try_iter()? {
            py_streaming_type_from_any(&item?, &mut parsed)?;
        }
    }
    Ok(parsed)
}

fn py_record_type_from_any(record_type: &Bound<'_, PyAny>) -> pyo3::PyResult<NautilusRecordType> {
    if let Ok(record_type) = record_type.extract::<PyRef<'_, PyNautilusRecordType>>() {
        return Ok(record_type.inner());
    }

    if let Ok(record_type) = record_type.extract::<String>() {
        return record_type
            .parse::<NautilusRecordType>()
            .map_err(to_pytype_err);
    }

    Err(to_pytype_err(
        "record_type must be NautilusRecordType or str",
    ))
}

fn py_record_types_from_any(
    record_types: Option<&Bound<'_, PyAny>>,
) -> pyo3::PyResult<Option<Vec<NautilusRecordType>>> {
    record_types
        .map(|record_types| {
            record_types
                .try_iter()?
                .map(|item| py_record_type_from_any(&item?))
                .collect::<pyo3::PyResult<Vec<_>>>()
        })
        .transpose()
}

pub(crate) fn py_instrument_type_from_any(
    instrument_type: &Bound<'_, PyAny>,
) -> pyo3::PyResult<NautilusInstrumentType> {
    if let Ok(instrument_type) = instrument_type.extract::<PyRef<'_, PyNautilusInstrumentType>>() {
        return Ok(instrument_type.inner());
    }

    if let Ok(instrument_type) = instrument_type.extract::<String>() {
        return instrument_type
            .parse::<NautilusInstrumentType>()
            .map_err(to_pytype_err);
    }

    Err(to_pytype_err(
        "instrument_type must be NautilusInstrumentType or str",
    ))
}

fn py_instrument_types_from_any(
    instrument_types: Option<&Bound<'_, PyAny>>,
) -> pyo3::PyResult<Option<Vec<NautilusInstrumentType>>> {
    instrument_types
        .map(|instrument_types| {
            instrument_types
                .try_iter()?
                .map(|item| py_instrument_type_from_any(&item?))
                .collect::<pyo3::PyResult<Vec<_>>>()
        })
        .transpose()
}

fn py_record_filters_from_any(
    record_filters: Option<&Bound<'_, PyAny>>,
) -> pyo3::PyResult<Option<Vec<StreamingRecordFilterConfig>>> {
    let Some(record_filters) = record_filters else {
        return Ok(None);
    };
    let record_filters = record_filters.cast::<PyDict>()?;
    let mut filters = Vec::with_capacity(record_filters.len()?);
    for (record_type, identifiers) in record_filters {
        let record_type = py_record_type_from_any(&record_type)?;
        let identifiers = if identifiers.is_none() {
            None
        } else if let Ok(identifier) = identifiers.extract::<String>() {
            Some(vec![identifier])
        } else {
            Some(identifiers.extract::<Vec<String>>()?)
        };
        filters.push(StreamingRecordFilterConfig {
            record_type,
            identifiers,
        });
    }

    Ok((!filters.is_empty()).then_some(filters))
}

fn positive_interval(interval_ns: Option<u64>) -> PyResult<DurationNanos> {
    let interval_ns = interval_ns.unwrap_or(NANOSECONDS_IN_DAY);
    if interval_ns == 0 {
        return Err(to_pyvalue_err("rotation_interval_ns must be positive"));
    }
    Ok(DurationNanos::new(interval_ns))
}
