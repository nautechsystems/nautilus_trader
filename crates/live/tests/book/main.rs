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

//! Tests for the shared book stress harness machinery.
//!
//! The `stress` module is a development tool for changes to book sync and recovery code. Venue
//! harnesses under `crates/adapters/<venue>/tests/stress/` include it to run live against a venue
//! with injected faults, checking every emitted book against the book stream contract in
//! `nautilus_live::book::conformance`. These tests cover the parts that need no venue:
//!
//! - The fault proxy's relay rules, against a local test venue.
//! - Argument parsing and usage text.
//! - The wire book that venue oracles rebuild from raw frames.
//!
//! Run with `cargo nextest run -p nautilus-live --features test-support --test book`. The developer
//! guide explains how the harnesses fit into book conformance validation:
//! [Order book sync conformance](https://nautilustrader.io/docs/nightly/developer_guide/spec_data_testing/#order-book-sync-conformance).

mod stress;
