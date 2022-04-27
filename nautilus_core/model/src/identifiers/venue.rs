// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::buffer::{Buffer, Buffer16};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct Venue {
    pub value: Buffer16,
}

impl Venue {
    pub fn from_str(s: &str) -> Venue {
        Venue {
            value: Buffer16::from_str(s),
        }
    }
}

impl Display for Venue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::venue::Venue;

    #[test]
    fn test_venue_from_str() {
        let venue1 = Venue::from_str("FTX");
        let venue2 = Venue::from_str("IDEALPRO");

        assert_eq!(venue1, venue1);
        assert_ne!(venue1, venue2);
        assert_eq!(venue1.to_string(), "FTX")
    }

    #[test]
    fn test_venue_as_str() {
        let venue = Venue::from_str("FTX");

        assert_eq!(venue.to_string(), "FTX")
    }
}
