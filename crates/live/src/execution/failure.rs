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

//! Shared classification of order command failures for live execution.

/// Classifies why a state-changing order command failed, by the evidence available.
///
/// Adapters keep their own error types and map each submit, modify, or cancel failure to one
/// variant, including the batch and list forms, so the same wire condition classifies identically
/// across venues. A definitive venue acceptance or update is not a failure and carries no variant.
///
/// This axis is independent of retryability. Retryability answers whether to send the request
/// again; this answers whether the venue may already have acted on the first attempt. An error can
/// be both, either, or neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandFailure {
    /// A deterministic local failure proves the command was never transmitted.
    ///
    /// A terminal rejection event is valid for this evidence.
    NotSent(String),
    /// The venue outcome is undefined; the command may still have been applied.
    ///
    /// A terminal event is never valid for this evidence. Leave the command in flight for a
    /// stream update, query, poll, or reconciliation to resolve it either way.
    Ambiguous(String),
    /// The venue explicitly declared the command rejected.
    ///
    /// A terminal rejection event is valid for this evidence.
    VenueRejected(String),
}

impl CommandFailure {
    /// Creates a new [`CommandFailure::NotSent`] with the given `reason`.
    #[must_use]
    pub fn not_sent(reason: impl Into<String>) -> Self {
        Self::NotSent(reason.into())
    }

    /// Creates a new [`CommandFailure::Ambiguous`] with the given `reason`.
    #[must_use]
    pub fn ambiguous(reason: impl Into<String>) -> Self {
        Self::Ambiguous(reason.into())
    }

    /// Creates a new [`CommandFailure::VenueRejected`] with the given `reason`.
    #[must_use]
    pub fn venue_rejected(reason: impl Into<String>) -> Self {
        Self::VenueRejected(reason.into())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::CommandFailure;

    #[rstest]
    #[case(
        CommandFailure::not_sent("Failed to build cancel params"),
        CommandFailure::NotSent("Failed to build cancel params".to_string())
    )]
    #[case(
        CommandFailure::ambiguous("connection closed"),
        CommandFailure::Ambiguous("connection closed".to_string())
    )]
    #[case(
        CommandFailure::venue_rejected("EOrder:Unknown order"),
        CommandFailure::VenueRejected("EOrder:Unknown order".to_string())
    )]
    fn test_constructor_variant_and_reason(
        #[case] failure: CommandFailure,
        #[case] expected: CommandFailure,
    ) {
        assert_eq!(failure, expected);
    }
}
