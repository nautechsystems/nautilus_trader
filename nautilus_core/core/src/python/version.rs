// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::{prelude::*, types::PyTuple};

#[must_use]
pub fn get_python_version() -> String {
    Python::with_gil(|py| {
        let sys = match py.import("sys") {
            Ok(mod_sys) => mod_sys,
            Err(_) => return "Unavailable (failed to import sys)".to_string(),
        };

        let version_info = match sys.getattr("version_info") {
            Ok(info) => info,
            Err(_) => return "Unavailable (version_info not found)".to_string(),
        };

        let version_tuple: &Bound<'_, PyTuple> = version_info
            .downcast::<PyTuple>()
            .expect("Failed to extract version_info");

        let major = version_tuple
            .get_item(0)
            .expect("Failed to get major version")
            .extract::<i32>()
            .unwrap_or(-1);
        let minor = version_tuple
            .get_item(1)
            .expect("Failed to get minor version")
            .extract::<i32>()
            .unwrap_or(-1);
        let micro = version_tuple
            .get_item(2)
            .expect("Failed to get micro version")
            .extract::<i32>()
            .unwrap_or(-1);

        if major == -1 || minor == -1 || micro == -1 {
            "Unavailable (failed to extract version components)".to_string()
        } else {
            format!("{major}.{minor}.{micro}")
        }
    })
}

#[must_use]
pub fn get_python_package_version(package_name: &str) -> String {
    Python::with_gil(|py| match py.import(package_name) {
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
