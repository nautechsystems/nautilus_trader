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

//! Reproducible historical replay from captured catalog archives.
//!
//! A replay archive is described by a [`manifest::ReplayManifest`], which states where the data
//! came from, when it was captured, which files belong to it, and what it is missing. The manifest
//! turns its datasets into the catalog queries a `BacktestNode` replays, so an offline command can
//! reproduce a run from the manifest and the catalog files alone.

pub mod manifest;

pub use manifest::{
    REPLAY_CHECKSUM_PREFIX, REPLAY_MANIFEST_SCHEMA_VERSION, ReplayDataset, ReplayLimitation,
    ReplayManifest, ReplaySource,
};
