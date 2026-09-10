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

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow::{
    array::{FixedSizeBinaryArray, UInt64Array},
    datatypes::{DataType, Field, Schema, TimeUnit},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Bar, Data, OrderBookDepth, QuoteTick, TradeTick},
    events::AccountState,
};
use nautilus_persistence::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        migration::{ParquetMigrationConfig, migrate_parquet_catalog},
    },
    test_data::RustTestCustomData,
};
use nautilus_serialization::{arrow::DecodeTypedFromRecordBatch, ensure_custom_data_registered};
use object_store::local::LocalFileSystem;
use parquet::arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder};
use rstest::rstest;
use serde_json::Value;
use tempfile::TempDir;

#[rstest]
fn develop_catalog_migrates_to_final_arrow_without_changing_source() {
    ensure_custom_data_registered::<RustTestCustomData>();
    let source = fixture_path();
    let original = catalog_files(&source);
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("migrated");
    let report = migrate_parquet_catalog(config(&source, &target, false)).unwrap();
    assert_eq!(report.migrated_files, 7);
    assert_eq!(report.migrated_rows, 8);
    assert_eq!(report.skipped_files, 0);
    assert_eq!(catalog_files(&source), original);

    let expected: Value =
        serde_json::from_slice(&fs::read(source.join("expected.json")).unwrap()).unwrap();
    let mut catalog = ParquetDataCatalog::new(&target, None, None, None, None);
    macro_rules! check_rows {
        ($ty:ty, $key:literal, $many:expr) => {{
            let actual = catalog
                .query_typed_data::<$ty>(None, None, None, None, None, true)
                .unwrap();
            let expected_rows = if $many {
                expected[$key].clone()
            } else {
                Value::Array(vec![expected[$key].clone()])
            };
            let expected_rows: Vec<$ty> = serde_json::from_value(expected_rows).unwrap();
            assert_eq!(actual, expected_rows);
        }};
    }
    check_rows!(QuoteTick, "quotes", true);
    check_rows!(TradeTick, "trade", false);
    check_rows!(Bar, "bar", false);
    let mut expected_depth: OrderBookDepth =
        serde_json::from_value(expected["depth"].clone()).unwrap();
    // Develop's fixed-depth Arrow schema omits order IDs and decodes them as zero.
    for order in expected_depth
        .bids
        .iter_mut()
        .chain(expected_depth.asks.iter_mut())
    {
        order.order_id = 0;
    }
    let depths = catalog
        .query_typed_data::<OrderBookDepth>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(depths, vec![expected_depth]);
    let instruments = catalog.query_instruments(None).unwrap();
    assert_eq!(
        serde_json::to_value(&instruments).unwrap(),
        Value::Array(vec![expected["instrument"].clone()])
    );
    let batches = catalog
        .query_record_batches("account_state", None, None, None, None, true)
        .unwrap();
    let accounts = batches
        .into_iter()
        .flat_map(|batch| {
            let schema = batch.schema();
            AccountState::decode_typed_batch(schema.metadata(), batch).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(accounts).unwrap(),
        Value::Array(vec![expected["account_state"].clone()])
    );
    let custom = catalog
        .query_custom_data_dynamic("RustTestCustomData", None, None, None, None, None, true)
        .unwrap();
    assert_eq!(custom.len(), 1);
    let Data::Custom(custom) = &custom[0] else {
        panic!("Expected custom data");
    };
    let custom = custom
        .data
        .as_any()
        .downcast_ref::<RustTestCustomData>()
        .unwrap();
    assert_eq!(serde_json::to_value(custom).unwrap(), expected["custom"]);

    assert!(target.join("data/custom/RustTestCustomData").is_dir());
    for (relative, _) in catalog_files(&target) {
        if relative
            .extension()
            .is_none_or(|extension| extension != "parquet")
        {
            continue;
        }
        let reader = ParquetRecordBatchReaderBuilder::try_new(
            fs::File::open(target.join(&relative)).unwrap(),
        )
        .unwrap();
        let schema = reader.schema();
        assert!(schema.index_of("_nautilus_cluster_key").is_err());

        for name in ["ts_event", "ts_init"] {
            assert_eq!(
                schema.field_with_name(name).unwrap().data_type(),
                &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                "{}: {name}",
                relative.display()
            );
        }

        if relative.starts_with("data/account_state") {
            for name in ["balances", "margins", "info"] {
                assert_eq!(
                    schema
                        .field_with_name(name)
                        .unwrap()
                        .metadata()
                        .get("ARROW:extension:name")
                        .map(String::as_str),
                    Some("arrow.json")
                );
            }
        }

        if relative.starts_with("data/quotes") {
            for name in ["bid_price", "ask_price", "bid_size", "ask_size"] {
                assert_eq!(
                    schema.field_with_name(name).unwrap().data_type(),
                    &DataType::Decimal128(38, 16)
                );
            }
        }

        if relative.starts_with("data/trades") {
            assert_eq!(
                schema
                    .field_with_name("aggressor_side")
                    .unwrap()
                    .data_type(),
                &DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8))
            );
        }

        if relative.starts_with("data/order_book_depths") {
            for name in ["bids", "asks"] {
                assert!(matches!(
                    schema.field_with_name(name).unwrap().data_type(),
                    DataType::List(_)
                ));
            }
        }
    }
}

#[rstest]
fn migration_dry_run_does_not_create_destination() {
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("absent");
    let report = migrate_parquet_catalog(config(&fixture_path(), &target, true)).unwrap();
    assert_eq!(report.migrated_files, 0);
    assert!(!target.exists());
}

#[rstest]
fn migration_rejects_nonempty_destination() {
    let temporary = TempDir::new().unwrap();
    let marker = temporary.path().join("keep.txt");
    fs::write(&marker, b"unchanged").unwrap();
    assert!(migrate_parquet_catalog(config(&fixture_path(), temporary.path(), false)).is_err());
    assert_eq!(fs::read(marker).unwrap(), b"unchanged");
}

#[rstest]
#[case::canonical("quotes", "quotes")]
#[case::legacy_quote("quote_tick", "quotes")]
#[case::legacy_depth("order_book_depth10", "order_book_depths")]
#[case::instrument("currency_pair", "currency_pair")]
#[case::legacy_custom("custom_Feed", "custom/Feed")]
fn migration_preserves_empty_coverage_files(#[case] source_type: &str, #[case] target_type: &str) {
    let temporary = TempDir::new().unwrap();
    let source = temporary.path().join("source");
    let target = temporary.path().join("target");
    let relative = format!(
        "data/{source_type}/AUDUSD.SIM/2023-11-14T22-13-20-000000123Z_2023-11-14T22-13-20-000000126Z.parquet"
    );
    let expected_relative = format!(
        "data/{target_type}/AUDUSD.SIM/2023-11-14T22-13-20-000000123Z_2023-11-14T22-13-20-000000126Z.parquet"
    );
    let marker = source.join(&relative);
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(&marker, []).unwrap();
    let report = migrate_parquet_catalog(config(&source, &target, false)).unwrap();
    assert_eq!(report.migrated_files, 1);
    assert_eq!(report.migrated_rows, 0);
    assert_eq!(fs::read(&marker).unwrap(), Vec::<u8>::new());
    assert_eq!(
        fs::read(target.join(expected_relative)).unwrap(),
        Vec::<u8>::new()
    );
    let catalog = ParquetDataCatalog::new(&target, None, None, None, None);
    assert_eq!(
        catalog
            .get_intervals(target_type, Some("AUDUSD.SIM"))
            .unwrap(),
        vec![(1_700_000_000_000_000_123, 1_700_000_000_000_000_126)]
    );
}

#[rstest]
fn migration_rejects_overlapping_locations() {
    let source = fixture_path();
    for target in [
        source.clone(),
        source.join("nested"),
        source.parent().unwrap().to_path_buf(),
    ] {
        let error = migrate_parquet_catalog(config(&source, &target, false)).unwrap_err();
        assert!(error.to_string().contains("non-overlapping"), "{error}");
    }
}

#[rstest]
fn migration_rejects_opaque_fixed_columns_before_writing() {
    let temporary = TempDir::new().unwrap();
    let source = temporary.path().join("source");
    let target = temporary.path().join("destination");
    let file = source.join("data/custom/OpaqueValue/TEST/value.parquet");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let schema = Arc::new(Schema::new_with_metadata(
        vec![
            Field::new("value", DataType::FixedSizeBinary(16), false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ],
        HashMap::from([("type_name".to_string(), "OpaqueValue".to_string())]),
    ));
    let value = [17_u8; 16];
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(FixedSizeBinaryArray::try_from_iter([value.as_slice()].into_iter()).unwrap()),
            Arc::new(UInt64Array::from(vec![11])),
            Arc::new(UInt64Array::from(vec![13])),
        ],
    )
    .unwrap();
    let mut writer = ArrowWriter::try_new(fs::File::create(&file).unwrap(), schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    let original = fs::read(&file).unwrap();
    let error = migrate_parquet_catalog(config(&source, &target, false)).unwrap_err();
    assert!(
        error.to_string().contains("No final-format transcoder"),
        "{error}"
    );
    assert!(!target.exists());
    assert_eq!(fs::read(file).unwrap(), original);
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/nautilus/catalog_develop/128-bit")
}

fn config(source: &Path, target: &Path, dry_run: bool) -> ParquetMigrationConfig {
    ParquetMigrationConfig {
        source_uri: source.to_str().unwrap().to_string(),
        target_uri: target.to_str().unwrap().to_string(),
        source_options: Vec::new(),
        target_options: Vec::new(),
        dry_run,
    }
}

fn catalog_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

#[rstest]
#[case::bucket_root("")]
#[case::catalog_prefix("catalog")]
fn remote_migration_preserves_encoded_object_paths(#[case] prefix: &str) {
    let storage = TempDir::new().unwrap();
    let destination = TempDir::new().unwrap();
    let fixture = catalog_files(&fixture_path())
        .into_iter()
        .find(|(path, _)| path.to_string_lossy().contains("data/quotes/"))
        .unwrap();
    let filename = fixture.0.file_name().unwrap();
    let relative = Path::new(prefix)
        .join("data/quotes/AUD%2FUSD.SIM")
        .join(filename);
    let source_file = storage.path().join(&relative);
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(&source_file, &fixture.1).unwrap();
    let mut source = ParquetDataCatalog::new(storage.path(), None, None, None, None);
    source.base_path = prefix.to_string();
    source.original_uri = format!("s3://test-bucket/{prefix}");
    source.object_store = Arc::new(LocalFileSystem::new_with_prefix(storage.path()).unwrap());
    let mut target = ParquetDataCatalog::new(destination.path(), None, None, None, None);

    let report = target.migrate_from_legacy_parquet_catalog(&source).unwrap();
    let actual = target
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    let expected: Value =
        serde_json::from_slice(&fs::read(fixture_path().join("expected.json")).unwrap()).unwrap();
    let expected: Vec<QuoteTick> = serde_json::from_value(expected["quotes"].clone()).unwrap();
    assert_eq!(report.migrated_files, 1);
    assert_eq!(report.migrated_rows, 2);
    assert_eq!(actual, expected);
    assert_eq!(fs::read(source_file).unwrap(), fixture.1);
}
