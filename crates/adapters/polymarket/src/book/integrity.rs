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

//! Retains book timestamp ordering and the accepted snapshot's verification status.
//!
//! The status preserves strict malformed-update handling while the latest accepted snapshot is verified.
//! Incremental messages omit mutable hash inputs, so their hashes cannot gate publication.

use nautilus_core::UnixNanos;

#[derive(Debug, Default)]
pub(crate) struct BookIntegrity {
    latest_timestamp: UnixNanos,
    snapshot_verified: bool,
}

impl BookIntegrity {
    pub(crate) fn observe(&mut self, timestamp: UnixNanos) -> bool {
        if self.is_stale(timestamp) {
            return false;
        }

        self.latest_timestamp = timestamp;
        true
    }

    pub(crate) fn is_stale(&self, timestamp: UnixNanos) -> bool {
        timestamp < self.latest_timestamp
    }

    pub(crate) fn accept_snapshot(&mut self, verified: bool) {
        self.snapshot_verified = verified;
    }

    pub(crate) fn snapshot_verified(&self) -> bool {
        self.snapshot_verified
    }
}
