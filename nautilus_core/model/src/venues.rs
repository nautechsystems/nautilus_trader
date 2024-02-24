// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use once_cell::sync::Lazy;
use ustr::Ustr;

use crate::identifiers::venue::Venue;

static CBCM_LOCK: OnceLock<Venue> = OnceLock::new();
static GLBX_LOCK: OnceLock<Venue> = OnceLock::new();
static NYUM_LOCK: OnceLock<Venue> = OnceLock::new();
static XCBT_LOCK: OnceLock<Venue> = OnceLock::new();
static XCEC_LOCK: OnceLock<Venue> = OnceLock::new();
static XCME_LOCK: OnceLock<Venue> = OnceLock::new();
static XFXS_LOCK: OnceLock<Venue> = OnceLock::new();
static XNYM_LOCK: OnceLock<Venue> = OnceLock::new();

impl Venue {
    #[allow(non_snake_case)]
    pub fn CBCM() -> Self {
        *CBCM_LOCK.get_or_init(|| Self {
            value: Ustr::from("CBCM"),
        })
    }
    #[allow(non_snake_case)]
    pub fn GLBX() -> Self {
        *GLBX_LOCK.get_or_init(|| Self {
            value: Ustr::from("GLBX"),
        })
    }
    #[allow(non_snake_case)]
    pub fn NYUM() -> Self {
        *NYUM_LOCK.get_or_init(|| Self {
            value: Ustr::from("NYUM"),
        })
    }
    #[allow(non_snake_case)]
    pub fn XCBT() -> Self {
        *XCBT_LOCK.get_or_init(|| Self {
            value: Ustr::from("XCBT"),
        })
    }
    #[allow(non_snake_case)]
    pub fn XCEC() -> Self {
        *XCEC_LOCK.get_or_init(|| Self {
            value: Ustr::from("XCEC"),
        })
    }
    #[allow(non_snake_case)]
    pub fn XCME() -> Self {
        *XCME_LOCK.get_or_init(|| Self {
            value: Ustr::from("XCME"),
        })
    }
    #[allow(non_snake_case)]
    pub fn XFXS() -> Self {
        *XFXS_LOCK.get_or_init(|| Self {
            value: Ustr::from("XFXS"),
        })
    }
    #[allow(non_snake_case)]
    pub fn XNYM() -> Self {
        *XNYM_LOCK.get_or_init(|| Self {
            value: Ustr::from("XNYM"),
        })
    }
}

pub static VENUE_MAP: Lazy<Mutex<HashMap<&str, Venue>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(Venue::CBCM().value.as_str(), Venue::CBCM());
    map.insert(Venue::GLBX().value.as_str(), Venue::GLBX());
    map.insert(Venue::NYUM().value.as_str(), Venue::NYUM());
    map.insert(Venue::XCBT().value.as_str(), Venue::XCBT());
    map.insert(Venue::XCEC().value.as_str(), Venue::XCEC());
    map.insert(Venue::XCME().value.as_str(), Venue::XCME());
    map.insert(Venue::XFXS().value.as_str(), Venue::XFXS());
    map.insert(Venue::XNYM().value.as_str(), Venue::XNYM());
    Mutex::new(map)
});
