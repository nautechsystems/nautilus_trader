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
//! The `nautilus-serialization` crate provides comprehensive data serialization capabilities for converting
//! trading data between different formats including Apache Arrow and Cap'n Proto.
//! This enables efficient data storage, retrieval, and interoperability across different systems:
//!
//! - **Apache Arrow integration**: Schema definitions and encoding/decoding for market data types.
//! - **Parquet file operations**: High-performance columnar storage for historical data analysis.
//! - **Record batch processing**: Efficient batch operations for time-series data.
//! - **Schema management**: Type-safe schema definitions with metadata preservation.
//! - **Cross-format conversion**: Seamless data interchange between Arrow, Cap'n Proto, and native types.
//! - **Cap'n Proto serialization**: Zero-copy, schema-based serialization for efficient data interchange (requires `capnp` feature).
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
//! highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.
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
//! - `extension-module`: Builds the crate as a Python extension module.
//! - `capnp`: Enables [Cap'n Proto](https://capnproto.org/) serialization support.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod arrow;

/// Re-export MsgPack serialization helpers for consumers expecting to configure codecs via this crate.
pub use nautilus_core::serialization::msgpack;

#[cfg(feature = "capnp")]
pub mod capnp;

#[cfg(feature = "capnp")]
macro_rules! include_capnp_module {
    ($name:ident, $path:expr) => {
        #[cfg(all(feature = "capnp", not(docs_rs)))]
        #[allow(clippy::all, warnings, dead_code, missing_debug_implementations)]
        pub mod $name {
            include!(concat!(env!("OUT_DIR"), $path));
        }

        #[cfg(all(feature = "capnp", docs_rs))]
        #[allow(clippy::all, warnings, dead_code, missing_debug_implementations)]
        pub mod $name {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/generated/capnp",
                $path
            ));
        }
    };
}

#[cfg(feature = "capnp")]
include_capnp_module!(base_capnp, "/common/base_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(identifiers_capnp, "/common/identifiers_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(types_capnp, "/common/types_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(enums_capnp, "/common/enums_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(trading_capnp, "/commands/trading_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(data_capnp, "/commands/data_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(order_capnp, "/events/order_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(position_capnp, "/events/position_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(account_capnp, "/events/account_capnp.rs");
#[cfg(feature = "capnp")]
include_capnp_module!(market_capnp, "/data/market_capnp.rs");

#[cfg(feature = "python")]
pub mod python;
