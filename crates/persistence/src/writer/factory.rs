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
    fs, io,
    sync::Arc,
};

use indexmap::IndexMap;
use nautilus_core::Params;

use super::{
    feather::{RotationConfig, WriterClock},
    filter::WriterRecordFilter,
    traits::StreamingDataSink,
};
use crate::{
    catalog::factory::CatalogConnectConfig,
    common::{backend_name::backend_type, paths::local_writer_directory},
};

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
    /// Local run-session directory the writer appends Feather files to.
    pub uri: String,
    /// Catalog that receives promoted data; required by every backend except `Feather`.
    pub catalog: Option<CatalogConnectConfig>,
    /// Rotation settings used by built-in streaming writer backends.
    pub rotation_config: RotationConfig,
    /// Optional automatic flush interval in milliseconds.
    pub flush_interval_ms: Option<u64>,
    /// Optional record-family and identifier filter.
    pub record_filter: Option<WriterRecordFilter>,
    /// Interval in milliseconds for promoting sealed files into `catalog`.
    pub promotion_interval_ms: Option<u64>,
    /// Whether closing the writer promotes remaining files into `catalog`.
    pub promote_on_close: bool,
    /// Whether Feather files are deleted after a successful promotion.
    pub delete_feather_after_promotion: bool,
    /// Whether promotion replaces `ts_init` with `ts_event`.
    pub use_ts_event_for_ts_init: bool,
    /// Backend-specific writer parameters.
    pub params: Option<Params>,
}

impl WriterConnectConfig {
    /// Creates a connect config for the given writer directory and promotion catalog.
    #[must_use]
    pub fn new(uri: impl Into<String>, catalog: Option<CatalogConnectConfig>) -> Self {
        Self {
            uri: uri.into(),
            catalog,
            rotation_config: RotationConfig::NoRotation,
            flush_interval_ms: None,
            record_filter: None,
            promotion_interval_ms: None,
            promote_on_close: true,
            delete_feather_after_promotion: false,
            use_ts_event_for_ts_init: false,
            params: None,
        }
    }
}

/// Factory constructing a streaming writer from connection settings and a clock.
pub type WriterFactory =
    Arc<dyn Fn(&WriterConnectConfig, WriterClock) -> anyhow::Result<StreamingDataSink>>;

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
) -> anyhow::Result<StreamingDataSink> {
    match backend {
        WriterBackendType::Feather => factories
            .get(FEATHER_WRITER_FACTORY_NAME)
            .ok_or_else(|| anyhow::anyhow!("Feather writer factory missing from registry"))?(
            config, clock,
        ),
        WriterBackendType::Parquet => factories
            .get(PARQUET_WRITER_FACTORY_NAME)
            .ok_or_else(|| anyhow::anyhow!("Parquet writer factory missing from registry"))?(
            config, clock,
        ),
        WriterBackendType::External(name) => factories
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("No writer factory registered for '{name}'"))?(
            config, clock,
        ),
    }
}

impl WriterConnectConfig {
    /// Returns the catalog that receives promoted data.
    ///
    /// # Errors
    ///
    /// Returns an error naming `backend` when no catalog is configured.
    pub fn required_catalog(&self, backend: &str) -> anyhow::Result<&CatalogConnectConfig> {
        self.catalog
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("{backend} writer requires a promotion catalog"))
    }
}

/// Deletes existing files below the writer directory.
///
/// # Errors
///
/// Returns an error if the writer directory is not local or deleting it fails.
pub fn replace_existing_writer_data(config: &WriterConnectConfig) -> anyhow::Result<()> {
    let directory = local_writer_directory(&config.uri)?;
    match fs::remove_dir_all(&directory) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use nautilus_common::enums::Environment;
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Data, InstrumentClose, InstrumentStatus, NautilusDataType, QuoteTick},
        enums::{InstrumentCloseType, MarketStatusAction},
        identifiers::InstrumentId,
        instruments::{
            InstrumentAny, NautilusInstrumentType,
            stubs::{audusd_sim, futures_contract_es},
        },
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backend::{default_writer_factories, parquet::catalog::ParquetDataCatalog},
        catalog::types::CatalogDataType,
        writer::feather::FEATHER_PARTIAL_EXTENSION,
    };

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
        let quotes = CatalogDataType::from(NautilusDataType::QuoteTick);
        let trades = CatalogDataType::from(NautilusDataType::TradeTick);
        let empty = WriterRecordFilter::new();
        assert!(empty.contains(&quotes));
        assert!(empty.allows(&quotes, None));
        assert!(empty.allows(&trades, Some("AUD/USD.SIM")));

        let mut filter = WriterRecordFilter::new();
        filter.insert(NautilusDataType::QuoteTick, None);
        assert!(filter.contains(&quotes));
        assert!(!filter.contains(&trades));
        assert!(filter.allows(&quotes, None));
        assert!(filter.allows(&quotes, Some("AUD/USD.SIM")));
        assert!(!filter.allows(&trades, Some("AUD/USD.SIM")));
    }

    #[rstest]
    fn writer_record_filter_restricts_by_instrument_type() {
        let mut filter = WriterRecordFilter::new();
        filter.insert_instrument_type(NautilusInstrumentType::FuturesContract);

        assert!(filter.contains(&NautilusDataType::Instrument.into()));
        assert!(filter.allows(
            &CatalogDataType::Instrument(NautilusInstrumentType::FuturesContract),
            Some("ESM4.GLBX"),
        ));
        assert!(!filter.allows(
            &CatalogDataType::Instrument(NautilusInstrumentType::Equity),
            Some("AAPL.XNAS"),
        ));
        assert!(!filter.allows(&NautilusDataType::QuoteTick.into(), Some("ESM4.GLBX")));
    }

    #[rstest]
    fn writer_record_filter_allows_data_and_instrument_type_union() {
        let mut filter = WriterRecordFilter::new();
        filter.insert(NautilusDataType::Bar, None);
        filter.insert_instrument_type(NautilusInstrumentType::FuturesContract);

        assert!(filter.allows(&NautilusDataType::Bar.into(), Some("ESM4.GLBX")));
        assert!(filter.allows(
            &CatalogDataType::Instrument(NautilusInstrumentType::FuturesContract),
            Some("ESM4.GLBX"),
        ));
        assert!(!filter.allows(
            &CatalogDataType::Instrument(NautilusInstrumentType::Equity),
            Some("AAPL.XNAS"),
        ));
    }

    #[rstest]
    fn writer_record_filter_restricts_by_identifier() {
        let quotes = CatalogDataType::from(NautilusDataType::QuoteTick);
        let mut filter = WriterRecordFilter::new();
        filter.insert(
            NautilusDataType::QuoteTick,
            Some(vec!["AUD/USD.SIM".to_string()]),
        );

        assert!(filter.contains(&quotes));
        assert!(filter.allows(&quotes, Some("AUD/USD.SIM")));
        assert!(!filter.allows(&quotes, Some("GBP/USD.SIM")));
        assert!(!filter.allows(&quotes, None));
        assert!(!filter.allows(&NautilusDataType::TradeTick.into(), Some("AUD/USD.SIM")));
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
        assert_eq!(WriterBackendType::Feather.to_string(), "Feather");
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

        assert_eq!(count_feather_files(temp_dir.path()), 1);
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

        assert_eq!(count_feather_files(&session), 1);
    }

    #[rstest]
    fn create_writer_feather_recovers_partial_files_at_startup() {
        let temp_dir = TempDir::new().unwrap();
        let config = WriterConnectConfig::new(temp_dir.path().to_str().unwrap(), None);
        let registry = default_writer_factories();
        let start = || {
            create_writer(
                &WriterBackendType::Feather,
                &config,
                WriterClock::Test(Arc::new(AtomicU64::new(0))),
                &registry,
            )
            .unwrap()
        };

        let mut crashed = start();
        crashed.write_data(Data::Quote(quote(100))).unwrap();
        crashed.close().unwrap();

        // A writer that exited before sealing leaves its flushed stream as a partial file
        let sealed = temp_dir.path().join("quotes").join("quotes_0.feather");
        let partial = sealed.with_extension(FEATHER_PARTIAL_EXTENSION);
        fs::rename(&sealed, &partial).unwrap();

        let mut restarted = start();
        let sealed_at_start = sealed.exists();
        let partial_at_start = partial.exists();
        restarted.close().unwrap();

        assert!(sealed_at_start);
        assert!(!partial_at_start);
        assert_eq!(count_feather_files(temp_dir.path()), 1);
    }

    #[rstest]
    fn create_writer_feather_round_trips_quotes_through_catalog() {
        let temp_dir = TempDir::new().unwrap();
        let session = temp_dir.path().join("backtest").join("run-001");
        let expected = quote(100);
        let config = WriterConnectConfig::new(session.to_str().unwrap(), None);
        let registry = default_writer_factories();

        let mut writer = create_writer(
            &WriterBackendType::Feather,
            &config,
            WriterClock::Live,
            &registry,
        )
        .unwrap();
        writer.write_data(Data::Quote(expected)).unwrap();
        writer.close().unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        catalog
            .convert_stream_to_data(
                "run-001",
                &CatalogDataType::from(NautilusDataType::QuoteTick),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();

        let loaded = catalog
            .query::<QuoteTick>(None, None, None, None, None, true)
            .unwrap();
        assert_eq!(loaded, vec![expected]);
    }

    #[rstest]
    fn create_writer_feather_round_trips_status_and_closes_for_each_instrument() {
        let temp_dir = TempDir::new().unwrap();
        let session = temp_dir.path().join("backtest").join("run-001");
        let first = InstrumentId::from("AUD/USD.SIM");
        let second = InstrumentId::from("GBP/USD.SIM");
        let statuses = vec![
            InstrumentStatus::new(
                first,
                MarketStatusAction::Trading,
                UnixNanos::from(1),
                UnixNanos::from(1),
                None,
                None,
                Some(true),
                None,
                None,
            ),
            InstrumentStatus::new(
                second,
                MarketStatusAction::Halt,
                UnixNanos::from(2),
                UnixNanos::from(2),
                None,
                None,
                Some(false),
                None,
                None,
            ),
        ];
        let closes = vec![
            InstrumentClose::new(
                first,
                Price::from("0.66"),
                InstrumentCloseType::EndOfSession,
                UnixNanos::from(3),
                UnixNanos::from(3),
            ),
            InstrumentClose::new(
                second,
                Price::from("1.250"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::from(4),
                UnixNanos::from(4),
            ),
        ];
        let config = WriterConnectConfig::new(session.to_str().unwrap(), None);
        let registry = default_writer_factories();

        let mut writer = create_writer(
            &WriterBackendType::Feather,
            &config,
            WriterClock::Live,
            &registry,
        )
        .unwrap();

        for status in &statuses {
            writer.write_data(Data::InstrumentStatus(*status)).unwrap();
        }

        for close in &closes {
            writer.write_data(Data::InstrumentClose(*close)).unwrap();
        }

        writer.close().unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);

        for data_type in [
            NautilusDataType::InstrumentStatus,
            NautilusDataType::InstrumentClose,
        ] {
            catalog
                .convert_stream_to_data(
                    "run-001",
                    &CatalogDataType::from(data_type),
                    Environment::Backtest,
                    None,
                    false,
                )
                .unwrap();
        }

        let loaded_statuses = catalog
            .query::<InstrumentStatus>(None, None, None, None, None, true)
            .unwrap();
        let loaded_closes = catalog
            .query::<InstrumentClose>(None, None, None, None, None, true)
            .unwrap();
        assert_eq!(loaded_statuses, statuses);
        assert_eq!(loaded_closes, closes);
    }

    #[rstest]
    fn create_writer_feather_filters_and_converts_instruments_by_type() {
        let temp_dir = TempDir::new().unwrap();
        let session = temp_dir.path().join("backtest").join("run-001");
        let futures = InstrumentAny::FuturesContract(futures_contract_es(None, None));
        let mut record_filter = WriterRecordFilter::new();
        record_filter.insert_instrument_type(NautilusInstrumentType::FuturesContract);
        let mut config = WriterConnectConfig::new(session.to_str().unwrap(), None);
        config.record_filter = Some(record_filter);
        let registry = default_writer_factories();

        let mut writer = create_writer(
            &WriterBackendType::Feather,
            &config,
            WriterClock::Live,
            &registry,
        )
        .unwrap();
        writer
            .write_data(Data::Instrument(Box::new(InstrumentAny::CurrencyPair(
                audusd_sim(),
            ))))
            .unwrap();
        writer
            .write_data(Data::Instrument(Box::new(futures.clone())))
            .unwrap();
        writer.close().unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        catalog
            .convert_stream_to_data(
                "run-001",
                &CatalogDataType::from(NautilusDataType::Instrument),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();

        let loaded = catalog.query_instruments(None).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(vec![futures]).unwrap()
        );
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

    fn count_feather_files(dir: &std::path::Path) -> usize {
        let mut count = 0;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "feather")
                {
                    count += 1;
                }
            }
        }

        count
    }
}
