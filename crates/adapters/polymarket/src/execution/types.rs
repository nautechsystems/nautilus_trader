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

//! Shared types for the Polymarket execution module.

use crate::common::consts::CANCEL_ALREADY_DONE;

/// Classifies cancel rejection reasons to eliminate duplicate if/else blocks.
pub(crate) enum CancelOutcome {
    AlreadyDone,
    Rejected(String),
}

impl CancelOutcome {
    pub fn classify(reason: &str) -> Self {
        if reason.contains(CANCEL_ALREADY_DONE) {
            Self::AlreadyDone
        } else {
            Self::Rejected(reason.to_string())
        }
    }
}
