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
/// Fields are ordered from most abstract (workflow grouping) to most concrete (one-hop
/// lineage), matching the CQRS / event-sourcing convention (`EventStore`, Axon, Marten
/// all list correlation, then causation, then message id). All fields default to `None` so
/// capture works before propagation discipline lands across the command, event, and
/// reconciliation report types. Once a field is populated, the bus capture adapter writes
/// it through; replay never invents values.
///
/// Agent-level intent is not a separate field at this layer: when an agent decision is
/// lowered into a bus message, the agent's `intent_id` is written to the message's
/// `correlation_id`, so forensics queries that need "find by agent intent" scan the
/// captured stream by `correlation_id`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Headers {
    /// The correlation chain id that ties commands, events, and reports to one logical action.
    pub correlation_id: Option<UUID4>,
    /// The id of the message that directly caused this one, if any.
    pub causation_id: Option<UUID4>,
}

impl Headers {
    /// Creates a new [`Headers`] with all fields unset.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            correlation_id: None,
            causation_id: None,
        }
    }

    /// Returns `true` if every header field is unset.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.correlation_id.is_none() && self.causation_id.is_none()
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
            correlation_id: Some(UUID4::new()),
            causation_id: None,
        };
        assert!(!headers.is_empty());
    }
}
