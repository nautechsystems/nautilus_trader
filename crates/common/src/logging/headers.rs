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

#[cfg(all(target_os = "linux", target_env = "gnu"))]
use std::ffi::CStr;

use nautilus_core::{UUID4, consts::NAUTILUS_VERSION_CORE};
use nautilus_model::identifiers::TraderId;
use sysinfo::System;
use ustr::Ustr;

use crate::{enums::LogColor, logging::log_info};

const GIT_COMMIT_LEN: usize = 12;

const BUILD_VERSIONS: &[(&str, &str)] = &[
    ("git_commit", env!("NAUTILUS_BUILD_GIT_COMMIT")),
    ("rustc", env!("NAUTILUS_BUILD_RUSTC_VERSION")),
    ("target", env!("NAUTILUS_BUILD_TARGET")),
    ("profile", env!("NAUTILUS_BUILD_PROFILE")),
    ("cargo_lock", env!("NAUTILUS_BUILD_CARGO_LOCK_CRC32")),
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    ("libc_crate", env!("NAUTILUS_BUILD_LIBC_VERSION")),
    ("rust_decimal", env!("NAUTILUS_BUILD_RUST_DECIMAL_VERSION")),
    #[cfg(feature = "python")]
    ("pyo3", env!("NAUTILUS_BUILD_PYO3_VERSION")),
];

#[rustfmt::skip]
pub fn log_header(trader_id: TraderId, machine_id: &str, instance_id: UUID4, component: Ustr) {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let c = component;

    let kernel_version = System::kernel_version().map_or(String::new(), |v| format!("kernel-{v} "));
    let os_version = System::long_os_version().unwrap_or_default();
    let pid = std::process::id();

    header_sepr(c, "=================================================================");
    header_sepr(c, " NAUTILUS TRADER - Automated Algorithmic Trading Platform");
    header_sepr(c, " by Nautech Systems Pty Ltd.");
    header_sepr(c, " Copyright (C) 2015-2026. All rights reserved.");
    header_sepr(c, "=================================================================");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣀⣀⡤⠤⠤⠤⠤⠤⠤⠤⢤⡀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠚⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣸⠁⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⢀⣠⠖⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⡴⠚⠁⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⢀⡴⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡞⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⣠⠏⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⠤⠖⠒⠒⠒⠒⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠀⢀⡖⠋⣠⢴⢪⠞⣩⢟⡭⠵⢤⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⣦⡙⠦⣄⡀⠀⢀⣠⠴⠋⢠⡐⣇⢸⡘⢦⡇⣏⡴⣋⡭⠖⠮⢥⡀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⢸⡉⠓⠦⠭⠭⠭⠴⣺⠃⠸⣷⢬⣓⣛⠒⠩⣌⢡⡷⢒⣫⠭⣝⡛⠆⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠙⠦⢤⣀⣠⠤⠞⣡⠞⡆⠺⣭⣭⡷⢇⣷⡻⠡⠾⣛⣒⠦⢤⡙⡆⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠈⠳⣖⠒⠒⠒⠋⠁⣠⠇⡼⢦⠀⣌⡉⢥⣄⣛⡻⢥⡈⠉⢳⡙⠇⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠈⠙⣒⣒⣒⣋⡥⠞⠁⣸⠃⡇⠙⢦⢹⠀⠙⡆⢳⢀⡴⠃⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠈⠙⠒⠦⠤⠴⣚⣡⠞⠁⣠⠏⡼⢀⣠⠇⠞⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_line(c, "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⠉⠉⠑⠚⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    header_sepr(c, "=================================================================");
    header_sepr(c, " SYSTEM SPECIFICATION");
    header_sepr(c, "=================================================================");

    if let Some(cpu) = sys.cpus().first() {
        header_line(c, &format!("CPU architecture: {}", cpu.brand()));
        header_line(c, &format!("CPU(s): {} @ {} MHz", sys.cpus().len(), cpu.frequency()));
    } else {
        header_line(c, "CPU: unknown");
    }
    header_line(c, &format!("OS: {kernel_version}{os_version}"));

    log_sysinfo(component);

    header_sepr(c, "=================================================================");
    header_sepr(c, " IDENTIFIERS");
    header_sepr(c, "=================================================================");
    header_line(c, &format!("trader_id: {trader_id}"));
    header_line(c, &format!("machine_id: {machine_id}"));
    header_line(c, &format!("instance_id: {instance_id}"));
    header_line(c, &format!("PID: {pid}"));

    header_sepr(c, "=================================================================");
    header_sepr(c, " VERSIONING");
    header_sepr(c, "=================================================================");

    #[cfg(not(feature = "python"))]
    log_rust_versioning(c);

    #[cfg(feature = "python")]
    if python_available() {
        log_python_versioning(c);
    } else {
        log_rust_versioning(c);
    }

    header_sepr(c, "=================================================================");
}

#[rustfmt::skip]
fn log_rust_versioning(c: Ustr) {
    header_line(c, &format!("nautilus_trader: {NAUTILUS_VERSION_CORE}"));
    log_build_versioning(c);
    #[cfg(feature = "live")]
    log_tokio_versioning(c);
}

#[cfg(feature = "python")]
#[rustfmt::skip]
fn log_python_versioning(c: Ustr) {
    let package = "nautilus_trader";
    header_line(c, &format!("{package}: {}", python_package_version(package)));
    header_line(c, &format!("nautilus_core: {NAUTILUS_VERSION_CORE}"));
    header_line(c, &format!("python: {}", python_version()));
    log_build_versioning(c);

    // Transitional: these optional-package lines will be removed once v1 support is dropped.
    for package in ["numpy", "pandas", "msgspec", "pyarrow", "pytz", "uvloop"] {
        if let Some(version) = python_package_version_opt(package) {
            header_line(c, &format!("{package}: {version}"));
        }
    }

    #[cfg(feature = "live")]
    log_tokio_versioning(c);
}

#[rustfmt::skip]
fn log_build_versioning(c: Ustr) {
    for (name, version) in build_versions() {
        header_line(c, &format!("{name}: {version}"));
    }
}

#[cfg(feature = "live")]
#[rustfmt::skip]
fn log_tokio_versioning(c: Ustr) {
    let version = env!("NAUTILUS_BUILD_TOKIO_VERSION");
    if !version.is_empty() {
        header_line(c, &format!("tokio: {version}"));
    }
}

fn build_versions() -> Vec<(&'static str, String)> {
    let versions = BUILD_VERSIONS
        .iter()
        .filter(|(_, version)| !version.is_empty())
        .map(|(name, version)| (*name, display_version(name, version)))
        .collect::<Vec<_>>();

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    let versions = {
        let mut versions = versions;
        if let Some(version) = libc_runtime_version() {
            let index = versions
                .iter()
                .position(|(name, _)| *name == "libc_crate")
                .map_or(versions.len(), |index| index + 1);
            versions.insert(index, ("libc_runtime", version));
        }
        versions
    };

    versions
}

fn display_version(name: &str, version: &str) -> String {
    match name {
        "cargo_lock" => version
            .strip_prefix("crc32:")
            .unwrap_or(version)
            .to_string(),
        "git_commit" => version.get(..GIT_COMMIT_LEN).unwrap_or(version).to_string(),
        _ => version.to_string(),
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
fn libc_runtime_version() -> Option<String> {
    // SAFETY: glibc owns the returned static null-terminated string.
    let version = unsafe { libc::gnu_get_libc_version() };
    if version.is_null() {
        return None;
    }

    // SAFETY: glibc documents the returned pointer as a valid C string.
    let version = unsafe { CStr::from_ptr(version) }.to_str().ok()?;
    Some(format!("glibc {version}"))
}

#[cfg(feature = "python")]
#[inline]
#[allow(unsafe_code)]
fn python_available() -> bool {
    // SAFETY: `Py_IsInitialized` reads a flag and is safe to call at any
    // time, even before the interpreter has been started.
    unsafe { pyo3::ffi::Py_IsInitialized() != 0 }
}

#[rustfmt::skip]
pub fn log_sysinfo(component: Ustr) {
    let mut sys = System::new();
    sys.refresh_memory();

    let c = component;

    let ram_total = sys.total_memory();
    let ram_used = sys.used_memory();
    let ram_used_p = (ram_used as f64 / ram_total as f64) * 100.0;
    let ram_avail = ram_total - ram_used;
    let ram_avail_p = (ram_avail as f64 / ram_total as f64) * 100.0;

    header_sepr(c, "=================================================================");
    header_sepr(c, " MEMORY USAGE");
    header_sepr(c, "=================================================================");
    header_line(c, &format!("RAM-Total: {:.2} GiB", bytes_to_gib(ram_total)));
    header_line(c, &format!("RAM-Used: {:.2} GiB ({:.2}%)", bytes_to_gib(ram_used), ram_used_p));
    header_line(c, &format!("RAM-Avail: {:.2} GiB ({:.2}%)", bytes_to_gib(ram_avail), ram_avail_p));

    let swap_total = sys.total_swap();
    if swap_total > 0 {
        let swap_used = sys.used_swap();
        let swap_used_p = (swap_used as f64 / swap_total as f64) * 100.0;
        let swap_avail = swap_total.saturating_sub(swap_used);
        let swap_avail_p = (swap_avail as f64 / swap_total as f64) * 100.0;
        header_line(c, &format!("Swap-Total: {:.2} GiB", bytes_to_gib(swap_total)));
        header_line(c, &format!("Swap-Used: {:.2} GiB ({:.2}%)", bytes_to_gib(swap_used), swap_used_p));
        header_line(c, &format!("Swap-Avail: {:.2} GiB ({:.2}%)", bytes_to_gib(swap_avail), swap_avail_p));
    } else {
        header_line(c, "Swap: disabled");
    }
}

fn header_sepr(c: Ustr, s: &str) {
    log_info!("{}", s, color = LogColor::Cyan, component = c.as_str());
}

fn header_line(c: Ustr, s: &str) {
    log_info!("{}", s, component = c.as_str());
}

fn bytes_to_gib(b: u64) -> f64 {
    b as f64 / (2u64.pow(30) as f64)
}

#[cfg(feature = "python")]
fn python_package_version(package: &str) -> String {
    nautilus_core::python::version::get_python_package_version(package)
}

#[cfg(feature = "python")]
fn python_package_version_opt(package: &str) -> Option<String> {
    nautilus_core::python::version::get_python_package_version_opt(package)
}

#[cfg(feature = "python")]
fn python_version() -> String {
    nautilus_core::python::version::get_python_version()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_build_versions_match_compiled_metadata() {
        let versions = build_versions();

        for (name, expected) in BUILD_VERSIONS {
            let expected = (!expected.is_empty()).then(|| display_version(name, expected));
            assert_eq!(version(&versions, name), expected.as_deref());
        }
    }

    #[rstest]
    fn test_cargo_lock_fingerprint_format() {
        const CRC32_HEX_LEN: usize = 8;

        let versions = build_versions();
        let Some(fingerprint) = version(&versions, "cargo_lock") else {
            return;
        };
        let embedded = env!("NAUTILUS_BUILD_CARGO_LOCK_CRC32");
        let Some(checksum) = embedded.strip_prefix("crc32:") else {
            panic!("embedded cargo_lock fingerprint should use the crc32 prefix");
        };

        assert_eq!(checksum.len(), CRC32_HEX_LEN);
        assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(fingerprint.len(), CRC32_HEX_LEN);
        assert_eq!(fingerprint, checksum);
    }

    #[rstest]
    fn test_git_commit_format() {
        let versions = build_versions();
        let Some(commit) = version(&versions, "git_commit") else {
            return;
        };

        assert_eq!(commit.len(), GIT_COMMIT_LEN);
        assert!(commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[rstest]
    fn test_libc_runtime_version_reports_glibc() {
        let version = libc_runtime_version().expect("glibc should report its runtime version");

        assert!(version.starts_with("glibc "));
        assert!(
            version["glibc ".len()..]
                .bytes()
                .any(|byte| byte.is_ascii_digit())
        );
    }

    fn version<'a>(versions: &'a [(&str, String)], name: &str) -> Option<&'a str> {
        versions
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, version)| version.as_str())
    }
}
