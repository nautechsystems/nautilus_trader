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

//! Data serialization and format conversion for [NautilusTrader](http://nautilustrader.io).
//!
//! The *serialization* crate provides comprehensive data serialization capabilities for converting
//! trading data between different formats including Apache Arrow, Parquet, and custom schemas.
//! This enables efficient data storage, retrieval, and interoperability across different systems:
//!
//! - **Apache Arrow integration**: Schema definitions and encoding/decoding for market data types.
//! - **Parquet file operations**: High-performance columnar storage for historical data analysis.
//! - **Record batch processing**: Efficient batch operations for time-series data.
//! - **Schema management**: Type-safe schema definitions with metadata preservation.
//! - **Cross-format conversion**: Seamless data interchange between Arrow, Parquet, and native types.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod arrow;

#[cfg(feature = "python")]
pub mod python;
