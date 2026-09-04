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

//! Common `Venue` constants.

use std::{
    collections::HashMap,
    sync::{LazyLock, OnceLock},
};

use parking_lot::Mutex;

use crate::identifiers::Venue;

static CBCM_LOCK: OnceLock<Venue> = OnceLock::new();
static GLBX_LOCK: OnceLock<Venue> = OnceLock::new();
static NYUM_LOCK: OnceLock<Venue> = OnceLock::new();
static XCBT_LOCK: OnceLock<Venue> = OnceLock::new();
static XCEC_LOCK: OnceLock<Venue> = OnceLock::new();
static XCME_LOCK: OnceLock<Venue> = OnceLock::new();
static XFXS_LOCK: OnceLock<Venue> = OnceLock::new();
static XNYM_LOCK: OnceLock<Venue> = OnceLock::new();

impl Venue {
    /// Returns the CBCM (Chicago Board of Trade) venue.
    #[allow(non_snake_case)]
    pub fn CBCM() -> Self {
        *CBCM_LOCK.get_or_init(|| Self::from("CBCM"))
    }
    /// Returns the GLBX (Globex) venue.
    #[allow(non_snake_case)]
    pub fn GLBX() -> Self {
        *GLBX_LOCK.get_or_init(|| Self::from("GLBX"))
    }
    /// Returns the NYUM (New York Mercantile Exchange) venue.
    #[allow(non_snake_case)]
    pub fn NYUM() -> Self {
        *NYUM_LOCK.get_or_init(|| Self::from("NYUM"))
    }
    /// Returns the XCBT (Chicago Board of Trade) venue.
    #[allow(non_snake_case)]
    pub fn XCBT() -> Self {
        *XCBT_LOCK.get_or_init(|| Self::from("XCBT"))
    }
    /// Returns the XCEC (Chicago Mercantile Exchange Center) venue.
    #[allow(non_snake_case)]
    pub fn XCEC() -> Self {
        *XCEC_LOCK.get_or_init(|| Self::from("XCEC"))
    }
    /// Returns the XCME (Chicago Mercantile Exchange) venue.
    #[allow(non_snake_case)]
    pub fn XCME() -> Self {
        *XCME_LOCK.get_or_init(|| Self::from("XCME"))
    }
    /// Returns the XFXS (CME FX) venue.
    #[allow(non_snake_case)]
    pub fn XFXS() -> Self {
        *XFXS_LOCK.get_or_init(|| Self::from("XFXS"))
    }
    /// Returns the XNYM (New York Mercantile Exchange) venue.
    #[allow(non_snake_case)]
    pub fn XNYM() -> Self {
        *XNYM_LOCK.get_or_init(|| Self::from("XNYM"))
    }
}

/// A map of built-in `Venue` constants.
pub static VENUE_MAP: LazyLock<Mutex<HashMap<&str, Venue>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    map.insert(Venue::CBCM().inner().as_str(), Venue::CBCM());
    map.insert(Venue::GLBX().inner().as_str(), Venue::GLBX());
    map.insert(Venue::NYUM().inner().as_str(), Venue::NYUM());
    map.insert(Venue::XCBT().inner().as_str(), Venue::XCBT());
    map.insert(Venue::XCEC().inner().as_str(), Venue::XCEC());
    map.insert(Venue::XCME().inner().as_str(), Venue::XCME());
    map.insert(Venue::XFXS().inner().as_str(), Venue::XFXS());
    map.insert(Venue::XNYM().inner().as_str(), Venue::XNYM());
    Mutex::new(map)
});

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    #[case::cbcm(Venue::CBCM, "CBCM")]
    #[case::glbx(Venue::GLBX, "GLBX")]
    #[case::nyum(Venue::NYUM, "NYUM")]
    #[case::xcbt(Venue::XCBT, "XCBT")]
    #[case::xcec(Venue::XCEC, "XCEC")]
    #[case::xcme(Venue::XCME, "XCME")]
    #[case::xfxs(Venue::XFXS, "XFXS")]
    #[case::xnym(Venue::XNYM, "XNYM")]
    fn test_venue_constant(#[case] constructor: fn() -> Venue, #[case] expected: &'static str) {
        let first = constructor();
        let second = constructor();
        let venue_map = VENUE_MAP.lock();

        assert_eq!(first, second);
        assert_eq!(first.inner().as_str(), expected);
        assert_eq!(first.to_string(), expected);
        assert_eq!(venue_map.get(expected), Some(&first));
    }

    #[rstest]
    fn test_venue_constants_are_unique() {
        let venues = all_venues();

        for (i, venue) in venues.iter().enumerate() {
            assert!(!venues[i + 1..].contains(venue), "duplicate venue {venue}");
        }
    }

    #[rstest]
    fn test_venue_map_has_expected_size() {
        let venue_map = VENUE_MAP.lock();

        assert_eq!(venue_map.len(), 8);
    }

    #[rstest]
    #[case("INVALID")]
    #[case("")]
    #[case("NYSE")]
    fn test_venue_map_lookup_returns_none(#[case] value: &str) {
        let venue_map = VENUE_MAP.lock();

        assert_eq!(venue_map.get(value), None);
    }

    #[rstest]
    #[expect(clippy::needless_collect)] // Collect needed for thread handles
    fn test_venue_constants_thread_safety() {
        use std::thread;

        let handles: Vec<_> = (0..4).map(|_| thread::spawn(all_venues)).collect();

        let results: Vec<[Venue; 8]> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        for venues in &results {
            assert_eq!(*venues, all_venues());
        }
    }

    #[rstest]
    #[expect(clippy::needless_collect)] // Collect needed for thread handles
    fn test_venue_map_thread_safety() {
        use std::thread;

        let handles: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(|| {
                    let venue_map = VENUE_MAP.lock();
                    venue_map.get("XCME").copied()
                })
            })
            .collect();

        let results: Vec<Option<Venue>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All threads should return the same result
        for result in results {
            assert_eq!(result, Some(Venue::XCME()));
        }
    }

    fn all_venues() -> [Venue; 8] {
        [
            Venue::CBCM(),
            Venue::GLBX(),
            Venue::NYUM(),
            Venue::XCBT(),
            Venue::XCEC(),
            Venue::XCME(),
            Venue::XFXS(),
            Venue::XNYM(),
        ]
    }
}
