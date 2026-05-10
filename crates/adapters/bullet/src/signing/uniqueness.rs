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

//! Uniqueness / nonce generation for Bullet transactions.
//!
//! Uses `Generation(microseconds)` mode — the recommended approach per Bullet docs.
//! Multiple transactions can be in-flight concurrently; the only requirement is that
//! each timestamp is distinct within a ~5-6 second block window.

use std::time::{SystemTime, UNIX_EPOCH};

/// Return a microsecond-resolution Unix timestamp suitable for use as a
/// `UniquenessData::Generation` value.
///
/// # Panics
///
/// Panics if the system clock is set before the Unix epoch (should never happen in practice).
#[must_use]
pub fn generation_nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_micros() as u64
}
