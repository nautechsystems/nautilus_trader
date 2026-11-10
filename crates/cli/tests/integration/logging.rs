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

//! Log file output from the `nautilus` binary.

#![warn(clippy::clone_on_ref_ptr)]

use std::{fs, process::Command};

use rstest::rstest;
use tempfile::TempDir;

// The lazy logger is never dropped, so this line reaches the file only via `logging_shutdown`
#[rstest]
fn failed_command_writes_error_to_log_file() {
    let temporary = TempDir::new().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nautilus"))
        .args([
            "catalog",
            "migrate-parquet",
            "missing-source",
            "destination",
            "--dry-run",
        ])
        .current_dir(temporary.path())
        .env("NAUTILUS_LOG", "stdout=Off;fileout=Error")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let log_contents: String = fs::read_dir(temporary.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "log"))
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert!(
        log_contents.contains("Error executing Nautilus CLI"),
        "log file contents: {log_contents:?}"
    );
}

#[cfg(unix)]
#[rstest]
fn unwritable_log_directory_falls_back_to_console_logging() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = TempDir::new().unwrap();
    let read_only = temporary.path().join("read-only");
    fs::create_dir(&read_only).unwrap();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();

    // Privileged users bypass directory permissions, so the failure cannot be simulated
    if fs::write(read_only.join("probe"), "").is_ok() {
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_nautilus"))
        .args([
            "catalog",
            "migrate-parquet",
            "missing-source",
            "destination",
            "--dry-run",
        ])
        .current_dir(&read_only)
        .env("NAUTILUS_LOG", "stdout=Off;fileout=Error")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("Continuing without file logging: failed to open log file "),
        "stderr: {stderr:?}"
    );
    assert!(
        stderr.contains("Error executing Nautilus CLI"),
        "stderr: {stderr:?}"
    );
}
