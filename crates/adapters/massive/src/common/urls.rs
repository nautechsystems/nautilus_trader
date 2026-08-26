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

use super::{
    consts::{REST_URL, WS_URL_DELAYED, WS_URL_REALTIME},
    enums::MassiveDataFeed,
};

pub fn rest_url() -> &'static str {
    REST_URL
}

pub fn ws_url(feed: MassiveDataFeed) -> &'static str {
    match feed {
        MassiveDataFeed::RealTime => WS_URL_REALTIME,
        MassiveDataFeed::Delayed => WS_URL_DELAYED,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_rest_url() {
        assert_eq!(rest_url(), REST_URL);
    }

    #[rstest]
    fn test_ws_url_realtime() {
        assert_eq!(ws_url(MassiveDataFeed::RealTime), WS_URL_REALTIME);
    }

    #[rstest]
    fn test_ws_url_delayed() {
        assert_eq!(ws_url(MassiveDataFeed::Delayed), WS_URL_DELAYED);
    }
}
