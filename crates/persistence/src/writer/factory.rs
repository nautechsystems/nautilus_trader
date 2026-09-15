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

//! Streaming writer factory registry and connections.
use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_common::live::block_on_nautilus_with;
use nautilus_core::Params;
use object_store::{ObjectStoreExt, path::Path as ObjectPath};

use super::{
    feather::{RotationConfig, WriterClock},
    filter::WriterRecordFilter,
    traits::StreamingSinkBox,
};
use crate::common::{backend_name::backend_type, storage::create_storage_backend_from_path};

/// Built-in Feather streaming writer registry key.
pub const FEATHER_WRITER_FACTORY_NAME: &str = "Feather";

/// Built-in Parquet streaming writer registry key.
pub const PARQUET_WRITER_FACTORY_NAME: &str = "Parquet";

backend_type!(
    /// Streaming writer backend used to persist run data, mirroring `CatalogBackendType`.
    WriterBackendType {
        Feather => FEATHER_WRITER_FACTORY_NAME,
        Parquet => PARQUET_WRITER_FACTORY_NAME,
    }
);

/// Connection settings handed to writer factories.
#[derive(Clone, Debug)]
pub struct WriterConnectConfig {
    /// Run-session storage URI the writer stages into.
    pub uri: String,
    /// Backend-specific storage options (credentials, endpoints).
    pub storage_options: Option<AHashMap<String, String>>,
    /// Rotation settings used by built-in streaming writer backends.
    pub rotation_config: RotationConfig,
    /// Optional automatic flush interval in milliseconds.
    pub flush_interval_ms: Option<u64>,
    /// Optional record-family and identifier filter.
    pub record_filter: Option<WriterRecordFilter>,
    /// Backend-specific writer parameters.
    pub params: Option<Params>,
}

impl WriterConnectConfig {
    /// Creates a connect config for the given URI.
    #[must_use]
    pub fn new(uri: impl Into<String>, storage_options: Option<AHashMap<String, String>>) -> Self {
        Self {
            uri: uri.into(),
            storage_options,
            rotation_config: RotationConfig::NoRotation,
            flush_interval_ms: None,
            record_filter: None,
            params: None,
        }
    }
}

/// Factory constructing a streaming writer from connection settings and a clock.
pub type WriterFactory =
    Arc<dyn Fn(&WriterConnectConfig, WriterClock) -> anyhow::Result<StreamingSinkBox>>;

/// Ordered registry of writer factories keyed by name.
pub type WriterFactoryRegistry = IndexMap<String, WriterFactory>;

/// Resolves a writer backend through the factory registry and constructs the writer.
///
/// All variants resolve through the registry so built-ins and custom names share one code path.
///
/// # Errors
///
/// Returns an error if the backend's factory is not registered or construction fails.
pub fn create_writer(
    backend: &WriterBackendType,
    config: &WriterConnectConfig,
    clock: WriterClock,
    factories: &WriterFactoryRegistry,
) -> anyhow::Result<StreamingSinkBox> {
    let name = backend.to_string();
    factories
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("No writer factory registered for '{name}'"))?(
        config, clock
    )
}

/// Deletes existing objects below writer connection URI.
///
/// # Errors
///
/// Returns an error if storage cannot be opened or listing/deleting objects fails.
pub fn replace_existing_writer_data(config: &WriterConnectConfig) -> anyhow::Result<()> {
    let storage = create_storage_backend_from_path(&config.uri, config.storage_options.clone())?;
    block_on_nautilus_with(|| async {
        for path in storage.list_files("", None).await? {
            storage.object_store.delete(&ObjectPath::from(path)).await?;
        }
        Ok::<(), anyhow::Error>(())
    })
}
#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Data, QuoteTick},
        identifiers::InstrumentId,
        instruments::{InstrumentAny, NautilusInstrumentType},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{backend::default_writer_factories, common::paths::CatalogPathPrefix};

    fn quote(ts_init: u64) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("0.66"),
            Price::from("0.67"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )
    }

    #[rstest]
    fn replace_existing_writer_data_removes_local_files() {
        let directory = TempDir::new().unwrap();
        let stale_file = directory.path().join("stale.feather");
        std::fs::write(&stale_file, b"stale").unwrap();
        let config =
            WriterConnectConfig::new(format!("file://{}", directory.path().display()), None);

        replace_existing_writer_data(&config).unwrap();

        assert!(!stale_file.exists());
    }

    #[rstest]
    fn writer_record_filter_allows_empty_and_record_family_filters() {
        let empty = WriterRecordFilter::new();
        assert!(empty.contains_prefix("quotes"));
        assert!(empty.allows("quotes", None, None));
        assert!(empty.allows("trades", Some("AUD/USD.SIM"), None));

        let mut filter = WriterRecordFilter::new();
        filter.insert_prefix("quotes", None);
        assert!(filter.contains_prefix("quotes"));
        assert!(!filter.contains_prefix("trades"));
        assert!(filter.allows("quotes", None, None));
        assert!(filter.allows("quotes", Some("AUD/USD.SIM"), None));
        assert!(!filter.allows("trades", Some("AUD/USD.SIM"), None));
    }

    #[rstest]
    fn writer_record_filter_restricts_by_instrument_type() {
        let mut filter = WriterRecordFilter::new();
        filter.insert_instrument_type(&NautilusInstrumentType::FuturesContract);

        assert!(filter.contains_prefix(InstrumentAny::path_prefix()));
        assert!(filter.allows(
            InstrumentAny::path_prefix(),
            Some("ESM4.GLBX"),
            Some("FuturesContract")
        ));
        assert!(!filter.allows(
            InstrumentAny::path_prefix(),
            Some("AAPL.XNAS"),
            Some("Equity")
        ));
        assert!(!filter.allows("quotes", Some("ESM4.GLBX"), None));
    }

    #[rstest]
    fn writer_record_filter_allows_data_and_instrument_type_union() {
        let mut filter = WriterRecordFilter::new();
        filter.insert_prefix("bars", None);
        filter.insert_instrument_type(&NautilusInstrumentType::FuturesContract);

        assert!(filter.allows("bars", Some("ESM4.GLBX"), None));
        assert!(filter.allows(
            InstrumentAny::path_prefix(),
            Some("ESM4.GLBX"),
            Some("FuturesContract")
        ));
        assert!(!filter.allows(
            InstrumentAny::path_prefix(),
            Some("AAPL.XNAS"),
            Some("Equity")
        ));
    }

    #[rstest]
    fn writer_record_filter_restricts_by_identifier() {
        let mut filter = WriterRecordFilter::new();
        filter.insert_prefix("quotes", Some(vec!["AUD/USD.SIM".to_string()]));

        assert!(filter.contains_prefix("quotes"));
        assert!(filter.allows("quotes", Some("AUD/USD.SIM"), None));
        assert!(!filter.allows("quotes", Some("GBP/USD.SIM"), None));
        assert!(!filter.allows("quotes", None, None));
        assert!(!filter.allows("trades", Some("AUD/USD.SIM"), None));
    }

    #[rstest]
    fn writer_backend_type_parses_built_ins_case_insensitively() {
        assert_eq!(
            "feather".parse::<WriterBackendType>().unwrap(),
            WriterBackendType::Feather,
        );
        assert_eq!(
            "parquet".parse::<WriterBackendType>().unwrap(),
            WriterBackendType::Parquet,
        );
        assert_eq!(
            "parquet".parse::<WriterBackendType>().unwrap(),
            WriterBackendType::Parquet,
        );
        assert_eq!(WriterBackendType::Feather.to_string(), "Feather");
        assert_eq!(WriterBackendType::Parquet.to_string(), "Parquet");
        assert_eq!(WriterBackendType::Parquet.to_string(), "Parquet");
    }

    #[rstest]
    fn writer_backend_type_preserves_custom_factory_case() {
        assert_eq!(
            "CustomSink".parse::<WriterBackendType>().unwrap(),
            WriterBackendType::External("CustomSink".to_string()),
        );
        assert!("".parse::<WriterBackendType>().is_err());
    }

    #[rstest]
    fn default_registry_contains_built_in_writer_factories() {
        let registry = default_writer_factories();
        assert!(registry.contains_key(FEATHER_WRITER_FACTORY_NAME));
    }

    #[rstest]
    fn create_writer_builds_feather_writer_through_registry() {
        let temp_dir = TempDir::new().unwrap();
        let config = WriterConnectConfig::new(temp_dir.path().to_str().unwrap(), None);
        let registry = default_writer_factories();

        let mut writer = create_writer(
            &WriterBackendType::Feather,
            &config,
            WriterClock::Live,
            &registry,
        )
        .unwrap();
        writer.write_data(Data::Quote(quote(100))).unwrap();
        writer.flush().unwrap();
        writer.close().unwrap();

        let feather_files = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "feather")
            })
            .count();
        assert_eq!(feather_files, 1);
    }

    #[rstest]
    #[case("backtest")]
    #[case("live")]
    fn create_writer_builds_feather_writer_for_run_session_kind(#[case] run_kind: &str) {
        let temp_dir = TempDir::new().unwrap();
        let session = temp_dir.path().join(run_kind).join("run-001");
        let config = WriterConnectConfig::new(session.to_str().unwrap(), None);
        let registry = default_writer_factories();

        let mut writer = create_writer(
            &WriterBackendType::Feather,
            &config,
            WriterClock::Live,
            &registry,
        )
        .unwrap();
        writer.write_data(Data::Quote(quote(100))).unwrap();
        writer.close().unwrap();

        let feather_files = std::fs::read_dir(&session)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "feather")
            })
            .count();
        assert_eq!(feather_files, 1);
    }

    #[rstest]
    fn create_writer_errors_for_missing_external() {
        let temp_dir = TempDir::new().unwrap();
        let session = temp_dir.path().join("backtest").join("run-001");
        let config = WriterConnectConfig::new(session.to_str().unwrap(), None);
        let registry = default_writer_factories();

        let result = create_writer(
            &WriterBackendType::External("Missing".to_string()),
            &config,
            WriterClock::Live,
            &registry,
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No writer factory registered for 'Missing'"),
        );
    }
}
