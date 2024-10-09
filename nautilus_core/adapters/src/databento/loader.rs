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

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use databento::dbn;
use dbn::{
    compat::InstrumentDefMsgV1,
    decode::{dbn::Decoder, DbnMetadata, DecodeStream},
};
use fallible_streaming_iterator::FallibleStreamingIterator;
use indexmap::IndexMap;
use nautilus_model::{
    data::{
        bar::Bar, delta::OrderBookDelta, depth::OrderBookDepth10, quote::QuoteTick,
        status::InstrumentStatus, trade::TradeTick, Data,
    },
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::any::InstrumentAny,
    types::currency::Currency,
};
use ustr::Ustr;

use super::{
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg_v1, decode_record, decode_statistics_msg,
        decode_status_msg, raw_ptr_to_ustr,
    },
    symbology::decode_nautilus_instrument_id,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, Dataset, PublisherId},
};

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
pub struct DatabentoDataLoader {
    publishers_map: IndexMap<PublisherId, DatabentoPublisher>,
    venue_dataset_map: IndexMap<Venue, Dataset>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
}

impl DatabentoDataLoader {
    /// Creates a new [`DatabentoDataLoader`] instance.
    pub fn new(publishers_filepath: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut loader = Self {
            publishers_map: IndexMap::new(),
            venue_dataset_map: IndexMap::new(),
            publisher_venue_map: IndexMap::new(),
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
            .load_publishers(publishers_filepath.clone())
            .unwrap_or_else(|_| {
                panic!(
                    "No such file or directory '{}'",
                    publishers_filepath.display()
                )
            });

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
    pub const fn get_publishers(&self) -> &IndexMap<u16, DatabentoPublisher> {
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

    pub fn schema_from_file(&self, filepath: &Path) -> anyhow::Result<Option<String>> {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata();
        Ok(metadata.schema.map(|schema| schema.to_string()))
    }

    pub fn read_definition_records(
        &mut self,
        filepath: &Path,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<InstrumentAny>> + '_> {
        let mut decoder = Decoder::from_zstd_file(filepath)?;
        decoder.set_upgrade_policy(dbn::VersionUpgradePolicy::Upgrade);
        let mut dbn_stream = decoder.decode_stream::<InstrumentDefMsgV1>();

        Ok(std::iter::from_fn(move || {
            if let Err(e) = dbn_stream.advance() {
                return Some(Err(e.into()));
            }
            match dbn_stream.get() {
                Some(rec) => {
                    let record = dbn::RecordRef::from(rec);
                    let msg = record.get::<InstrumentDefMsgV1>().unwrap();

                    let raw_symbol = unsafe {
                        raw_ptr_to_ustr(rec.raw_symbol.as_ptr())
                            .expect("Error obtaining `raw_symbol` pointer")
                    };
                    let symbol = Symbol::from(raw_symbol);

                    let venue = self
                        .publisher_venue_map
                        .get(&msg.hd.publisher_id)
                        .expect("`Venue` not found `publisher_id`");
                    let instrument_id = InstrumentId::new(symbol, *venue);

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
        include_trades: bool,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<(Option<Data>, Option<Data>)>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = Currency::USD().precision; // Hard coded for now

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
                            &metadata,
                            &self.publisher_venue_map,
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

    pub fn load_instruments(&mut self, filepath: &Path) -> anyhow::Result<Vec<InstrumentAny>> {
        self.read_definition_records(filepath)?
            .collect::<Result<Vec<_>, _>>()
    }

    // Cannot include trades
    pub fn load_order_book_deltas(
        &self,
        filepath: &Path,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        self.read_records::<dbn::MboMsg>(filepath, instrument_id, false)?
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
    ) -> anyhow::Result<Vec<OrderBookDepth10>> {
        self.read_records::<dbn::Mbp10Msg>(filepath, instrument_id, false)?
            .filter_map(|result| match result {
                Ok((Some(item1), _)) => {
                    if let Data::Depth10(depth) = item1 {
                        Some(Ok(depth))
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
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::Mbp1Msg>(filepath, instrument_id, false)?
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
    ) -> anyhow::Result<Vec<QuoteTick>> {
        self.read_records::<dbn::BboMsg>(filepath, instrument_id, false)?
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
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TbboMsg>(filepath, instrument_id, false)?
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
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.read_records::<dbn::TradeMsg>(filepath, instrument_id, false)?
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
    ) -> anyhow::Result<Vec<Bar>> {
        self.read_records::<dbn::OhlcvMsg>(filepath, instrument_id, false)?
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
                            &metadata,
                            &self.publisher_venue_map,
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
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoImbalance>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = Currency::USD().precision; // Hard coded for now

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
                            &metadata,
                            &self.publisher_venue_map,
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
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<DatabentoStatistics>> + '_>
    where
        T: dbn::Record + dbn::HasRType + 'static,
    {
        let decoder = Decoder::from_zstd_file(filepath)?;
        let metadata = decoder.metadata().clone();
        let mut dbn_stream = decoder.decode_stream::<T>();

        let price_precision = Currency::USD().precision; // Hard coded for now

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
                            &metadata,
                            &self.publisher_venue_map,
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

    use rstest::*;

    use super::*;

    fn test_data_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/databento/test_data")
    }

    fn data_loader() -> DatabentoDataLoader {
        let publishers_filepath = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("databento")
            .join("publishers.json");

        DatabentoDataLoader::new(Some(publishers_filepath)).unwrap()
    }

    // TODO: Improve the below assertions that we've actually read the records we expected

    #[rstest]
    // #[case(test_data_path().join("test_data.definition.dbn.zst"))] // TODO: Fails
    #[case(test_data_path().join("test_data.definition.v1.dbn.zst"))]
    fn test_load_instruments(#[case] path: PathBuf) {
        let mut loader = data_loader();
        let instruments = loader.load_instruments(&path).unwrap();

        assert_eq!(instruments.len(), 2);
    }

    #[rstest]
    fn test_load_order_book_deltas() {
        let path = test_data_path().join("test_data.mbo.dbn.zst");
        let loader = data_loader();
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let deltas = loader
            .load_order_book_deltas(&path, Some(instrument_id))
            .unwrap();

        assert_eq!(deltas.len(), 2);
    }

    #[rstest]
    fn test_load_order_book_depth10() {
        let path = test_data_path().join("test_data.mbp-10.dbn.zst");
        let loader = data_loader();
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let depths = loader
            .load_order_book_depth10(&path, Some(instrument_id))
            .unwrap();

        assert_eq!(depths.len(), 2);
    }

    #[rstest]
    fn test_load_quotes() {
        let path = test_data_path().join("test_data.mbp-1.dbn.zst");
        let loader = data_loader();
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader.load_quotes(&path, Some(instrument_id)).unwrap();

        assert_eq!(quotes.len(), 2);
    }

    #[rstest]
    #[case(test_data_path().join("test_data.bbo-1s.dbn.zst"))]
    #[case(test_data_path().join("test_data.bbo-1m.dbn.zst"))]
    fn test_load_bbo_quotes(#[case] path: PathBuf) {
        let loader = data_loader();
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let quotes = loader.load_bbo_quotes(&path, Some(instrument_id)).unwrap();

        assert_eq!(quotes.len(), 2);
    }

    #[rstest]
    fn test_load_tbbo_trades() {
        let path = test_data_path().join("test_data.tbbo.dbn.zst");
        let loader = data_loader();
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let _trades = loader.load_tbbo_trades(&path, Some(instrument_id)).unwrap();

        // assert_eq!(trades.len(), 2);  TODO: No records?
    }

    #[rstest]
    fn test_load_trades() {
        let path = test_data_path().join("test_data.trades.dbn.zst");
        let loader = data_loader();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let trades = loader.load_trades(&path, Some(instrument_id)).unwrap();

        assert_eq!(trades.len(), 2);
    }

    #[rstest]
    // #[case(test_data_path().join("test_data.ohlcv-1d.dbn.zst"))]  // TODO: Needs new data
    #[case(test_data_path().join("test_data.ohlcv-1h.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1m.dbn.zst"))]
    #[case(test_data_path().join("test_data.ohlcv-1s.dbn.zst"))]
    fn test_load_bars(#[case] path: PathBuf) {
        let loader = data_loader();

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let bars = loader.load_bars(&path, Some(instrument_id)).unwrap();

        assert_eq!(bars.len(), 2);
    }
}
