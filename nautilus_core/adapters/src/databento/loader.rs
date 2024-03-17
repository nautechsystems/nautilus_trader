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

use databento::dbn;
use dbn::{
    compat::InstrumentDefMsgV1,
    decode::{dbn::Decoder, DbnMetadata, DecodeStream},
};
use indexmap::IndexMap;
use nautilus_model::{
    data::Data,
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    instruments::InstrumentType,
    types::currency::Currency,
};
use streaming_iterator::StreamingIterator;
use ustr::Ustr;

use super::{
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg_v1, decode_record, decode_statistics_msg,
        raw_ptr_to_ustr,
    },
    symbology::decode_nautilus_instrument_id,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, Dataset, PublisherId},
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
/// <https://databento.com/docs/knowledge-base/new-users/dbn-encoding>
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoDataLoader {
    publishers_map: IndexMap<PublisherId, DatabentoPublisher>,
    venue_dataset_map: IndexMap<Venue, Dataset>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
}

impl DatabentoDataLoader {
    pub fn new(path: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut loader = Self {
            publishers_map: IndexMap::new(),
            venue_dataset_map: IndexMap::new(),
            publisher_venue_map: IndexMap::new(),
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
    pub fn load_publishers(&mut self, path: PathBuf) -> anyhow::Result<()> {
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

        self.publisher_venue_map = publishers
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(())
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

    pub fn schema_from_file(&self, path: PathBuf) -> anyhow::Result<Option<String>> {
        let decoder = Decoder::from_zstd_file(path)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema.map(|schema| schema.to_string()))
    }

    pub fn read_definition_records(
        &mut self,
        path: PathBuf,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<InstrumentType>> + '_> {
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

                    let venue = self
                        .publisher_venue_map
                        .get(&msg.hd.publisher_id)
                        .expect("`Venue` not found `publisher_id`");
                    let instrument_id = InstrumentId::new(symbol, *venue);

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
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<(Option<Data>, Option<Data>)>> + '_>
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &metadata,
                            &self.publisher_venue_map,
                        )
                        .unwrap(), // TODO: Panic on error for now
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

    pub fn read_imbalance_records<T>(
        &self,
        path: PathBuf,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoImbalance>> + '_>
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &metadata,
                            &self.publisher_venue_map,
                        )
                        .unwrap(), // TODO: Panic on error for now
                    };

                    let msg = record
                        .get::<dbn::ImbalanceMsg>()
                        .expect("Invalid `ImbalanceMsg`");
                    match decode_imbalance_msg(msg, instrument_id, price_precision, msg.ts_recv) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }

    pub fn read_statistics_records<T>(
        &self,
        path: PathBuf,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoStatistics>> + '_>
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &metadata,
                            &self.publisher_venue_map,
                        )
                        .unwrap(), // TODO: Panic on error for now
                    };

                    let msg = record.get::<dbn::StatMsg>().expect("Invalid `StatMsg`");
                    match decode_statistics_msg(msg, instrument_id, price_precision, msg.ts_recv) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }
}
