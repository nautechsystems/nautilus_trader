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

use std::{fs, process::Command};

use rstest::rstest;

#[rstest]
fn test_enum_strum_serde_compiles_with_renamed_serde() {
    let temp_dir = tempfile::tempdir().unwrap();
    let source_dir = temp_dir.path().join("src");
    fs::create_dir(&source_dir).unwrap();

    let model_path = env!("CARGO_MANIFEST_DIR").replace('\\', "/");
    let manifest = format!(
        r#"[package]
name = "serde-renamed-consumer"
version = "0.0.0"
edition = "2024"

[workspace]

[dependencies]
nautilus-model = {{ path = "{model_path}" }}
renamed-serde = {{ package = "serde", version = "1" }}
"#,
    );
    fs::write(temp_dir.path().join("Cargo.toml"), manifest).unwrap();
    fs::write(
        source_dir.join("main.rs"),
        include_str!("../test_data/serde_renamed/main.rs"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO"))
        .args([
            "check",
            "--offline",
            "--quiet",
            "--jobs",
            "2",
            "--manifest-path",
        ])
        .arg(temp_dir.path().join("Cargo.toml"))
        .arg("--target-dir")
        .arg(temp_dir.path().join("target"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "renamed serde consumer failed to compile\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
