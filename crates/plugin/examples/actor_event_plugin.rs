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

//! Actor cdylib used by live plug-in inbound event-boundary tests.

use std::{fs, path::PathBuf};

use nautilus_model::{
    data::{OptionChainSlice, OrderBookDeltas, QuoteTick},
    identifiers::{InstrumentId, OptionSeriesId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_plugin::prelude::*;

pub struct ActorEventProbe {
    expected_instrument_id: InstrumentId,
    expected_series_id: OptionSeriesId,
    callback_path: Option<PathBuf>,
}

impl PluginActor for ActorEventProbe {
    const TYPE_NAME: &'static str = "ActorEventProbe";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, config_json: &str) -> Self {
        let config = serde_json::from_str::<serde_json::Value>(config_json)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        let expected_instrument_id = config
            .get("instrument_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ETH-USDT.BINANCE");
        let expected_series_id = config
            .get("series_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("DERIBIT:BTC:BTC:1700000000000000000");
        let callback_path = config
            .get("callback_path")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from);

        Self {
            expected_instrument_id: InstrumentId::from(expected_instrument_id),
            expected_series_id: expected_series_id
                .parse()
                .expect("actor event series id config parses"),
            callback_path,
        }
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        self.record_instrument_id(instrument.id())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.record_instrument_id(deltas.instrument_id)
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.record_instrument_id(quote.instrument_id)
    }

    fn on_option_chain(&mut self, chain: &OptionChainSlice) -> anyhow::Result<()> {
        self.record_series_id(chain.series_id)
    }
}

impl ActorEventProbe {
    fn record_instrument_id(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        if instrument_id != self.expected_instrument_id {
            anyhow::bail!(
                "instrument id mismatch: expected {}, received {}",
                self.expected_instrument_id,
                instrument_id
            );
        }

        if let Some(path) = &self.callback_path {
            fs::write(path, instrument_id.to_string())?;
        }
        Ok(())
    }

    fn record_series_id(&self, series_id: OptionSeriesId) -> anyhow::Result<()> {
        if series_id != self.expected_series_id {
            anyhow::bail!(
                "series id mismatch: expected {}, received {}",
                self.expected_series_id,
                series_id
            );
        }

        if let Some(path) = &self.callback_path {
            fs::write(path, series_id.to_wire_string())?;
        }
        Ok(())
    }
}

nautilus_plugin::nautilus_plugin! {
    name: "actor-event-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    actors: [ActorEventProbe],
}

#[allow(dead_code)]
fn main() {}
