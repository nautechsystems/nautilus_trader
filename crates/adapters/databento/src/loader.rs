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
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use databento::dbn;
use dbn::{
    Publisher,
    compat::InstrumentDefMsgV1,
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
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg_v1, decode_record, decode_statistics_msg,
        decode_status_msg,
    },
    symbology::decode_nautilus_instrument_id,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, Dataset, PublisherId},
};
use crate::symbology::MetadataCache;

/// A Nautilus data loader for Databento Binary Encoding (DBN) format data.
///
/// # Supported schemas:
///  - MBO -> `OrderBookDelta`
///  - MBP_1 -> `(QuoteTick, Option<TradeTick>)`
///  - MBP_10 -> `OrderBookDepth10`
///  - BBO_1S -> `QuoteTick`
///  - BBO_1M -> `QuoteTick`
///  - TBBO -> `(QuoteTick, TradeTick)`
///  - TRADES -> `TradeTick`
///  - OHLCV_1S -> `Bar`
///  - OHLCV_1M -> `Bar`
///  - OHLCV_1H -> `Bar`
///  - OHLCV_1D -> `Bar`
///  - DEFINITION -> `Instrument`
///  - IMBALANCE -> `DatabentoImbalance`
///  - STATISTICS -> `DatabentoStatistics`
///  - STATUS -> `InstrumentStatus`
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
    symbol_venue_map: HashMap<Symbol, Venue>,
}

impl DatabentoDataLoader {
    /// Creates a new [`DatabentoDataLoader`] instance.
    pub fn new(publishers_filepath: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut loader = Self {
            publishers_map: IndexMap::new(),
            venue_dataset_map: IndexMap::new(),
            publisher_venue_map: IndexMap::new(),
            symbol_venue_map: HashMap::new(),
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
            .unwrap_or_else(|e| panic!("Error loading publishers.json: {e}"));

        Ok(loader)
    }

    /// Load the publishers data from the file at the given `filepath`.
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
    pub fn schema_from_file(&self, filepath: &Path) -> anyhow::Result<Option<String>> {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema.map(|schema| schema.to_string()))
    }

    pub fn read_definition_records(
        &mut self,
        filepath: &Path,
        use_exchange_as_venue: bool,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<InstrumentAny>> + '_> {
        let mut decoder = Decoder::from_zstd_file(filepath)?;

        // Setting the policy to decode v1 data in its original format,
        // rather than upgrading to v2 for now (decoding tests fail on `UpgradeToV2`).
        let upgrade_policy = dbn::VersionUpgradePolicy::AsIs;
        decoder.set_upgrade_policy(upgrade_policy);

        let mut dbn_stream = decoder.decode_stream::<InstrumentDefMsgV1>();

        Ok(std::iter::from_fn(move || {
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let msg = record.get::<InstrumentDefMsgV1>().unwrap();

                    let raw_symbol = rec.raw_symbol().expect("Error decoding `raw_symbol`");
                    let symbol = Symbol::from(raw_symbol);

                    let publisher = rec.hd.publisher().expect("Invalid `publisher` for record");
                    let venue = match publisher {
                        Publisher::GlbxMdp3Glbx if use_exchange_as_venue => {
                            // SAFETY: GLBX instruments have a valid `exchange` field
                            let exchange = rec.exchange().unwrap();
                            let venue = Venue::from_code(exchange).unwrap_or_else(|_| {
                                panic!("`Venue` not found for exchange {exchange}")
                            });
                            self.symbol_venue_map.insert(symbol, venue);
                            venue
                        }
                        _ => *self
                            .publisher_venue_map
                            .get(&msg.hd.publisher_id)
                            .expect("`Venue` not found `publisher_id`"),
                    };
                    let instrument_id = InstrumentId::new(symbol, venue);

                    match decode_instrument_def_msg_v1(rec, instrument_id, msg.ts_recv.into()) {
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
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
        include_trades: bool,
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
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let instrument_id = match &instrument_id {
                        Some(id) => *id, // Copy
                        None => decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        )
                        .expect("Failed to decode record"),
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

    pub fn load_instruments(
        &mut self,
        filepath: &Path,
        use_exchange_as_venue: bool,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.read_definition_records(filepath, use_exchange_as_venue)?
            .collect::<Result<Vec<_>, _>>()
    }

    // Cannot include trades
    pub fn load_order_book_deltas(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        self.read_records::<dbn::MboMsg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_order_book_depth10(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<OrderBookDepth10>> {
        self.read_records::<dbn::Mbp10Msg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::Mbp1Msg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_bbo_quotes(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::BboMsg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_tbbo_trades(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TbboMsg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_trades(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TradeMsg>(filepath, instrument_id, price_precision, false)?
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

    pub fn load_bars(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> anyhow::Result<Vec<Bar>> {
        self.read_records::<dbn::OhlcvMsg>(filepath, instrument_id, price_precision, false)?
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        )
                        .expect("Failed to decode record"),
                    };

                    let msg = record.get::<dbn::StatusMsg>().expect("Invalid `StatusMsg`");
                    match decode_status_msg(msg, instrument_id, msg.ts_recv.into()) {
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        )
                        .expect("Failed to decode record"),
                    };

                    let msg = record
                        .get::<dbn::ImbalanceMsg>()
                        .expect("Invalid `ImbalanceMsg`");
                    match decode_imbalance_msg(
                        msg,
                        instrument_id,
                        price_precision,
                        msg.ts_recv.into(),
                    ) {
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
                        None => decode_nautilus_instrument_id(
                            &record,
                            &mut metadata_cache,
                            &self.publisher_venue_map,
                            &self.symbol_venue_map,
                        )
                        .expect("Failed to decode record"),
                    };

                    let msg = record.get::<dbn::StatMsg>().expect("Invalid `StatMsg`");
                    match decode_statistics_msg(
                        msg,
                        instrument_id,
                        price_precision,
                        msg.ts_recv.into(),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

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
    // #[case(test_data_path().join("test_data.definition.dbn.zst"))] // TODO: Fails
    #[case(test_data_path().join("test_data.definition.v1.dbn.zst"))]
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

        assert_eq!(quotes.len(), 2);
    }

    #[rstest]
    fn test_load_tbbo_trades(loader: DatabentoDataLoader) {
        let path = test_data_path().join("test_data.tbbo.dbn.zst");
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let _trades = loader
            .load_tbbo_trades(&path, Some(instrument_id), None)
            .unwrap();

        // assert_eq!(trades.len(), 2);  TODO: No records?
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
    // #[case(test_data_path().join("test_data.ohlcv-1d.dbn.zst"))]  // TODO: Needs new data
    #[case(test_data_path().join("test_data.ohlcv-1h.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1m.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"))]
    fn test_load_bars(loader: DatabentoDataLoader, #[case] path: PathBuf) {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bars = loader.load_bars(&path, Some(instrument_id), None).unwrap();

        assert_eq!(bars.len(), 2);
    }
}
