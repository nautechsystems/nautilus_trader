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

//! Semantic version parsing and comparison.

use std::fmt::Display;

/// Parsed semantic version with major, minor, and patch components.
///
/// Supports parsing `"X.Y.Z"` strings and lexicographic comparison
/// (major, then minor, then patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    /// Major version number.
    pub major: u64,
    /// Minor version number.
    pub minor: u64,
    /// Patch version number.
    pub patch: u64,
}

impl SemVer {
    /// Parses a `"major.minor.patch"` string into a [`SemVer`].
    ///
    /// Missing components default to zero.
    ///
    /// # Errors
    ///
    /// Returns an error if any component of `s` fails to parse as a [`u64`].
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let mut parts = s.split('.').map(str::parse::<u64>);
        let major = parts.next().unwrap_or(Ok(0))?;
        let minor = parts.next().unwrap_or(Ok(0))?;
        let patch = parts.next().unwrap_or(Ok(0))?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("6.2.0", 6, 2, 0)]
    #[case("7.0.15", 7, 0, 15)]
    #[case("0.0.1", 0, 0, 1)]
    #[case("1", 1, 0, 0)]
    #[case("2.5", 2, 5, 0)]
    fn test_semver_parse(
        #[case] input: &str,
        #[case] major: u64,
        #[case] minor: u64,
        #[case] patch: u64,
    ) {
        let v = SemVer::parse(input).unwrap();
        assert_eq!(v.major, major);
        assert_eq!(v.minor, minor);
        assert_eq!(v.patch, patch);
    }

    #[rstest]
    fn test_semver_display() {
        let v = SemVer::parse("7.2.4").unwrap();
        assert_eq!(v.to_string(), "7.2.4");
    }

    #[rstest]
    fn test_semver_ordering() {
        let v620 = SemVer::parse("6.2.0").unwrap();
        let v700 = SemVer::parse("7.0.0").unwrap();
        let v621 = SemVer::parse("6.2.1").unwrap();
        let v630 = SemVer::parse("6.3.0").unwrap();

        assert!(v700 > v620);
        assert!(v621 > v620);
        assert!(v630 > v621);
        assert!(v700 >= v620);
        assert!(v620 >= v620);
    }

    #[rstest]
    fn test_semver_parse_invalid() {
        assert!(SemVer::parse("abc").is_err());
    }
}
