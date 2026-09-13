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

use std::{path::Path, process::Command};

use rstest::rstest;
use tempfile::TempDir;

#[rstest]
fn parquet_migration_command_validates_then_converts_catalog() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/nautilus/catalog_develop/128-bit");
    let temporary = TempDir::new().unwrap();
    let target = temporary.path().join("destination");
    let run = |dry_run: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nautilus"));
        command
            .args(["catalog", "migrate-parquet"])
            .arg(&source)
            .arg(&target)
            .current_dir(temporary.path());

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
