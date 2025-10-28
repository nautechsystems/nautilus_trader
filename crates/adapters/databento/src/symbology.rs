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

use ahash::AHashMap;
use databento::dbn::{self, PitSymbolMap, SType};
use dbn::{Publisher, Record};
use indexmap::IndexMap;
use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};

use super::types::PublisherId;

#[derive(Debug)]
pub struct MetadataCache {
    metadata: dbn::Metadata,
    date_metadata_map: AHashMap<time::Date, PitSymbolMap>,
}

impl MetadataCache {
    #[must_use]
    pub fn new(metadata: dbn::Metadata) -> Self {
        Self {
            metadata,
            date_metadata_map: AHashMap::new(),
        }
    }

    /// # Errors
    ///
    /// Returns an error if metadata for the given `date` cannot be retrieved.
    pub fn symbol_map_for_date(&mut self, date: time::Date) -> dbn::Result<&PitSymbolMap> {
        if !self.date_metadata_map.contains_key(&date) {
            let map = self.metadata.symbol_map_for_date(date)?;
            self.date_metadata_map.insert(date, map);
        }

        self.date_metadata_map
            .get(&date)
            .ok_or_else(|| dbn::Error::decode(format!("metadata cache missing for date {date}")))
    }
}

pub fn instrument_id_to_symbol_string(
    instrument_id: InstrumentId,
    symbol_venue_map: &mut AHashMap<Symbol, Venue>,
) -> String {
    symbol_venue_map
        .entry(instrument_id.symbol)
        .or_insert(instrument_id.venue);
    instrument_id.symbol.to_string()
}

/// Decodes a Databento record into a Nautilus `InstrumentId`.
///
/// # Errors
///
/// Returns an error if:
/// - The publisher cannot be extracted from the record.
/// - The publisher ID is not found in the venue map.
/// - The underlying instrument ID mapping fails.
pub fn decode_nautilus_instrument_id(
    record: &dbn::RecordRef,
    metadata: &mut MetadataCache,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
) -> anyhow::Result<InstrumentId> {
    let publisher = record
        .publisher()
        .map_err(|e| anyhow::anyhow!("Invalid `publisher` for record: {e}"))?;
    let publisher_id = publisher as PublisherId;
    let venue = publisher_venue_map
        .get(&publisher_id)
        .ok_or_else(|| anyhow::anyhow!("`Venue` not found for `publisher_id` {publisher_id}"))?;
    let mut instrument_id = get_nautilus_instrument_id_for_record(record, metadata, *venue)?;
    if publisher == Publisher::GlbxMdp3Glbx
        && let Some(venue) = symbol_venue_map.get(&instrument_id.symbol)
    {
        instrument_id.venue = *venue;
    }

    Ok(instrument_id)
}

/// Gets the Nautilus `InstrumentId` for a Databento record.
///
/// # Errors
///
/// Returns an error if:
/// - The record type is not supported.
/// - Timestamp overflow occurs when calculating the date.
/// - Symbol metadata lookup fails.
/// - No raw symbol is found for the instrument ID.
pub fn get_nautilus_instrument_id_for_record(
    record: &dbn::RecordRef,
    metadata: &mut MetadataCache,
    venue: Venue,
) -> anyhow::Result<InstrumentId> {
    let (instrument_id, nanoseconds) = if let Some(msg) = record.get::<dbn::MboMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::TradeMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::Mbp1Msg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::Bbo1SMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::Bbo1MMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::OhlcvMsg>() {
        (msg.hd.instrument_id, msg.hd.ts_event)
    } else if let Some(msg) = record.get::<dbn::StatusMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::ImbalanceMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::StatMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::InstrumentDefMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::Cmbp1Msg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::CbboMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else if let Some(msg) = record.get::<dbn::TbboMsg>() {
        (msg.hd.instrument_id, msg.ts_recv)
    } else {
        anyhow::bail!("DBN message type is not currently supported")
    };

    let duration = time::Duration::nanoseconds(nanoseconds as i64);
    let datetime = time::OffsetDateTime::UNIX_EPOCH
        .checked_add(duration)
        .ok_or_else(|| anyhow::anyhow!("Timestamp overflow for record"))?;
    let date = datetime.date();
    let symbol_map = metadata.symbol_map_for_date(date)?;
    let raw_symbol = symbol_map
        .get(instrument_id)
        .ok_or_else(|| anyhow::anyhow!("No raw symbol found for {instrument_id}"))?;

    let symbol = Symbol::from_str_unchecked(raw_symbol);

    Ok(InstrumentId::new(symbol, venue))
}

#[must_use]
pub fn infer_symbology_type(symbol: &str) -> SType {
    if symbol.ends_with(".FUT") || symbol.ends_with(".OPT") {
        return SType::Parent;
    }

    let parts: Vec<&str> = symbol.split('.').collect();
    if parts.len() == 3 && parts[2].chars().all(|c| c.is_ascii_digit()) {
        return SType::Continuous;
    }

    if symbol.chars().all(|c| c.is_ascii_digit()) {
        return SType::InstrumentId;
    }

    SType::RawSymbol
}

/// # Errors
///
/// Returns an error if `symbols` is empty or symbols have inconsistent symbology types.
pub fn check_consistent_symbology(symbols: &[&str]) -> anyhow::Result<()> {
    if symbols.is_empty() {
        anyhow::bail!("No symbols provided");
    }
    let first_symbol = symbols[0];
    let first_stype = infer_symbology_type(first_symbol);

    for symbol in symbols {
        let next_stype = infer_symbology_type(symbol);
        if next_stype != first_stype {
            anyhow::bail!(
                "Inconsistent symbology types: '{}' for {} vs '{}' for {}",
                first_stype,
                first_symbol,
                next_stype,
                symbol
            );
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    #[case("1", "instrument_id")]
    #[case("123456789", "instrument_id")]
    #[case("AAPL", "raw_symbol")]
    #[case("ESM4", "raw_symbol")]
    #[case("BRN FMM0024!", "raw_symbol")]
    #[case("BRN  99   5617289", "raw_symbol")]
    #[case("SPY   240319P00511000", "raw_symbol")]
    #[case("ES.FUT", "parent")]
    #[case("ES.OPT", "parent")]
    #[case("BRN.FUT", "parent")]
    #[case("SPX.OPT", "parent")]
    #[case("ES.c.0", "continuous")]
    #[case("SPX.n.0", "continuous")]
    fn test_infer_symbology_type(#[case] symbol: String, #[case] expected: SType) {
        let result = infer_symbology_type(&symbol);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_check_consistent_symbology_when_empty_symbols() {
        let symbols: Vec<&str> = vec![];
        assert!(check_consistent_symbology(&symbols).is_err());
    }

    #[rstest]
    fn test_instrument_id_to_symbol_string_updates_map() {
        let symbol = Symbol::from("TEST");
        let venue = Venue::from("XNAS");
        let instrument_id = InstrumentId::new(symbol, venue);
        let mut map: AHashMap<Symbol, Venue> = AHashMap::new();

        // First call should insert the mapping
        let sym_str = instrument_id_to_symbol_string(instrument_id, &mut map);
        assert_eq!(sym_str, "TEST");
        assert_eq!(map.get(&Symbol::from("TEST")), Some(&Venue::from("XNAS")));

        // Call again with same symbol but different venue should not override existing
        let other = Venue::from("XLON");
        let inst2 = InstrumentId::new(Symbol::from("TEST"), other);
        let sym_str2 = instrument_id_to_symbol_string(inst2, &mut map);
        assert_eq!(sym_str2, "TEST");

        // Venue remains the original
        assert_eq!(map.get(&Symbol::from("TEST")), Some(&Venue::from("XNAS")));
    }

    #[rstest]
    fn test_check_consistent_symbology_when_inconsistent() {
        let symbols = vec!["ESM4", "ES.OPT"];
        let result = check_consistent_symbology(&symbols);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Inconsistent symbology types: 'raw_symbol' for ESM4 vs 'parent' for ES.OPT"
        );
    }

    #[rstest]
    #[case(vec!["AAPL,MSFT"])]
    #[case(vec!["ES.OPT,ES.FUT"])]
    #[case(vec!["ES.c.0,ES.c.1"])]
    fn test_check_consistent_symbology_when_consistent(#[case] symbols: Vec<&str>) {
        let result = check_consistent_symbology(&symbols);
        assert!(result.is_ok());
    }
}
