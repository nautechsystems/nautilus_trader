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

//! Represents a valid strategy ID.

use std::fmt::{Debug, Display};

use nautilus_core::correctness::{FAILED, check_string_contains, check_valid_string_ascii};
use ustr::Ustr;

/// The identifier for all 'external' strategy IDs (not local to this system instance).
const EXTERNAL_STRATEGY_ID: &str = "EXTERNAL";

/// Represents a valid strategy ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct StrategyId(Ustr);

impl StrategyId {
    /// Creates a new [`StrategyId`] instance.
    ///
    /// Must be correctly formatted with two valid strings either side of a hyphen.
    /// It is expected a strategy ID is the class name of the strategy,
    /// with an order ID tag number separated by a hyphen.
    ///
    /// Example: "EMACross-001".
    ///
    /// The reason for the numerical component of the ID is so that order and position IDs
    /// do not collide with those from another strategy within the node instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is not a valid ASCII string.
    /// - `value` is not "EXTERNAL" and does not contain a hyphen '-' separator.
    /// - Either the name or tag part (before/after the hyphen) is empty.
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let value = value.as_ref();
        check_valid_string_ascii(value, stringify!(value))?;
        if value != EXTERNAL_STRATEGY_ID {
            check_string_contains(value, "-", stringify!(value))?;

            if let Some((name, tag)) = value.rsplit_once('-') {
                anyhow::ensure!(
                    !name.is_empty(),
                    "`value` name part (before '-') cannot be empty"
                );
                anyhow::ensure!(
                    !tag.is_empty(),
                    "`value` tag part (after '-') cannot be empty"
                );
            }
        }
        Ok(Self(Ustr::from(value)))
    }

    /// Creates a new [`StrategyId`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not a valid string.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect(FAILED)
    }

    /// Sets the inner identifier value.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub fn external() -> Self {
        // SAFETY:: Constant value is safe
        Self::new(EXTERNAL_STRATEGY_ID)
    }

    #[must_use]
    pub fn is_external(&self) -> bool {
        self.0 == EXTERNAL_STRATEGY_ID
    }

    /// Returns the numerical tag portion of the strategy ID.
    ///
    /// For external strategy IDs (no separator), returns the full ID string.
    #[must_use]
    pub fn get_tag(&self) -> &str {
        self.0
            .rsplit_once('-')
            .map_or(self.0.as_str(), |(_, tag)| tag)
    }
}

impl Debug for StrategyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.0)
    }
}

impl Display for StrategyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::StrategyId;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_string_reprs(strategy_id_ema_cross: StrategyId) {
        assert_eq!(strategy_id_ema_cross.as_str(), "EMACross-001");
        assert_eq!(format!("{strategy_id_ema_cross}"), "EMACross-001");
    }

    #[rstest]
    fn test_get_external() {
        assert_eq!(StrategyId::external().as_str(), "EXTERNAL");
    }

    #[rstest]
    fn test_is_external() {
        assert!(StrategyId::external().is_external());
    }

    #[rstest]
    fn test_get_tag(strategy_id_ema_cross: StrategyId) {
        assert_eq!(strategy_id_ema_cross.get_tag(), "001");
    }

    #[rstest]
    fn test_get_tag_external() {
        assert_eq!(StrategyId::external().get_tag(), "EXTERNAL");
    }

    #[rstest]
    #[should_panic(expected = "name part (before '-') cannot be empty")]
    fn test_new_with_empty_name_panics() {
        let _ = StrategyId::new("-001");
    }

    #[rstest]
    #[should_panic(expected = "tag part (after '-') cannot be empty")]
    fn test_new_with_empty_tag_panics() {
        let _ = StrategyId::new("EMACross-");
    }

    #[rstest]
    fn test_new_checked_with_empty_name_returns_error() {
        assert!(StrategyId::new_checked("-001").is_err());
    }

    #[rstest]
    fn test_new_checked_with_empty_tag_returns_error() {
        assert!(StrategyId::new_checked("EMACross-").is_err());
    }
}
