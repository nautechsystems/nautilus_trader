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

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Massive market data feed selection.
///
/// Which feed a key can use depends on the subscribed Massive plan; delayed
/// feeds replay the SIP with a 15-minute lag.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.massive",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.massive")
)]
pub enum MassiveDataFeed {
    /// Real-time SIP feed.
    #[default]
    RealTime,
    /// 15-minute delayed feed.
    Delayed,
}

impl MassiveDataFeed {
    /// Returns true if this is the delayed feed.
    #[must_use]
    pub const fn is_delayed(self) -> bool {
        matches!(self, Self::Delayed)
    }
}

/// WebSocket channels on the Massive US stocks cluster.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    AsRefStr,
)]
pub enum MassiveWsChannel {
    /// Tick-level trades (`T.<ticker>`).
    #[serde(rename = "T")]
    #[strum(serialize = "T")]
    Trades,
    /// NBBO quotes (`Q.<ticker>`).
    #[serde(rename = "Q")]
    #[strum(serialize = "Q")]
    Quotes,
    /// Per-second aggregates (`A.<ticker>`).
    #[serde(rename = "A")]
    #[strum(serialize = "A")]
    AggregatesSecond,
    /// Per-minute aggregates (`AM.<ticker>`).
    #[serde(rename = "AM")]
    #[strum(serialize = "AM")]
    AggregatesMinute,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_feed_default_is_realtime() {
        assert_eq!(MassiveDataFeed::default(), MassiveDataFeed::RealTime);
        assert!(!MassiveDataFeed::RealTime.is_delayed());
        assert!(MassiveDataFeed::Delayed.is_delayed());
    }

    #[rstest]
    #[case(MassiveWsChannel::Trades, "T")]
    #[case(MassiveWsChannel::Quotes, "Q")]
    #[case(MassiveWsChannel::AggregatesSecond, "A")]
    #[case(MassiveWsChannel::AggregatesMinute, "AM")]
    fn test_ws_channel_round_trip(#[case] channel: MassiveWsChannel, #[case] s: &str) {
        assert_eq!(channel.as_ref(), s);
        assert_eq!(MassiveWsChannel::from_str(s).unwrap(), channel);
    }
}
