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

use std::{env, fs, path::PathBuf};

use anyhow::{bail, Result};
use databento::dbn;
use dbn::{
    decode::{dbn::Decoder, DecodeDbn},
    Record,
};
use indexmap::IndexMap;
use nautilus_model::{
    data::Data,
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::currency::Currency,
};
use pyo3::prelude::*;
use streaming_iterator::StreamingIterator;
use time;
use ustr::Ustr;

use super::{parsing::parse_record, types::DatabentoPublisher};

pub type PublisherId = u16;
pub type Dataset = Ustr;

/// Provides a Nautilus data loader for Databento Binary Encoding (DBN) format data.
///
/// # Supported schemas:
///  - MBO -> `OrderBookDelta`
///  - MBP_1 -> `QuoteTick` | `TradeTick`
///  - MBP_10 -> `OrderBookDepth10`
///  - TBBO -> `QuoteTick` | `TradeTick`
///  - TRADES -> `TradeTick`
///  - OHLCV_1S -> `Bar`
///  - OHLCV_1M -> `Bar`
///  - OHLCV_1H -> `Bar`
///  - OHLCV_1D -> `Bar`
///  - DEFINITION -> `Instrument`
///  - IMBALANCE -> `DatabentoImbalance`
///  - STATISTICS -> `DatabentoStatistics`
///
/// For the loader to work correctly, you must first either:
///  - Load Databento instrument definitions from a DBN file using `load_instruments(...)`
///  - Manually add Nautilus instrument objects through `add_instruments(...)`
///
/// # Warnings
/// The following Databento instrument classes are not supported:
///  - ``FUTURE_SPREAD``
///  - ``OPTION_SPEAD``
///  - ``MIXED_SPREAD``
///  - ``FX_SPOT``
///
/// # References
/// https://docs.databento.com/knowledge-base/new-users/dbn-encoding
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoDataLoader {
    publishers: IndexMap<PublisherId, DatabentoPublisher>,
    venue_dataset: IndexMap<Venue, Dataset>,
}

impl DatabentoDataLoader {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let mut loader = Self {
            publishers: IndexMap::new(),
            venue_dataset: IndexMap::new(),
        };

        // Load publishers
        let publishers_path = match path {
            Some(p) => p,
            None => {
                // Use built-in publishers path
                let mut exe_path = env::current_exe()?;
                exe_path.pop();
                exe_path.push("publishers.json");
                exe_path
            }
        };

        loader.load_publishers(publishers_path)?;

        Ok(loader)
    }

    /// Load the publishers data from the file at the given `path`.
    pub fn load_publishers(&mut self, path: PathBuf) -> Result<()> {
        let file_content = fs::read_to_string(path)?;
        let publishers: Vec<DatabentoPublisher> = serde_json::from_str(&file_content)?;

        self.publishers = publishers
            .clone()
            .into_iter()
            .map(|p| (p.publisher_id, p))
            .collect::<IndexMap<u16, DatabentoPublisher>>();

        self.venue_dataset = publishers
            .iter()
            .map(|p| {
                (
                    Venue::from(p.venue.as_str()),
                    Dataset::from(p.dataset.as_str()),
                )
            })
            .collect::<IndexMap<Venue, Ustr>>();

        Ok(())
    }

    /// Return the internal Databento publishers currently held by the loader.
    #[must_use]
    pub fn get_publishers(&self) -> &IndexMap<u16, DatabentoPublisher> {
        &self.publishers
    }

    // Return the dataset which matches the given `venue` (if found).
    #[must_use]
    pub fn get_dataset_for_venue(&self, venue: &Venue) -> Option<&Dataset> {
        self.venue_dataset.get(venue)
    }

    pub fn get_nautilus_instrument_id_for_record(
        &self,
        record: &dbn::RecordRef,
        metadata: &dbn::Metadata,
    ) -> Result<InstrumentId> {
        let (publisher_id, instrument_id, nanoseconds) = match record.rtype()? {
            dbn::RType::Mbo => {
                let msg = record.get::<dbn::MboMsg>().unwrap(); // SAFETY: RType known
                (msg.hd.publisher_id, msg.hd.instrument_id, msg.ts_recv)
            }
            dbn::RType::Mbp0 => {
                let msg = record.get::<dbn::TradeMsg>().unwrap(); // SAFETY: RType known
                (msg.hd.publisher_id, msg.hd.instrument_id, msg.ts_recv)
            }
            dbn::RType::Mbp1 => {
                let msg = record.get::<dbn::Mbp1Msg>().unwrap(); // SAFETY: RType known
                (msg.hd.publisher_id, msg.hd.instrument_id, msg.ts_recv)
            }
            dbn::RType::Mbp10 => {
                let msg = record.get::<dbn::Mbp10Msg>().unwrap(); // SAFETY: RType known
                (msg.hd.publisher_id, msg.hd.instrument_id, msg.ts_recv)
            }
            dbn::RType::Ohlcv1S
            | dbn::RType::Ohlcv1M
            | dbn::RType::Ohlcv1H
            | dbn::RType::Ohlcv1D
            | dbn::RType::OhlcvEod => {
                let msg = record.get::<dbn::OhlcvMsg>().unwrap(); // SAFETY: RType known
                (msg.hd.publisher_id, msg.hd.instrument_id, msg.hd.ts_event)
            }
            _ => bail!("RType is currently unsupported by NautilusTrader"),
        };

        let duration = time::Duration::nanoseconds(nanoseconds as i64);
        let datetime = time::OffsetDateTime::UNIX_EPOCH
            .checked_add(duration)
            .unwrap();
        let date = datetime.date();
        let symbol_map = metadata.symbol_map_for_date(date)?;
        let raw_symbol = symbol_map
            .get(instrument_id)
            .expect("No raw symbol found for {instrument_id}");

        let symbol = Symbol {
            value: Ustr::from(raw_symbol),
        };
        let venue_str = self.publishers.get(&publisher_id).unwrap().venue.as_str();
        let venue = Venue {
            value: Ustr::from(venue_str),
        };

        Ok(InstrumentId::new(symbol, venue))
    }

    pub fn schema_from_file(&self, path: PathBuf) -> Result<Option<dbn::Schema>> {
        let decoder = Decoder::from_zstd_file(path)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema)
    }

    pub fn read_records<T>(
        &self,
        path: PathBuf,
        instrument_id: Option<InstrumentId>,
    ) -> Result<impl Iterator<Item = Result<(Data, Option<Data>)>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(path)?;
        let metadata = decoder.metadata().clone();
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = Currency::USD().precision; // Hard coded for now

        Ok(std::iter::from_fn(move || {
            dbn_stream.advance();
            match dbn_stream.get() {
                Some(record) => {
                    let rec_ref = dbn::RecordRef::from(record);
                    let rtype = rec_ref.rtype().expect("Invalid `rtype` for data loading");
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => self
                            .get_nautilus_instrument_id_for_record(&rec_ref, &metadata)
                            .expect("Error resolving symbology mapping for {rec_ref}"),
                    };

                    match parse_record(&rec_ref, rtype, instrument_id, price_precision, None) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }
}
