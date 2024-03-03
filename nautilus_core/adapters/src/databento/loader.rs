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

use std::{collections::HashMap, env, fs, path::PathBuf};

use anyhow::{bail, Result};
use dbn::{
    compat::InstrumentDefMsgV1,
    decode::{dbn::Decoder, DbnMetadata, DecodeStream},
    Publisher, Record,
};
use indexmap::IndexMap;
use nautilus_model::{
    data::Data,
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    instruments::Instrument,
    types::currency::Currency,
};
use streaming_iterator::StreamingIterator;
use ustr::Ustr;

use super::{
    decode::{decode_instrument_def_msg_v1, decode_record, raw_ptr_to_ustr},
    types::{DatabentoPublisher, Dataset, PublisherId},
};

/// Provides a Nautilus data loader for Databento Binary Encoding (DBN) format data.
///
/// # Supported schemas:
///  - MBO -> `OrderBookDelta`
///  - MBP_1 -> `QuoteTick` + `TradeTick`
///  - MBP_10 -> `OrderBookDepth10`
///  - TBBO -> `QuoteTick` + `TradeTick`
///  - TRADES -> `TradeTick`
///  - OHLCV_1S -> `Bar`
///  - OHLCV_1M -> `Bar`
///  - OHLCV_1H -> `Bar`
///  - OHLCV_1D -> `Bar`
///  - DEFINITION -> `Instrument`
///  - IMBALANCE -> `DatabentoImbalance`
///  - STATISTICS -> `DatabentoStatistics`
///
/// # References
/// <https://docs.databento.com/knowledge-base/new-users/dbn-encoding>
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoDataLoader {
    publishers_map: IndexMap<PublisherId, DatabentoPublisher>,
    venue_dataset_map: IndexMap<Venue, Dataset>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    glbx_exchange_map: HashMap<Symbol, Venue>,
}

impl DatabentoDataLoader {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let mut loader = Self {
            publishers_map: IndexMap::new(),
            venue_dataset_map: IndexMap::new(),
            publisher_venue_map: IndexMap::new(),
            glbx_exchange_map: HashMap::new(),
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

        self.publishers_map = publishers
            .clone()
            .into_iter()
            .map(|p| (p.publisher_id, p))
            .collect::<IndexMap<u16, DatabentoPublisher>>();

        self.venue_dataset_map = publishers
            .iter()
            .map(|p| {
                (
                    Venue::from(p.venue.as_str()),
                    Dataset::from(p.dataset.as_str()),
                )
            })
            .collect::<IndexMap<Venue, Ustr>>();

        // Insert CME Globex exchanges
        let glbx = Dataset::from("GLBX");
        self.venue_dataset_map.insert(Venue::CBCM(), glbx);
        self.venue_dataset_map.insert(Venue::GLBX(), glbx);
        self.venue_dataset_map.insert(Venue::NYUM(), glbx);
        self.venue_dataset_map.insert(Venue::XCBT(), glbx);
        self.venue_dataset_map.insert(Venue::XCEC(), glbx);
        self.venue_dataset_map.insert(Venue::XCME(), glbx);
        self.venue_dataset_map.insert(Venue::XFXS(), glbx);
        self.venue_dataset_map.insert(Venue::XNYM(), glbx);

        self.publisher_venue_map = publishers
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(())
    }

    // Return the map of CME Globex symbols to exchange venues.
    pub fn load_glbx_exchange_map(&mut self, map: HashMap<Symbol, Venue>) {
        self.glbx_exchange_map = map;
    }

    /// Return the internal Databento publishers currently held by the loader.
    #[must_use]
    pub fn get_publishers(&self) -> &IndexMap<u16, DatabentoPublisher> {
        &self.publishers_map
    }

    // Return the dataset which matches the given `venue` (if found).
    #[must_use]
    pub fn get_dataset_for_venue(&self, venue: &Venue) -> Option<&Dataset> {
        self.venue_dataset_map.get(venue)
    }

    // Return the venue which matches the given `publisher_id` (if found).
    #[must_use]
    pub fn get_venue_for_publisher(&self, publisher_id: PublisherId) -> Option<&Venue> {
        self.publisher_venue_map.get(&publisher_id)
    }

    // Return the venue which matches the given `publisher_id` (if found).
    #[must_use]
    pub fn get_glbx_exchange_map(&self) -> HashMap<Symbol, Venue> {
        self.glbx_exchange_map.clone()
    }

    pub fn get_nautilus_instrument_id_for_record(
        &self,
        record: &dbn::RecordRef,
        metadata: &dbn::Metadata,
        venue: Venue,
    ) -> Result<InstrumentId> {
        let (instrument_id, nanoseconds) = if let Some(msg) = record.get::<dbn::MboMsg>() {
            (msg.hd.instrument_id, msg.ts_recv)
        } else if let Some(msg) = record.get::<dbn::TradeMsg>() {
            (msg.hd.instrument_id, msg.ts_recv)
        } else if let Some(msg) = record.get::<dbn::Mbp1Msg>() {
            (msg.hd.instrument_id, msg.ts_recv)
        } else if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
            (msg.hd.instrument_id, msg.ts_recv)
        } else if let Some(msg) = record.get::<dbn::OhlcvMsg>() {
            (msg.hd.instrument_id, msg.hd.ts_event)
        } else {
            bail!("DBN message type is not currently supported")
        };

        let duration = time::Duration::nanoseconds(nanoseconds as i64);
        let datetime = time::OffsetDateTime::UNIX_EPOCH
            .checked_add(duration)
            .unwrap(); // SAFETY: Relying on correctness of record timestamps
        let date = datetime.date();
        let symbol_map = metadata.symbol_map_for_date(date)?;
        let raw_symbol = symbol_map
            .get(instrument_id)
            .expect("No raw symbol found for {instrument_id}");

        let symbol = Symbol::from_str_unchecked(raw_symbol);

        Ok(InstrumentId::new(symbol, venue))
    }

    pub fn schema_from_file(&self, path: PathBuf) -> Result<Option<String>> {
        let decoder = Decoder::from_zstd_file(path)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema.map(|schema| schema.to_string()))
    }

    pub fn read_definition_records(
        &mut self,
        path: PathBuf,
    ) -> Result<impl Iterator<Item = Result<Box<dyn Instrument>>> + '_> {
        let mut decoder = Decoder::from_zstd_file(path)?;
        decoder.set_upgrade_policy(dbn::VersionUpgradePolicy::Upgrade);
        let mut dbn_stream = decoder.decode_stream::<InstrumentDefMsgV1>();

        Ok(std::iter::from_fn(move || {
            dbn_stream.advance();

            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let msg = record.get::<InstrumentDefMsgV1>().unwrap();

                    let raw_symbol = unsafe {
                        raw_ptr_to_ustr(rec.raw_symbol.as_ptr())
                            .expect("Error obtaining `raw_symbol` pointer")
                    };
                    let symbol = Symbol { value: raw_symbol };

                    let publisher = rec.hd.publisher().expect("Invalid `publisher` for record");
                    let venue = match publisher {
                        Publisher::GlbxMdp3Glbx => {
                            // SAFETY: GLBX instruments have a valid `exchange` field
                            let exchange = rec.exchange().unwrap();
                            let venue = Venue::from_code(exchange).unwrap_or_else(|_| {
                                panic!("`Venue` not found for exchange {exchange}")
                            });
                            self.glbx_exchange_map.insert(symbol, venue);
                            venue
                        }
                        _ => *self
                            .publisher_venue_map
                            .get(&msg.hd.publisher_id)
                            .expect("`Venue` not found `publisher_id`"),
                    };
                    let instrument_id = InstrumentId::new(symbol, venue);

                    match decode_instrument_def_msg_v1(rec, instrument_id, msg.ts_recv) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }

    pub fn read_records<T>(
        &self,
        path: PathBuf,
        instrument_id: Option<InstrumentId>,
        include_trades: bool,
    ) -> Result<impl Iterator<Item = Result<(Option<Data>, Option<Data>)>> + '_>
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
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => {
                            let publisher =
                                record.publisher().expect("Invalid `publisher` for record");
                            let publisher_id = publisher as PublisherId;
                            let venue =
                                self.publisher_venue_map
                                    .get(&publisher_id)
                                    .unwrap_or_else(|| {
                                        panic!(
                                            "`Venue` not found for `publisher_id` {publisher_id}"
                                        )
                                    });
                            let mut instrument_id = self
                                .get_nautilus_instrument_id_for_record(&record, &metadata, *venue)
                                .unwrap_or_else(|_| {
                                    panic!("Error resolving symbology mapping for {record:?}")
                                });

                            if publisher == Publisher::GlbxMdp3Glbx {
                                // Source actual exchange from GLBX instrument
                                // definitions if they were loaded.
                                if let Some(venue) =
                                    self.glbx_exchange_map.get(&instrument_id.symbol)
                                {
                                    instrument_id.venue = *venue;
                                }
                            };

                            instrument_id
                        }
                    };

                    match decode_record(
                        &record,
                        instrument_id,
                        price_precision,
                        None,
                        include_trades,
                    ) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }
}
