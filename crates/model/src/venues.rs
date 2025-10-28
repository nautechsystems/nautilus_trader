// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    sync::{LazyLock, Mutex, OnceLock},
};

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::MUTEX_POISONED;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_venue_constants() {
        // Test that all venue constants return consistent values
        let cbcm1 = Venue::CBCM();
        let cbcm2 = Venue::CBCM();
        assert_eq!(cbcm1, cbcm2);
        assert_eq!(cbcm1.inner().as_str(), "CBCM");

        let glbx1 = Venue::GLBX();
        let glbx2 = Venue::GLBX();
        assert_eq!(glbx1, glbx2);
        assert_eq!(glbx1.inner().as_str(), "GLBX");

        let nyum1 = Venue::NYUM();
        let nyum2 = Venue::NYUM();
        assert_eq!(nyum1, nyum2);
        assert_eq!(nyum1.inner().as_str(), "NYUM");

        let xcbt1 = Venue::XCBT();
        let xcbt2 = Venue::XCBT();
        assert_eq!(xcbt1, xcbt2);
        assert_eq!(xcbt1.inner().as_str(), "XCBT");

        let xcec1 = Venue::XCEC();
        let xcec2 = Venue::XCEC();
        assert_eq!(xcec1, xcec2);
        assert_eq!(xcec1.inner().as_str(), "XCEC");

        let xcme1 = Venue::XCME();
        let xcme2 = Venue::XCME();
        assert_eq!(xcme1, xcme2);
        assert_eq!(xcme1.inner().as_str(), "XCME");

        let xfxs1 = Venue::XFXS();
        let xfxs2 = Venue::XFXS();
        assert_eq!(xfxs1, xfxs2);
        assert_eq!(xfxs1.inner().as_str(), "XFXS");

        let xnym1 = Venue::XNYM();
        let xnym2 = Venue::XNYM();
        assert_eq!(xnym1, xnym2);
        assert_eq!(xnym1.inner().as_str(), "XNYM");
    }

    #[rstest]
    fn test_venue_constants_uniqueness() {
        // Test that all venue constants are different from each other
        let venues = [
            Venue::CBCM(),
            Venue::GLBX(),
            Venue::NYUM(),
            Venue::XCBT(),
            Venue::XCEC(),
            Venue::XCME(),
            Venue::XFXS(),
            Venue::XNYM(),
        ];

        // Check that all venues are unique
        for (i, venue1) in venues.iter().enumerate() {
            for (j, venue2) in venues.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        venue1, venue2,
                        "Venues at indices {i} and {j} should be different"
                    );
                }
            }
        }
    }

    #[rstest]
    fn test_venue_map_contains_all_venues() {
        let venue_map = VENUE_MAP.lock().expect(MUTEX_POISONED);

        // Test that all venue constants are in the map
        assert!(venue_map.contains_key("CBCM"));
        assert!(venue_map.contains_key("GLBX"));
        assert!(venue_map.contains_key("NYUM"));
        assert!(venue_map.contains_key("XCBT"));
        assert!(venue_map.contains_key("XCEC"));
        assert!(venue_map.contains_key("XCME"));
        assert!(venue_map.contains_key("XFXS"));
        assert!(venue_map.contains_key("XNYM"));

        // Test that the map has exactly 8 entries
        assert_eq!(venue_map.len(), 8);
    }

    #[rstest]
    fn test_venue_map_values_match_constants() {
        let venue_map = VENUE_MAP.lock().expect(MUTEX_POISONED);

        // Test that map values match the venue constants
        assert_eq!(venue_map.get("CBCM").unwrap(), &Venue::CBCM());
        assert_eq!(venue_map.get("GLBX").unwrap(), &Venue::GLBX());
        assert_eq!(venue_map.get("NYUM").unwrap(), &Venue::NYUM());
        assert_eq!(venue_map.get("XCBT").unwrap(), &Venue::XCBT());
        assert_eq!(venue_map.get("XCEC").unwrap(), &Venue::XCEC());
        assert_eq!(venue_map.get("XCME").unwrap(), &Venue::XCME());
        assert_eq!(venue_map.get("XFXS").unwrap(), &Venue::XFXS());
        assert_eq!(venue_map.get("XNYM").unwrap(), &Venue::XNYM());
    }

    #[rstest]
    fn test_venue_map_lookup_nonexistent() {
        let venue_map = VENUE_MAP.lock().expect(MUTEX_POISONED);

        // Test that non-existent venues return None
        assert!(venue_map.get("INVALID").is_none());
        assert!(venue_map.get("").is_none());
        assert!(venue_map.get("NYSE").is_none()); // Valid venue but not in our constants
    }

    #[rstest]
    fn test_venue_constants_lazy_initialization() {
        // Test that venue constants work with lazy initialization
        // This implicitly tests that OnceLock works correctly

        // Multiple calls should return the same instance
        let cbcm_calls = (0..10).map(|_| Venue::CBCM()).collect::<Vec<_>>();
        let first_cbcm = cbcm_calls[0];

        for cbcm in cbcm_calls {
            assert_eq!(cbcm, first_cbcm);
        }
    }

    #[rstest]
    fn test_all_venue_strings() {
        // Test the string representations of all venues
        let expected_venues = vec![
            ("CBCM", Venue::CBCM()),
            ("GLBX", Venue::GLBX()),
            ("NYUM", Venue::NYUM()),
            ("XCBT", Venue::XCBT()),
            ("XCEC", Venue::XCEC()),
            ("XCME", Venue::XCME()),
            ("XFXS", Venue::XFXS()),
            ("XNYM", Venue::XNYM()),
        ];

        for (expected_str, venue) in expected_venues {
            assert_eq!(venue.inner().as_str(), expected_str);
            assert_eq!(format!("{venue}"), expected_str);
        }
    }

    #[rstest]
    fn test_venue_constants_thread_safety() {
        use std::thread;

        // Test that venue constants are thread-safe
        let handles: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(|| {
                    // Access all venue constants from different threads
                    let venues = vec![
                        Venue::CBCM(),
                        Venue::GLBX(),
                        Venue::NYUM(),
                        Venue::XCBT(),
                        Venue::XCEC(),
                        Venue::XCME(),
                        Venue::XFXS(),
                        Venue::XNYM(),
                    ];
                    venues
                })
            })
            .collect();

        let results: Vec<Vec<Venue>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All threads should return the same venue instances
        for venues in &results {
            assert_eq!(venues[0], Venue::CBCM());
            assert_eq!(venues[1], Venue::GLBX());
            assert_eq!(venues[2], Venue::NYUM());
            assert_eq!(venues[3], Venue::XCBT());
            assert_eq!(venues[4], Venue::XCEC());
            assert_eq!(venues[5], Venue::XCME());
            assert_eq!(venues[6], Venue::XFXS());
            assert_eq!(venues[7], Venue::XNYM());
        }
    }

    #[rstest]
    fn test_venue_map_thread_safety() {
        use std::thread;

        // Test that VENUE_MAP is thread-safe
        let handles: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(|| {
                    let venue_map = VENUE_MAP.lock().expect(MUTEX_POISONED);
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
}
