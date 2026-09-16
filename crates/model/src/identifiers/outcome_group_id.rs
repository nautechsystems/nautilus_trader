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

//! Represents the venue-scoped identity of a prediction market outcome group.
//!
//! Group identity is distinct from event identity and from tradable instrument identity. One event
//! can carry several groups, and one group carries one instrument per outcome. A venue supplies the
//! group key, such as a Polymarket condition ID or a Kalshi market ticker.
//!
//! The group key is held as a [`String`] rather than an interned string. Group keys are
//! high-cardinality external identifiers, and interning each one would grow process memory without
//! bound.

use std::fmt::Display;

use nautilus_core::correctness::CorrectnessError;
use serde::{Deserialize, Serialize};

use crate::identifiers::Venue;

/// Identifies a prediction market outcome group at a specific venue.
///
/// The pair of venue and group key is stable for the life of the market, so it can key settlement,
/// resolution caches, and exposure limits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutcomeGroupId {
    /// The trading venue.
    pub venue: Venue,
    /// The venue-assigned group key, such as a condition ID or market ticker.
    pub group: String,
}

/// Error returned when a value is not a valid [`OutcomeGroupId`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OutcomeGroupIdError {
    /// The venue component is invalid.
    #[error("invalid `OutcomeGroupId` value '{value}': invalid venue: {source}")]
    InvalidVenue {
        /// The invalid identifier value.
        value: String,
        /// The venue validation failure.
        source: Box<CorrectnessError>,
    },
    /// The group key is empty.
    #[error("invalid `OutcomeGroupId` value '{value}': group key must not be empty")]
    EmptyGroup {
        /// The invalid identifier value.
        value: String,
    },
    /// The value is not in `GROUP.VENUE` form.
    #[error("invalid `OutcomeGroupId` value '{value}': expected 'GROUP.VENUE'")]
    Malformed {
        /// The invalid identifier value.
        value: String,
    },
}

impl OutcomeGroupId {
    /// Creates a new [`OutcomeGroupId`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `group` is empty or `venue` is invalid.
    pub fn new_checked(venue: &str, group: &str) -> Result<Self, OutcomeGroupIdError> {
        let value = format!("{group}.{venue}");
        if group.is_empty() {
            return Err(OutcomeGroupIdError::EmptyGroup { value });
        }
        let venue =
            Venue::new_checked(venue).map_err(|source| OutcomeGroupIdError::InvalidVenue {
                value,
                source: Box::new(source),
            })?;

        Ok(Self {
            venue,
            group: group.to_string(),
        })
    }

    /// Creates a new [`OutcomeGroupId`] from an already-validated venue.
    ///
    /// # Errors
    ///
    /// Returns an error if `group` is empty.
    pub fn from_parts(venue: Venue, group: &str) -> Result<Self, OutcomeGroupIdError> {
        if group.is_empty() {
            return Err(OutcomeGroupIdError::EmptyGroup {
                value: format!("{group}.{venue}"),
            });
        }

        Ok(Self {
            venue,
            group: group.to_string(),
        })
    }

    /// Returns the venue-scoped group key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.group
    }
}

impl Display for OutcomeGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.group, self.venue)
    }
}

impl std::str::FromStr for OutcomeGroupId {
    type Err = OutcomeGroupIdError;

    /// Parses the canonical `GROUP.VENUE` form.
    ///
    /// A group key may contain dots, so the venue is taken from after the last one.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((group, venue)) = value.rsplit_once('.') else {
            return Err(OutcomeGroupIdError::Malformed {
                value: value.to_string(),
            });
        };

        Self::new_checked(venue, group)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_checked_valid() {
        let id = OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap();

        assert_eq!(id.venue, Venue::from("POLYMARKET"));
        assert_eq!(id.key(), "0xCONDITION");
        assert_eq!(id.to_string(), "0xCONDITION.POLYMARKET");
    }

    #[rstest]
    fn test_from_str_roundtrips_display() {
        let id = OutcomeGroupId::new_checked("KALSHI", "KXBTCD-25DEC31").unwrap();

        assert_eq!(id.to_string().parse::<OutcomeGroupId>().unwrap(), id);
    }

    #[rstest]
    fn test_from_str_keeps_dots_in_group_key() {
        let id = "a.b.c.VENUE".parse::<OutcomeGroupId>().unwrap();

        assert_eq!(id.group, "a.b.c");
        assert_eq!(id.venue, Venue::from("VENUE"));
    }

    #[rstest]
    fn test_from_str_rejects_value_without_venue() {
        let error = "no-venue"
            .parse::<OutcomeGroupId>()
            .expect_err("must reject");

        assert!(matches!(error, OutcomeGroupIdError::Malformed { value } if value == "no-venue"));
    }

    #[rstest]
    #[case::empty_group("POLYMARKET", "")]
    #[case::empty_venue("", "0xCONDITION")]
    fn test_new_checked_rejects_invalid(#[case] venue: &str, #[case] group: &str) {
        assert!(OutcomeGroupId::new_checked(venue, group).is_err());
    }

    #[rstest]
    fn test_identity_is_venue_scoped() {
        let polymarket = OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap();
        let kalshi = OutcomeGroupId::new_checked("KALSHI", "0xCONDITION").unwrap();

        assert_ne!(polymarket, kalshi);
    }

    #[rstest]
    fn test_serialization_round_trip() {
        let id = OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap();

        let json = serde_json::to_string(&id).unwrap();
        let restored: OutcomeGroupId = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, id);
    }
}
