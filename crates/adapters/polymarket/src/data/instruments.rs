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

use std::sync::Arc;

use dashmap::DashMap;
use nautilus_common::messages::DataEvent;
use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use crate::{filters::InstrumentFilter, http::gamma::PolymarketGammaHttpClient};

#[derive(Clone, Copy, Debug)]
pub(super) struct TokenMeta {
    pub(super) instrument_id: InstrumentId,
    pub(super) price_precision: u8,
    pub(super) size_precision: u8,
}

// Inserts `instrument` into the live instrument cache and updates the
// `token_meta` routing index in one step. Every path that populates the live
// cache must go through here so WS messages can always resolve token_id back
// to an InstrumentId.
pub(super) fn cache_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) {
    let instrument_id = instrument.id();
    token_meta.insert(
        Ustr::from(instrument.raw_symbol().as_str()),
        TokenMeta {
            instrument_id,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
        },
    );
    instruments.insert(instrument_id, instrument.clone());
}

pub(super) fn cache_and_publish_instruments(
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Vec<InstrumentAny>,
) -> usize {
    let total = instruments.len();

    for instrument in instruments {
        let instrument_id = instrument.id();
        cache_instrument(instruments_cache, token_meta, &instrument);

        if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
            log::warn!("Failed to publish instrument {instrument_id}: {e}");
        }
    }

    total
}

pub(super) async fn refresh_scoped_instruments(
    http_client: PolymarketGammaHttpClient,
    instrument_config: Option<crate::config::PolymarketInstrumentProviderConfig>,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> anyhow::Result<usize> {
    let Some(instrument_config) = instrument_config else {
        return Ok(0);
    };
    let refreshed =
        crate::providers::fetch_configured_instruments(&http_client, &instrument_config, &filters)
            .await?;

    Ok(cache_and_publish_instruments(
        instruments_cache,
        token_meta,
        data_sender,
        refreshed,
    ))
}
