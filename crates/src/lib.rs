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

//! Workspace shim crate for *NautilusTrader*.
//!
//! This crate does **not** expose any public API of its own – it exists solely so that the root
//! `crates/` directory is a valid Cargo crate.  Having a real compilation unit prevents Cargo
//! from discarding incremental build artefacts for the workspace when only the dependency graph
//! changes, which in turn keeps rebuild times predictable.
//!
//! Because downstream users should never depend on this shim directly, the crate is marked
//! `publish = false` in its `Cargo.toml`.
//!
//! If you are looking for the public entry-points of NautilusTrader, refer to the individual crates
//! under `crates/*` such as `nautilus-core`, `nautilus-model`, `nautilus-backtest`, etc.

// This minimal crate (with a placeholder `src/` and `Cargo.toml`) ensures Cargo treats it as a
// valid compilation unit. Without these placeholders, Cargo might skip or rebuild this directory
// in a way that forces other crates to rebuild. By including a “dummy” crate, we keep the build
// cache fresh and avoid unnecessary full rebuilds.
