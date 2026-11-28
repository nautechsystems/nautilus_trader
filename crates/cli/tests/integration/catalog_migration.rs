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

#![warn(clippy::clone_on_ref_ptr)]

use std::{path::Path, process::Command};

use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
fn parquet_migration_command_validates_then_converts_catalog() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/nautilus/catalog_develop/128-bit");
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("destination");
    let run = |dry_run: bool| {
        let mut command = migration_command(temporary.path(), &source, &target);
        if dry_run {
            command.arg("--dry-run");
        }
        command.output().unwrap()
    };
    let dry_run = run(true);
    assert!(
        dry_run.status.success(),
        "{}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert!(!target.exists());
    let migrated = run(false);
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    assert!(
        String::from_utf8_lossy(&migrated.stdout).contains("7 migrated files, 8 migrated rows")
    );
    assert!(target.join("data/custom/RustTestCustomData").is_dir());
    let repeated = run(false);
    assert!(!repeated.status.success());
}

#[rstest]
#[cfg_attr(not(feature = "high-precision"), ignore = "requires high-precision")]
fn parquet_migration_preserves_high_precision_quote() {
    let temporary = TempDir::new().unwrap();
    let source = TempDir::new_in(temporary.path()).unwrap();
    let target = temporary.path().join("destination");

    let quote = QuoteTick::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        Price::from("1.0000000000000001"),
        Price::from("2.0000000000000003"),
        Quantity::from("3.0000000000000005"),
        Quantity::from("4.0000000000000007"),
        100.into(),
        200.into(),
    );

    let source_catalog = ParquetDataCatalog::builder()
        .uri(source.path())
        .build()
        .unwrap();
    source_catalog
        .write_to_parquet(&[quote], None, None, None)
        .unwrap();

    let output = migration_command(temporary.path(), source.path(), &target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut migrated = ParquetDataCatalog::builder().uri(&target).build().unwrap();
    let quotes = migrated.query::<QuoteTick>().execute().unwrap();
    assert_eq!(quotes, [quote]);
}

fn migration_command(directory: &Path, source: &Path, target: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nautilus"));
    command
        .args(["catalog", "migrate-parquet"])
        .arg(source)
        .arg(target)
        .current_dir(directory);
    command
}
