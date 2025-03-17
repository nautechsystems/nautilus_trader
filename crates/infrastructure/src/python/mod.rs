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

//! Python bindings from [PyO3](https://pyo3.rs).

#[cfg(feature = "redis")]
pub mod redis;

#[cfg(feature = "postgres")]
pub mod sql;

use pyo3::{prelude::*, pymodule};

#[pymodule]
pub fn infrastructure(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::cache::RedisCacheDatabase>()?;
    #[cfg(feature = "redis")]
    m.add_class::<crate::redis::msgbus::RedisMessageBusDatabase>()?;
    #[cfg(feature = "postgres")]
    m.add_class::<crate::sql::cache::PostgresCacheDatabase>()?;
    Ok(())
}
