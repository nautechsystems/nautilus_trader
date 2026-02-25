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

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::option_chain::{AtmSource, StrikeRange},
    identifiers::{ClientId, OptionSeriesId, Venue},
};

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
pub struct SubscribeOptionChain {
    pub series_id: OptionSeriesId,
    pub strike_range: StrikeRange,
    pub atm_source: Option<AtmSource>,
    pub snapshot_interval_ms: u64,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
}

impl SubscribeOptionChain {
    /// Creates a new [`SubscribeOptionChain`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        atm_source: Option<AtmSource>,
        snapshot_interval_ms: u64,
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            series_id,
            strike_range,
            atm_source,
            snapshot_interval_ms,
            command_id,
            ts_init,
            client_id,
            venue,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeOptionChain {
    pub series_id: OptionSeriesId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
}

impl UnsubscribeOptionChain {
    /// Creates a new [`UnsubscribeOptionChain`] instance.
    pub fn new(
        series_id: OptionSeriesId,
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            series_id,
            command_id,
            ts_init,
            client_id,
            venue,
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::*;
    use ustr::Ustr;

    use super::*;

    fn make_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            Ustr::from("BTC"),
            Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    #[rstest]
    fn test_subscribe_option_chain_new() {
        let series_id = make_series_id();
        let strike_range = StrikeRange::AtmRelative {
            strikes_above: 5,
            strikes_below: 5,
        };
        let atm_source = AtmSource::IndexPrice(InstrumentId::from("BTC-PERPETUAL.DERIBIT"));
        let cmd = SubscribeOptionChain::new(
            series_id,
            strike_range,
            Some(atm_source),
            1000,
            UUID4::new(),
            UnixNanos::from(1u64),
            None,
            Some(Venue::new("DERIBIT")),
        );

        assert_eq!(cmd.series_id, series_id);
        assert_eq!(cmd.snapshot_interval_ms, 1000);
    }

    #[rstest]
    fn test_unsubscribe_option_chain_new() {
        let series_id = make_series_id();
        let cmd = UnsubscribeOptionChain::new(
            series_id,
            UUID4::new(),
            UnixNanos::from(1u64),
            None,
            Some(Venue::new("DERIBIT")),
        );

        assert_eq!(cmd.series_id, series_id);
    }
}
