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
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const DIRECT_VERSION_PACKAGES: [(&str, &str); 5] = [
    ("libc", "NAUTILUS_BUILD_LIBC_VERSION"),
    ("pyo3", "NAUTILUS_BUILD_PYO3_VERSION"),
    (
        "pyo3-async-runtimes",
        "NAUTILUS_BUILD_PYO3_ASYNC_RUNTIMES_VERSION",
    ),
    ("rust_decimal", "NAUTILUS_BUILD_RUST_DECIMAL_VERSION"),
    ("tokio", "NAUTILUS_BUILD_TOKIO_VERSION"),
];

const LOCK_VERSION_PACKAGES: [(&str, &str); 4] = [
    ("aws-lc-rs", "NAUTILUS_BUILD_AWS_LC_RS_VERSION"),
    ("mimalloc", "NAUTILUS_BUILD_MIMALLOC_VERSION"),
    ("redb", "NAUTILUS_BUILD_REDB_VERSION"),
    ("rustls", "NAUTILUS_BUILD_RUSTLS_VERSION"),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!(
        "cargo:rustc-env=NAUTILUS_BUILD_RUSTC_VERSION={}",
        rustc_version(),
    );
    println!(
        "cargo:rustc-env=NAUTILUS_BUILD_TARGET={}",
        env::var("TARGET").unwrap_or_default(),
    );
    println!(
        "cargo:rustc-env=NAUTILUS_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap_or_default(),
    );
    println!("cargo:rustc-env=NAUTILUS_BUILD_GIT_COMMIT={}", git_commit());

    let Some(lock_path) = find_ancestor_file("Cargo.lock") else {
        emit_unavailable_lock_metadata();
        return;
    };
    println!("cargo:rerun-if-changed={}", lock_path.display());

    let lock_bytes = fs::read(&lock_path).expect("failed to read Cargo.lock");
    let lock_contents = std::str::from_utf8(&lock_bytes).expect("Cargo.lock is not valid UTF-8");
    let lock = toml::from_str::<toml::Value>(lock_contents).expect("failed to parse Cargo.lock");
    let fingerprint = crc32_ieee(&lock_bytes);
    println!("cargo:rustc-env=NAUTILUS_BUILD_CARGO_LOCK_CRC32=crc32:{fingerprint:08x}");

    let current_package = env::var("CARGO_PKG_NAME").unwrap_or_default();
    let current_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();

    for (package, variable) in DIRECT_VERSION_PACKAGES {
        let version = resolved_package_version(&lock, &current_package, &current_version, package)
            .unwrap_or_default();
        println!("cargo:rustc-env={variable}={version}");
    }

    for (package, variable) in LOCK_VERSION_PACKAGES {
        let version = resolved_unique_package_version(&lock, package).unwrap_or_default();
        println!("cargo:rustc-env={variable}={version}");
    }
}

fn find_ancestor_file(name: &str) -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR")?);
    manifest_dir
        .ancestors()
        .map(|path| path.join(name))
        .find(|path| path.is_file())
}

fn emit_unavailable_lock_metadata() {
    println!("cargo:rustc-env=NAUTILUS_BUILD_CARGO_LOCK_CRC32=");

    for (_, variable) in DIRECT_VERSION_PACKAGES
        .into_iter()
        .chain(LOCK_VERSION_PACKAGES)
    {
        println!("cargo:rustc-env={variable}=");
    }
}

fn git_commit() -> String {
    let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) else {
        return String::new();
    };

    if let Some(git_dir) = git_output(&manifest_dir, &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
    }

    if let Some(common_dir) = git_output(&manifest_dir, &["rev-parse", "--git-common-dir"])
        .map(|path| resolve_path(&manifest_dir, &path))
    {
        println!(
            "cargo:rerun-if-changed={}",
            common_dir.join("packed-refs").display()
        );

        if let Some(reference) = git_output(&manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
            println!(
                "cargo:rerun-if-changed={}",
                common_dir.join(reference).display(),
            );
        }
    }

    git_output(&manifest_dir, &["rev-parse", "HEAD"])
        .filter(|commit| commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_default()
}

fn git_output(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_path(directory: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        directory.join(path)
    }
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 == 0 {
                crc >>= 1;
            } else {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            }
        }
    }
    !crc
}

fn rustc_version() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let Ok(output) = Command::new(rustc).arg("--version").output() else {
        return String::new();
    };

    if !output.status.success() {
        return String::new();
    }

    String::from_utf8(output.stdout)
        .map(|value| {
            value
                .trim()
                .strip_prefix("rustc ")
                .unwrap_or(value.trim())
                .to_string()
        })
        .unwrap_or_default()
}

fn resolved_package_version(
    lock: &toml::Value,
    current_package: &str,
    current_version: &str,
    package: &str,
) -> Option<String> {
    let packages = lock.get("package")?.as_array()?;
    let current = packages.iter().find(|entry| {
        entry.get("name").and_then(toml::Value::as_str) == Some(current_package)
            && entry.get("version").and_then(toml::Value::as_str) == Some(current_version)
    })?;
    let dependency = current
        .get("dependencies")?
        .as_array()?
        .iter()
        .filter_map(toml::Value::as_str)
        .find(|dependency| {
            *dependency == package
                || dependency
                    .strip_prefix(package)
                    .is_some_and(|suffix| suffix.starts_with(' '))
        })?;
    let specified_version = dependency.strip_prefix(package)?.split_whitespace().next();

    if let Some(version) = specified_version {
        return packages
            .iter()
            .any(|entry| {
                entry.get("name").and_then(toml::Value::as_str) == Some(package)
                    && entry.get("version").and_then(toml::Value::as_str) == Some(version)
            })
            .then(|| version.to_string());
    }

    resolved_unique_package_version(lock, package)
}

fn resolved_unique_package_version(lock: &toml::Value, package: &str) -> Option<String> {
    let packages = lock.get("package")?.as_array()?;
    let mut versions = packages.iter().filter_map(|entry| {
        (entry.get("name").and_then(toml::Value::as_str) == Some(package))
            .then(|| entry.get("version").and_then(toml::Value::as_str))
            .flatten()
    });
    let version = versions.next()?;
    versions.next().is_none().then(|| version.to_string())
}
