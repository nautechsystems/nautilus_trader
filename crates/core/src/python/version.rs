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

//! Functions for introspecting the running Python interpreter & installed packages.

#![expect(
    clippy::manual_let_else,
    reason = "Prefer explicit control flow for error handling"
)]
use pyo3::{Bound, prelude::*, types::PyTuple};

/// Retrieves the Python interpreter version as a string.
///
#[must_use]
pub fn get_python_version() -> String {
    Python::attach(|py| {
        let sys = match py.import("sys") {
            Ok(mod_sys) => mod_sys,
            Err(_) => return "Unavailable (failed to import sys)".to_string(),
        };

        let version_info = match sys.getattr("version_info") {
            Ok(info) => info,
            Err(_) => return "Unavailable (version_info not found)".to_string(),
        };

        let version_tuple: &Bound<'_, PyTuple> = match version_info.cast::<PyTuple>() {
            Ok(tuple) => tuple,
            Err(_) => return "Unavailable (failed to extract version_info)".to_string(),
        };

        let major = version_tuple
            .get_item(0)
            .ok()
            .and_then(|item| item.extract::<i32>().ok())
            .unwrap_or(-1);
        let minor = version_tuple
            .get_item(1)
            .ok()
            .and_then(|item| item.extract::<i32>().ok())
            .unwrap_or(-1);
        let micro = version_tuple
            .get_item(2)
            .ok()
            .and_then(|item| item.extract::<i32>().ok())
            .unwrap_or(-1);

        if major == -1 || minor == -1 || micro == -1 {
            "Unavailable (failed to extract version components)".to_string()
        } else {
            format!("{major}.{minor}.{micro}")
        }
    })
}

#[must_use]
/// Attempt to retrieve the `__version__` attribute of a *Python* package.
///
/// When the requested package cannot be imported, or when it does not define a `__version__`
/// attribute, the function returns a human-readable fallback string that starts with
/// `"Unavailable"` so that downstream code can distinguish *real* version strings from error
/// cases.
///
/// This function is primarily intended for diagnostic/logging purposes inside the NautilusTrader
/// Python bindings.
pub fn get_python_package_version(package_name: &str) -> String {
    Python::attach(|py| match py.import(package_name) {
        Ok(package) => match package.getattr("__version__") {
            Ok(version_attr) => match version_attr.extract::<String>() {
                Ok(version) => version,
                Err(_) => "Unavailable (failed to extract version)".to_string(),
            },
            Err(_) => "Unavailable (__version__ attribute not found)".to_string(),
        },
        Err(_) => "Unavailable (failed to import package)".to_string(),
    })
}

/// Returns the `__version__` of a *Python* package, or `None` when it is not installed
/// (or does not expose a usable `__version__`).
///
/// Unlike [`get_python_package_version`], this distinguishes "not installed" from a real
/// version so callers can omit absent packages instead of logging an "Unavailable" line.
#[must_use]
pub fn get_python_package_version_opt(package_name: &str) -> Option<String> {
    Python::attach(|py| {
        let package = py.import(package_name).ok()?;
        let version_attr = package.getattr("__version__").ok()?;
        version_attr.extract::<String>().ok()
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_python_version_handles_malformed_version_info() {
        Python::initialize();
        let version = Python::attach(|py| -> PyResult<String> {
            let sys = py.import("sys")?;
            let original = sys.getattr("version_info")?;
            sys.setattr("version_info", "malformed")?;
            let version = get_python_version();
            sys.setattr("version_info", original)?;
            Ok(version)
        })
        .expect("test Python setup should succeed");

        assert_eq!(version, "Unavailable (failed to extract version_info)");
    }

    #[rstest]
    fn test_get_python_package_version_opt_missing_returns_none() {
        Python::initialize();
        assert!(get_python_package_version_opt("nautilus_definitely_absent_pkg").is_none());
    }

    #[rstest]
    fn test_get_python_package_version_opt_present_returns_some() {
        Python::initialize();
        let version = Python::attach(|py| {
            let types = py.import("types").expect("import types");
            let module = types
                .call_method1("ModuleType", ("fake_pkg_xyz",))
                .expect("create module");
            module
                .setattr("__version__", "9.9.9")
                .expect("set __version__");
            let modules = py
                .import("sys")
                .expect("import sys")
                .getattr("modules")
                .expect("sys.modules");
            modules
                .set_item("fake_pkg_xyz", &module)
                .expect("register module");
            get_python_package_version_opt("fake_pkg_xyz")
        });

        assert_eq!(version, Some("9.9.9".to_string()));
    }
}
