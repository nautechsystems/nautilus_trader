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

//! First-class correlation headers propagated end-to-end across the system.

use nautilus_core::UUID4;
use serde::{Deserialize, Serialize};

/// First-class metadata propagated end-to-end across captured messages.
///
/// All fields default to `None` so capture works before propagation discipline lands across
/// the command, event, and reconciliation report types. Once a field is populated, the bus
/// capture adapter writes it through; replay never invents values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Headers {
    /// The agent or strategy intent that originated this message, if known.
    ///
    /// Replay keys forensics and decision-correlation lookups by `intent_id`.
    pub intent_id: Option<UUID4>,
    /// The correlation chain id that ties commands, events, and reports to one logical action.
    pub correlation_id: Option<UUID4>,
    /// The id of the message that directly caused this one, if any.
    pub caused_by: Option<UUID4>,
}

impl Headers {
    /// Creates a new [`Headers`] with all fields unset.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            intent_id: None,
            correlation_id: None,
            caused_by: None,
        }
    }

    /// Returns `true` if every header field is unset.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.intent_id.is_none() && self.correlation_id.is_none() && self.caused_by.is_none()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn default_is_empty() {
        let headers = Headers::default();
        assert!(headers.is_empty());
        assert_eq!(headers, Headers::empty());
    }

    #[rstest]
    fn populated_headers_are_not_empty() {
        let headers = Headers {
            intent_id: Some(UUID4::new()),
            correlation_id: None,
            caused_by: None,
        };
        assert!(!headers.is_empty());
    }
}
