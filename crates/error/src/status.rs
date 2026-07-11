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

/// Describes whether an error is retryable.
///
/// This enables callers to make programmatic retry decisions without
/// inspecting error messages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ErrorStatus {
    /// The error is permanent. Retrying without external changes will not help.
    /// Examples: invalid config, not found, permission denied.
    #[default]
    Permanent,

    /// The error is temporary. The caller may retry and succeed.
    /// Examples: network timeout, rate limited, transient IO error.
    Temporary,

    /// The error was once temporary but persists after retries.
    /// The caller should stop retrying and escalate.
    Persistent,
}

impl ErrorStatus {
    /// Returns `true` if the error may be resolved by retrying.
    #[inline]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Temporary)
    }
}

impl std::fmt::Display for ErrorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permanent => write!(f, "permanent"),
            Self::Temporary => write!(f, "temporary"),
            Self::Persistent => write!(f, "persistent"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_permanent() {
        assert_eq!(ErrorStatus::default(), ErrorStatus::Permanent);
    }

    #[test]
    fn retryable() {
        assert!(!ErrorStatus::Permanent.is_retryable());
        assert!(ErrorStatus::Temporary.is_retryable());
        assert!(!ErrorStatus::Persistent.is_retryable());
    }
}
