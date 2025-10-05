// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    env, fs,
    path::{Path, PathBuf},
};

use ahash::AHashMap;
use anyhow::Context;
use databento::dbn::{self, InstrumentDefMsg};
use dbn::{
    Publisher,
    decode::{DbnMetadata, DecodeStream, dbn::Decoder},
};
use fallible_streaming_iterator::FallibleStreamingIterator;
use indexmap::IndexMap;
use nautilus_model::{
    data::{Bar, Data, InstrumentStatus, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::InstrumentAny,
    types::Currency,
};

use super::{
    decode::{decode_imbalance_msg, decode_record, decode_statistics_msg, decode_status_msg},
    symbology::decode_nautilus_instrument_id,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, Dataset, PublisherId},
};
use crate::{decode::decode_instrument_def_msg, symbology::MetadataCache};

/// A Nautilus data loader for Databento Binary Encoding (DBN) format data.
///
/// # Supported schemas:
///  - `MBO` -> `OrderBookDelta`
///  - `MBP_1` -> `(QuoteTick, Option<TradeTick>)`
///  - `MBP_10` -> `OrderBookDepth10`
///  - `BBO_1S` -> `QuoteTick`
///  - `BBO_1M` -> `QuoteTick`
///  - `CMBP_1` -> `(QuoteTick, Option<TradeTick>)`
///  - `CBBO_1S` -> `QuoteTick`
///  - `CBBO_1M` -> `QuoteTick`
///  - `TCBBO` -> `(QuoteTick, TradeTick)`
///  - `TBBO` -> `(QuoteTick, TradeTick)`
///  - `TRADES` -> `TradeTick`
///  - `OHLCV_1S` -> `Bar`
///  - `OHLCV_1M` -> `Bar`
///  - `OHLCV_1H` -> `Bar`
///  - `OHLCV_1D` -> `Bar`
///  - `OHLCV_EOD` -> `Bar`
///  - `DEFINITION` -> `Instrument`
///  - `IMBALANCE` -> `DatabentoImbalance`
///  - `STATISTICS` -> `DatabentoStatistics`
///  - `STATUS` -> `InstrumentStatus`
///
/// # References
///
/// <https://databento.com/docs/schemas-and-data-formats>
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Debug)]
pub struct DatabentoDataLoader {
    publishers_map: IndexMap<PublisherId, DatabentoPublisher>,
    venue_dataset_map: IndexMap<Venue, Dataset>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    symbol_venue_map: AHashMap<Symbol, Venue>,
}

impl DatabentoDataLoader {
    /// Creates a new [`DatabentoDataLoader`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if locating or loading publishers data fails.
    pub fn new(publishers_filepath: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut loader = Self {
            publishers_map: IndexMap::new(),
            venue_dataset_map: IndexMap::new(),
            publisher_venue_map: IndexMap::new(),
            symbol_venue_map: AHashMap::new(),
        };

        // Load publishers
        let publishers_filepath = if let Some(p) = publishers_filepath {
            p
        } else {
            // Use built-in publishers path
            let mut exe_path = env::current_exe()?;
            exe_path.pop();
            exe_path.push("publishers.json");
            exe_path
        };

        loader
            .load_publishers(publishers_filepath)
            .context("Error loading publishers.json")?;

        Ok(loader)
    }

    /// Load the publishers data from the file at the given `filepath`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed as JSON.
    pub fn load_publishers(&mut self, filepath: PathBuf) -> anyhow::Result<()> {
        let file_content = fs::read_to_string(filepath)?;
        let publishers: Vec<DatabentoPublisher> = serde_json::from_str(&file_content)?;

        self.publishers_map = publishers
            .clone()
            .into_iter()
            .map(|p| (p.publisher_id, p))
            .collect();

        let mut venue_dataset_map = IndexMap::new();

        // Only insert a dataset if the venue key is not already in the map
        for publisher in &publishers {
            let venue = Venue::from(publisher.venue.as_str());
            let dataset = Dataset::from(publisher.dataset.as_str());
            venue_dataset_map.entry(venue).or_insert(dataset);
        }

        self.venue_dataset_map = venue_dataset_map;

        // Insert CME Globex exchanges
        let glbx = Dataset::from("GLBX.MDP3");
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
            .collect();

        Ok(())
    }

    /// Returns the internal Databento publishers currently held by the loader.
    #[must_use]
    pub const fn get_publishers(&self) -> &IndexMap<u16, DatabentoPublisher> {
        &self.publishers_map
    }

    /// Sets the `venue` to map to the given `dataset`.
    pub fn set_dataset_for_venue(&mut self, dataset: Dataset, venue: Venue) {
        _ = self.venue_dataset_map.insert(venue, dataset);
    }

    /// Returns the dataset which matches the given `venue` (if found).
    #[must_use]
    pub fn get_dataset_for_venue(&self, venue: &Venue) -> Option<&Dataset> {
        self.venue_dataset_map.get(venue)
    }

    /// Returns the venue which matches the given `publisher_id` (if found).
    #[must_use]
    pub fn get_venue_for_publisher(&self, publisher_id: PublisherId) -> Option<&Venue> {
        self.publisher_venue_map.get(&publisher_id)
    }

    /// Returns the schema for the given `filepath`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be decoded or metadata retrieval fails.
    pub fn schema_from_file(&self, filepath: &Path) -> anyhow::Result<Option<String>> {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema.map(|schema| schema.to_string()))
    }

    /// Reads instrument definition records from a DBN file.
    ///
    /// # Errors
    ///
    /// Returns an error if decoding the definition records fails.
    pub fn read_definition_records(
        &mut self,
        filepath: &Path,
        use_exchange_as_venue: bool,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<InstrumentAny>> + '_> {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let mut dbn_stream = decoder.decode_stream::<InstrumentDefMsg>();

        Ok(std::iter::from_fn(move || {
            let result: anyhow::Result<Option<InstrumentAny>> = (|| {
                dbn_stream
                    .advance()
                    .map_err(|e| anyhow::anyhow!("Stream advance error: {e}"))?;

                if let Some(rec) = dbn_stream.get() {
                    let record = dbn::RecordRef::from(rec);
                    let msg = record
                        .get::<InstrumentDefMsg>()
                        .ok_or_else(|| anyhow::anyhow!("Failed to decode InstrumentDefMsg"))?;

                    // Symbol and venue resolution
                    let raw_symbol = rec
                        .raw_symbol()
                        .map_err(|e| anyhow::anyhow!("Error decoding `raw_symbol`: {e}"))?;
                    let symbol = Symbol::from(raw_symbol);

                    let publisher = rec
                        .hd
                        .publisher()
                        .map_err(|e| anyhow::anyhow!("Invalid `publisher` for record: {e}"))?;
                    let venue = match publisher {
                        Publisher::GlbxMdp3Glbx if use_exchange_as_venue => {
                            let exchange = rec.exchange().map_err(|e| {
                                anyhow::anyhow!("Missing `exchange` for record: {e}")
                            })?;
                            let venue = Venue::from_code(exchange).map_err(|e| {
                                anyhow::anyhow!("Venue not found for exchange {exchange}: {e}")
                            })?;
                            self.symbol_venue_map.insert(symbol, venue);
                            venue
                        }
                        _ => *self
                            .publisher_venue_map
                            .get(&msg.hd.publisher_id)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Venue not found for publisher_id {}",
                                    msg.hd.publisher_id
                                )
                            })?,
                    };
                    let instrument_id = InstrumentId::new(symbol, venue);
                    let ts_init = msg.ts_recv.into();

                    let data = decode_instrument_def_msg(rec, instrument_id, Some(ts_init))?;
                    Ok(Some(data))
                } else {
                    // No more records
                    Ok(None)
                }
            })();

            match result {
                Ok(Some(item)) => Some(Ok(item)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        }))
    }

    /// Reads and decodes market data records from a DBN file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading records fails.
    pub fn read_records<T>(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
        include_trades: bool,
        bars_timestamp_on_close: Option<bool>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<(Option<Data>, Option<Data>)>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);

        Ok(std::iter::from_fn(move || {
            let result: anyhow::Result<Option<(Option<Data>, Option<Data>)>> = (|| {
                dbn_stream
                    .advance()
                    .map_err(|e| anyhow::anyhow!("Stream advance error: {e}"))?;
                if let Some(rec) = dbn_stream.get() {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = if let Some(id) = &instrument_id {
                        *id
                    } else {
                        decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        )
                        .context("Failed to decode instrument id")?
                    };
                    let (item1, item2) = decode_record(
                        &record,
                        instrument_id,
                        price_precision,
                        None,
                        include_trades,
                        bars_timestamp_on_close.unwrap_or(true),
                    )?;
                    Ok(Some((item1, item2)))
                } else {
                    Ok(None)
                }
            })();
            match result {
                Ok(Some(v)) => Some(Ok(v)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        }))
    }

    /// Loads all instrument definitions from a DBN file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading instruments fails.
    pub fn load_instruments(
        &mut self,
        filepath: &Path,
        use_exchange_as_venue: bool,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.read_definition_records(filepath, use_exchange_as_venue)?
            .collect::<Result<Vec<_>, _>>()
    }

    /// Loads order book delta messages from a DBN MBO schema file.
    ///
    /// Cannot include trades.
    ///
    /// # Errors
    ///
    /// Returns an error if loading order book deltas fails.
    pub fn load_order_book_deltas(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        self.read_records::<dbn::MboMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Delta(delta) = item1 {
                        Some(Ok(delta))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads order book depth10 snapshots from a DBN MBP-10 schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading order book depth10 fails.
    pub fn load_order_book_depth10(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<OrderBookDepth10>> {
        self.read_records::<dbn::Mbp10Msg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Depth10(depth) = item1 {
                        Some(Ok(*depth))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads quote tick messages from a DBN MBP-1 or TBBO schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading quotes fails.
    pub fn load_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::Mbp1Msg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Quote(quote) = item1 {
                        Some(Ok(quote))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads best bid/offer quote messages from a DBN BBO schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading BBO quotes fails.
    pub fn load_bbo_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::BboMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Quote(quote) = item1 {
                        Some(Ok(quote))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads consolidated MBP-1 quote messages from a DBN CMBP-1 schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading consolidated MBP-1 quotes fails.
    pub fn load_cmbp_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::Cmbp1Msg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Quote(quote) = item1 {
                        Some(Ok(quote))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads consolidated best bid/offer quote messages from a DBN CBBO schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading consolidated BBO quotes fails.
    pub fn load_cbbo_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::CbboMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Quote(quote) = item1 {
                        Some(Ok(quote))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads trade messages from a DBN TBBO schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading TBBO trades fails.
    pub fn load_tbbo_trades(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TbboMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((_, maybe_item2)) => {
                    if let Some(Data::Trade(trade)) = maybe_item2 {
                        Some(Ok(trade))
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads trade messages from a DBN TCBBO schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading TCBBO trades fails.
    pub fn load_tcbbo_trades(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::CbboMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((_, maybe_item2)) => {
                    if let Some(Data::Trade(trade)) = maybe_item2 {
                        Some(Ok(trade))
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads trade messages from a DBN TRADES schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading trades fails.
    pub fn load_trades(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TradeMsg>(filepath, instrument_id, price_precision, false, None)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Trade(trade) = item1 {
                        Some(Ok(trade))
                    } else {
                        None
                    }
                }
                Ok((None, _)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect()
    }

    /// Loads OHLCV bar messages from a DBN OHLCV schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading bars fails.
    pub fn load_bars(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
        timestamp_on_close: Option<bool>,
    ) -> anyhow::Result<Vec<Bar>> {
        self.read_records::<dbn::OhlcvMsg>(
            filepath,
            instrument_id,
            price_precision,
            false,
            timestamp_on_close,
        )?
        .filter_map(|result| match result {
            Ok((Some(item1), _)) => {
                if let Data::Bar(bar) = item1 {
                    Some(Ok(bar))
                } else {
                    None
                }
            }
            Ok((None, _)) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
    }

    /// Loads instrument status messages from a DBN STATUS schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading status records fails.
    pub fn load_status_records<T>(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<InstrumentStatus>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut dbn_stream = decoder.decode_stream::<T>();

        Ok(std::iter::from_fn(move || {
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => match decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        ) {
                            Ok(id) => id,
                            Err(e) => return Some(Err(e)),
                        },
                    };

                    let msg = match record.get::<dbn::StatusMsg>() {
                        Some(m) => m,
                        None => return Some(Err(anyhow::anyhow!("Invalid `StatusMsg`"))),
                    };
                    let ts_init = msg.ts_recv.into();

                    match decode_status_msg(msg, instrument_id, Some(ts_init)) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }

    /// Reads imbalance messages from a DBN IMBALANCE schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading imbalance records fails.
    pub fn read_imbalance_records<T>(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoImbalance>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);

        Ok(std::iter::from_fn(move || {
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => match decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        ) {
                            Ok(id) => id,
                            Err(e) => return Some(Err(e)),
                        },
                    };

                    let msg = match record.get::<dbn::ImbalanceMsg>() {
                        Some(m) => m,
                        None => return Some(Err(anyhow::anyhow!("Invalid `ImbalanceMsg`"))),
                    };
                    let ts_init = msg.ts_recv.into();

                    match decode_imbalance_msg(msg, instrument_id, price_precision, Some(ts_init)) {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }

    /// Reads statistics messages from a DBN STATISTICS schema file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading statistics records fails.
    pub fn read_statistics_records<T>(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoStatistics>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);

        Ok(std::iter::from_fn(move || {
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => match decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        ) {
                            Ok(id) => id,
                            Err(e) => return Some(Err(e)),
                        },
                    };
                    let msg = match record.get::<dbn::StatMsg>() {
                        Some(m) => m,
                        None => return Some(Err(anyhow::anyhow!("Invalid `StatMsg`"))),
                    };
                    let ts_init = msg.ts_recv.into();

                    match decode_statistics_msg(msg, instrument_id, price_precision, Some(ts_init))
                    {
                        Ok(data) => Some(Ok(data)),
                        Err(e) => Some(Err(e)),
                    }
                }
                None => None,
            }
        }))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use nautilus_model::types::{Price, Quantity};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;

    fn test_data_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    #[fixture]
    fn loader() -> DatabentoDataLoader {
        let publishers_filepath = Path::new(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
        DatabentoDataLoader::new(Some(publishers_filepath)).unwrap()
    }

    // TODO: Improve the below assertions that we've actually read the records we expected

    #[rstest]
    fn test_set_dataset_venue_mapping(mut loader: DatabentoDataLoader) {
        let dataset = Ustr::from("EQUS.PLUS");
        let venue = Venue::from("XNAS");
        loader.set_dataset_for_venue(dataset, venue);

        let result = loader.get_dataset_for_venue(&venue).unwrap();
        assert_eq!(*result, dataset);
    }

    #[rstest]
    #[case(test_data_path().join("test_data.definition.dbn.zst"))]
    fn test_load_instruments(mut loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instruments = loader.load_instruments(&path, false).unwrap();

        assert_eq!(instruments.len(), 2);
    }

    #[rstest]
    fn test_load_order_book_deltas(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.mbo.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let deltas = loader
            .load_order_book_deltas(&path, Some(instrument_id), None)
            .unwrap();

        assert_eq!(deltas.len(), 2);
    }

    #[rstest]
    fn test_load_order_book_depth10(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.mbp-10.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let depths = loader
            .load_order_book_depth10(&path, Some(instrument_id), None)
            .unwrap();

        assert_eq!(depths.len(), 2);
    }

    #[rstest]
    fn test_load_quotes(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.mbp-1.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader
            .load_quotes(&path, Some(instrument_id), None)
            .unwrap();

        assert_eq!(quotes.len(), 2);
    }

    #[rstest]
    #[case(test_data_path().join("test_data.bbo-1s.dbn.zst"))]
    #[case(test_data_path().join("test_data.bbo-1m.dbn.zst"))]
    fn test_load_bbo_quotes(loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader
            .load_bbo_quotes(&path, Some(instrument_id), None)
            .unwrap();

        assert_eq!(quotes.len(), 4);
    }

    #[rstest]
    fn test_load_cmbp_quotes(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.cmbp-1.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader
            .load_cmbp_quotes(&path, Some(instrument_id), None)
            .unwrap();

        // Verify exact data count
        assert_eq!(quotes.len(), 2);

        // Verify first quote fields
        let first_quote = &quotes[0];
        assert_eq!(first_quote.instrument_id, instrument_id);
        assert_eq!(first_quote.bid_price, Price::from("3720.25"));
        assert_eq!(first_quote.ask_price, Price::from("3720.50"));
        assert_eq!(first_quote.bid_size, Quantity::from(24));
        assert_eq!(first_quote.ask_size, Quantity::from(11));
        assert_eq!(first_quote.ts_event, 1609160400006136329);
        assert_eq!(first_quote.ts_init, 1609160400006136329);
    }

    #[rstest]
    fn test_load_cbbo_quotes(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader
            .load_cbbo_quotes(&path, Some(instrument_id), None)
            .unwrap();

        // Verify exact data count
        assert_eq!(quotes.len(), 2);

        // Verify first quote fields
        let first_quote = &quotes[0];
        assert_eq!(first_quote.instrument_id, instrument_id);
        assert_eq!(first_quote.bid_price, Price::from("3720.25"));
        assert_eq!(first_quote.ask_price, Price::from("3720.50"));
        assert_eq!(first_quote.bid_size, Quantity::from(24));
        assert_eq!(first_quote.ask_size, Quantity::from(11));
        assert_eq!(first_quote.ts_event, 1609160400006136329);
        assert_eq!(first_quote.ts_init, 1609160400006136329);
    }

    #[rstest]
    fn test_load_tbbo_trades(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.tbbo.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let trades = loader
            .load_tbbo_trades(&path, Some(instrument_id), None)
            .unwrap();

        // TBBO test data doesn't contain valid trade data (size/price may be 0)
        assert_eq!(trades.len(), 0);
    }

    #[rstest]
    fn test_load_tcbbo_trades(loader: DatabentoDataLoader) {
        // Since we don't have dedicated TCBBO test data, we'll use CBBO data
        // In practice, TCBBO would be CBBO messages with trade data
        let path = test_data_path().join("test_data.cbbo-1s.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let result = loader.load_tcbbo_trades(&path, Some(instrument_id), None);

        assert!(result.is_ok());
        let trades = result.unwrap();
        assert_eq!(trades.len(), 2);
    }

    #[rstest]
    fn test_load_trades(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.trades.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let trades = loader
            .load_trades(&path, Some(instrument_id), None)
            .unwrap();

        assert_eq!(trades.len(), 2);
    }

    #[rstest]
    // #[case(test_data_path().join("test_data.ohlcv-1d.dbn.zst"))]  // TODO: Empty file (0 records)
    #[case(test_data_path().join("test_data.ohlcv-1h.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1m.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"))]
    fn test_load_bars(loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bars = loader
            .load_bars(&path, Some(instrument_id), None, None)
            .unwrap();

        assert_eq!(bars.len(), 2);
    }

    #[rstest]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"))]
    fn test_load_bars_timestamp_on_close_true(loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bars = loader
            .load_bars(&path, Some(instrument_id), None, Some(true))
            .unwrap();

        assert_eq!(bars.len(), 2);

        // When bars_timestamp_on_close is true, both ts_event and ts_init should be close time
        for bar in &bars {
            assert_eq!(
                bar.ts_event, bar.ts_init,
                "ts_event and ts_init should both be close time when bars_timestamp_on_close=true"
            );
        }
    }

    #[rstest]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"))]
    fn test_load_bars_timestamp_on_close_false(loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bars = loader
            .load_bars(&path, Some(instrument_id), None, Some(false))
            .unwrap();

        assert_eq!(bars.len(), 2);

        // When bars_timestamp_on_close is false, ts_event is open time and ts_init is close time
        for bar in &bars {
            assert_ne!(
                bar.ts_event, bar.ts_init,
                "ts_event should be open time and ts_init should be close time when bars_timestamp_on_close=false"
            );
            // For 1-second bars, ts_init (close) should be 1 second after ts_event (open)
            assert_eq!(bar.ts_init.as_u64(), bar.ts_event.as_u64() + 1_000_000_000);
        }
    }

    #[rstest]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"), 0)]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"), 1)]
    fn test_load_bars_timestamp_comparison(
        loader: DatabentoDataLoader,
        #[case] path: PathBuf,
        #[case] bar_index: usize,
    ) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let bars_close = loader
            .load_bars(&path, Some(instrument_id), None, Some(true))
            .unwrap();

        let bars_open = loader
            .load_bars(&path, Some(instrument_id), None, Some(false))
            .unwrap();

        assert_eq!(bars_close.len(), bars_open.len());
        assert_eq!(bars_close.len(), 2);

        let bar_close = &bars_close[bar_index];
        let bar_open = &bars_open[bar_index];

        // Bars should have the same OHLCV data
        assert_eq!(bar_close.open, bar_open.open);
        assert_eq!(bar_close.high, bar_open.high);
        assert_eq!(bar_close.low, bar_open.low);
        assert_eq!(bar_close.close, bar_open.close);
        assert_eq!(bar_close.volume, bar_open.volume);

        // The close-timestamped bar should have later timestamp than open-timestamped bar
        // For 1-second bars, this should be exactly 1 second difference
        assert!(
            bar_close.ts_event > bar_open.ts_event,
            "Close-timestamped bar should have later timestamp than open-timestamped bar"
        );

        // The difference should be exactly 1 second (1_000_000_000 nanoseconds) for 1s bars
        const ONE_SECOND_NS: u64 = 1_000_000_000;
        assert_eq!(
            bar_close.ts_event.as_u64() - bar_open.ts_event.as_u64(),
            ONE_SECOND_NS,
            "Timestamp difference should be exactly 1 second for 1s bars"
        );
    }
}
