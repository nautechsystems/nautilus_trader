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

use ahash::{AHashMap, AHashSet};
use databento::dbn::{self, PitSymbolMap, SType};
use dbn::{Publisher, Record};
use indexmap::IndexMap;
use nautilus_model::identifiers::{
    InstrumentId, Symbol, Venue,
    futures::{
        FuturesYearDigits, format_futures_symbol, parse_futures_symbol, resolve_futures_year,
        resolve_futures_year_from_expiry,
    },
};

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
/// `Some` restricts year hints to the request set; an empty set disables hints.
/// `None` uses hints from shared venue mappings.
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
    futures_year_digits: FuturesYearDigits,
    futures_symbols: &AHashMap<(PublisherId, u32, time::Date), Symbol>,
    requested_symbols: Option<&AHashSet<Symbol>>,
) -> anyhow::Result<InstrumentId> {
    let publisher = record
        .publisher()
        .map_err(|e| anyhow::anyhow!("Invalid `publisher` for record: {e}"))?;
    let publisher_id = publisher as PublisherId;
    let venue = publisher_venue_map
        .get(&publisher_id)
        .ok_or_else(|| anyhow::anyhow!("`Venue` not found for `publisher_id` {publisher_id}"))?;
    let mut instrument_id = get_nautilus_instrument_id_for_record(
        record,
        metadata,
        *venue,
        futures_year_digits,
        futures_symbols,
        symbol_venue_map,
        requested_symbols,
    )?;

    if publisher == Publisher::GlbxMdp3Glbx
        && let Some(venue) = symbol_venue_map.get(&instrument_id.symbol)
    {
        instrument_id.venue = *venue;
    }

    Ok(instrument_id)
}

/// Gets the Nautilus `InstrumentId` for a Databento record.
///
/// Cached definitions apply only to the record's publisher, numeric ID, and date.
/// Databento can reuse numeric IDs on later dates.
/// Requested symbols distinguish contracts that trade after their contract-month year.
/// `Some` restricts year hints to the request set; an empty set disables hints.
/// `None` uses hints from shared venue mappings.
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
    futures_year_digits: FuturesYearDigits,
    futures_symbols: &AHashMap<(PublisherId, u32, time::Date), Symbol>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    requested_symbols: Option<&AHashSet<Symbol>>,
) -> anyhow::Result<InstrumentId> {
    if record.get::<dbn::MboMsg>().is_none()
        && record.get::<dbn::TradeMsg>().is_none()
        && record.get::<dbn::Mbp1Msg>().is_none()
        && record.get::<dbn::Bbo1SMsg>().is_none()
        && record.get::<dbn::Bbo1MMsg>().is_none()
        && record.get::<dbn::Mbp10Msg>().is_none()
        && record.get::<dbn::OhlcvMsg>().is_none()
        && record.get::<dbn::StatusMsg>().is_none()
        && record.get::<dbn::ImbalanceMsg>().is_none()
        && record.get::<dbn::StatMsg>().is_none()
        && record.get::<dbn::InstrumentDefMsg>().is_none()
        && record.get::<dbn::Cmbp1Msg>().is_none()
        && record.get::<dbn::CbboMsg>().is_none()
    {
        anyhow::bail!("DBN message type is not currently supported");
    }

    let instrument_id = record.header().instrument_id;
    let nanoseconds = record.raw_index_ts();
    let duration = time::Duration::nanoseconds(nanoseconds as i64);
    let datetime = time::OffsetDateTime::UNIX_EPOCH
        .checked_add(duration)
        .ok_or_else(|| anyhow::anyhow!("Timestamp overflow for record"))?;
    let date = datetime.date();
    let symbol_map = metadata.symbol_map_for_date(date)?;
    let raw_symbol = symbol_map
        .get(instrument_id)
        .ok_or_else(|| anyhow::anyhow!("No raw symbol found for {instrument_id}"))?;

    if futures_year_digits == FuturesYearDigits::Two
        && record.get::<dbn::InstrumentDefMsg>().is_none()
        && let Some(symbol) =
            futures_symbols.get(&(record.header().publisher_id, instrument_id, date))
    {
        return Ok(InstrumentId::new(*symbol, venue));
    }

    let symbol = Symbol::from_str_unchecked(raw_symbol);
    let instrument_id = InstrumentId::new(symbol, venue);
    resolve_futures_instrument_id(
        record,
        instrument_id,
        futures_year_digits,
        symbol_venue_map,
        requested_symbols,
    )
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
                "Inconsistent symbology types: '{first_stype}' for {first_symbol} vs '{next_stype}' for {symbol}"
            );
        }
    }

    Ok(())
}

pub(crate) fn dataset_uses_futures_symbology(dataset: &str) -> bool {
    matches!(
        dataset,
        "GLBX.MDP3"
            | "IFEU.IMPACT"
            | "NDEX.IMPACT"
            | "IFUS.IMPACT"
            | "IFLL.IMPACT"
            | "XEUR.EOBI"
            | "XEEE.EOBI"
            | "XCBF.PITCH"
            | "XOSE.ITCH"
    )
}

pub(crate) fn raw_symbol(
    symbol: &str,
    dataset: &str,
    futures_year_digits: FuturesYearDigits,
) -> String {
    if futures_year_digits == FuturesYearDigits::Two && dataset_uses_futures_symbology(dataset) {
        rewrite_futures_symbol(symbol, 0, 1, false).unwrap_or_else(|| symbol.to_string())
    } else {
        symbol.to_string()
    }
}

fn rewrite_futures_symbol(
    symbol: &str,
    reference_year: i32,
    year_digits: u8,
    from_definition: bool,
) -> Option<String> {
    let (code, suffix) = symbol
        .split_once(' ')
        .map_or((symbol, ""), |(code, suffix)| (code, suffix));

    let legs = code
        .split('-')
        .map(|leg| {
            let parsed = parse_futures_symbol(leg)?;
            let year = if year_digits == 1 {
                parsed.year_digits.parse::<i32>().ok()?
            } else if from_definition {
                resolve_futures_year_from_expiry(parsed.year_digits, reference_year)?
            } else {
                resolve_futures_year(parsed.year_digits, reference_year)?
            };

            format_futures_symbol(parsed.root, parsed.month_code, year, year_digits)
        })
        .collect::<Option<Vec<_>>>()?;

    let code = legs.join("-");
    Some(if suffix.is_empty() {
        code
    } else {
        format!("{code} {suffix}")
    })
}

pub(crate) fn resolve_futures_instrument_id(
    record: &dbn::RecordRef,
    instrument_id: InstrumentId,
    futures_year_digits: FuturesYearDigits,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    requested_symbols: Option<&AHashSet<Symbol>>,
) -> anyhow::Result<InstrumentId> {
    if futures_year_digits != FuturesYearDigits::Two
        || !record
            .publisher()
            .is_ok_and(|publisher| dataset_uses_futures_symbology(publisher.dataset().as_str()))
    {
        return Ok(instrument_id);
    }

    let mut timestamp = record.raw_index_ts();
    let definition = record.get::<dbn::InstrumentDefMsg>();
    let maturity_year = definition
        .filter(|msg| msg.instrument_class as u8 != b'S')
        .map(|msg| msg.maturity_year)
        .filter(|year| !matches!(*year, 0 | u16::MAX))
        .map(i32::from);

    if let Some(msg) = definition {
        if !matches!(msg.instrument_class as u8 as char, 'F' | 'S' | 'C' | 'P') {
            return Ok(instrument_id);
        }

        if msg.expiration == dbn::UNDEF_TIMESTAMP && maturity_year.is_none() {
            log::warn!(
                "Keeping raw futures symbol for {instrument_id}: definition has no expiration"
            );
            return Ok(instrument_id);
        }

        timestamp = msg.expiration;
    }

    let reference_year = if let Some(year) = maturity_year {
        year
    } else {
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp))
            .map_err(|e| anyhow::anyhow!("Invalid futures reference timestamp {timestamp}: {e}"))?
            .year()
    };

    let mut symbol = rewrite_futures_symbol(
        instrument_id.symbol.as_str(),
        reference_year,
        2,
        definition.is_some(),
    );

    if definition.is_none() {
        symbol = select_requested_futures_symbol(
            symbol,
            instrument_id.symbol.as_str(),
            reference_year,
            symbol_venue_map,
            requested_symbols,
        );
    }

    Ok(symbol.map_or(instrument_id, |symbol| {
        InstrumentId::new(Symbol::from(symbol.as_str()), instrument_id.venue)
    }))
}

fn select_requested_futures_symbol(
    inferred: Option<String>,
    native: &str,
    reference_year: i32,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    requested_symbols: Option<&AHashSet<Symbol>>,
) -> Option<String> {
    let hints_empty =
        requested_symbols.map_or_else(|| symbol_venue_map.is_empty(), |symbols| symbols.is_empty());

    let contains = |symbol: &str| {
        let symbol = Symbol::from(symbol);
        requested_symbols.map_or_else(
            || symbol_venue_map.contains_key(&symbol),
            |symbols| symbols.contains(&symbol),
        )
    };

    if hints_empty || inferred.as_ref().is_none_or(|symbol| contains(symbol)) {
        return inferred;
    }

    rewrite_futures_symbol(native, reference_year - 1, 2, false)
        .filter(|symbol| contains(symbol))
        .or(inferred)
}

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

    #[rstest]
    #[case("ESZ26", "GLBX.MDP3", 2, "ESZ6")]
    #[case("ESZ95", "GLBX.MDP3", 2, "ESZ5")]
    #[case("ESZ26 P5000", "GLBX.MDP3", 2, "ESZ6 P5000")]
    #[case("ESU26-ESM27", "GLBX.MDP3", 2, "ESU6-ESM7")]
    #[case("CLZ29-CLF30", "GLBX.MDP3", 2, "CLZ9-CLF0")]
    #[case("ESZ26", "XNAS.ITCH", 2, "ESZ26")]
    #[case("ESZ26", "GLBX.MDP3", 1, "ESZ26")]
    #[case("ES.c.0", "GLBX.MDP3", 2, "ES.c.0")]
    #[case("ES.FUT", "GLBX.MDP3", 2, "ES.FUT")]
    #[case("BRN FMM0024!", "IFEU.IMPACT", 2, "BRN FMM0024!")]
    fn test_raw_symbol_preserves_venue_format(
        #[case] symbol: &str,
        #[case] dataset: &str,
        #[case] digits: u8,
        #[case] expected: &str,
    ) {
        assert_eq!(
            raw_symbol(symbol, dataset, digits.try_into().unwrap()),
            expected
        );
    }

    #[rstest]
    #[case("ESZ6", "2026-12-18T00:00:00Z", "ESZ26.XCME")]
    #[case("ESZ9", "2029-12-21T00:00:00Z", "ESZ29.XCME")]
    #[case("ESH0", "2020-03-20T00:00:00Z", "ESH20.XCME")]
    #[case("ESH0", "2030-03-15T00:00:00Z", "ESH30.XCME")]
    #[case("CLF6", "2025-12-19T00:00:00Z", "CLF26.XCME")]
    #[case("CLF0", "2029-12-19T00:00:00Z", "CLF30.XCME")]
    #[case("SR3Z6", "2027-03-16T00:00:00Z", "SR3Z26.XCME")]
    #[case("SR3Z9", "2030-03-19T00:00:00Z", "SR3Z29.XCME")]
    #[case("SR3Z6-SR3H7", "2027-03-16T00:00:00Z", "SR3Z26-SR3H27.XCME")]
    #[case("SR3Z6-SR3Z3", "2027-03-16T00:00:00Z", "SR3Z26-SR3Z33.XCME")]
    #[case("CLZ9-CLF0", "2029-11-20T00:00:00Z", "CLZ29-CLF30.XCME")]
    fn test_futures_definition_uses_contract_year(
        #[case] symbol: &str,
        #[case] expiry: &str,
        #[case] expected: InstrumentId,
    ) {
        let expiration = expiry.parse::<jiff::Timestamp>().unwrap().as_nanosecond() as u64;

        let msg = dbn::InstrumentDefMsg {
            hd: dbn::RecordHeader::new::<dbn::InstrumentDefMsg>(
                dbn::rtype::INSTRUMENT_DEF,
                1,
                42,
                0,
            ),
            instrument_class: if symbol.contains('-') { b'S' } else { b'F' } as std::ffi::c_char,
            expiration,
            ..Default::default()
        };

        let id = InstrumentId::new(Symbol::from(symbol), Venue::from("XCME"));
        let record = dbn::RecordRef::from(&msg);
        assert_eq!(
            resolve_futures_instrument_id(
                &record,
                id,
                FuturesYearDigits::Two,
                &AHashMap::new(),
                None
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            resolve_futures_instrument_id(
                &record,
                id,
                FuturesYearDigits::One,
                &AHashMap::new(),
                None
            )
            .unwrap(),
            id
        );
    }

    #[rstest]
    #[case(dbn::UNDEF_TIMESTAMP)]
    #[case(1_000_000_000)]
    fn test_bbo_year_uses_receive_timestamp(#[case] ts_event: u64) {
        let ts_recv = "2029-12-20T00:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_nanosecond() as u64;
        let mut msg = dbn::BboMsg::default_for_schema(dbn::Schema::Bbo1S);
        msg.hd.publisher_id = 1;
        msg.hd.instrument_id = 42;
        msg.hd.ts_event = ts_event;
        msg.ts_recv = ts_recv;
        let id = InstrumentId::from("CLF0.XNYM");
        assert_eq!(
            resolve_futures_instrument_id(
                &dbn::RecordRef::from(&msg),
                id,
                FuturesYearDigits::Two,
                &AHashMap::new(),
                None
            )
            .unwrap(),
            InstrumentId::from("CLF30.XNYM")
        );
    }

    #[rstest]
    #[case(2, "SR3Z6", "SR3Z26", "SR3Z26.XCME")]
    #[case(2, "SR3Z6", "SR3Z36", "SR3Z36.XCME")]
    #[case(2, "SR3Z6", "CLF26", "SR3Z36.XCME")]
    #[case(2, "SR3Z6-SR3H7", "SR3Z26-SR3H27", "SR3Z26-SR3H27.XCME")]
    #[case(1, "SR3Z6", "SR3Z26", "SR3Z6.XCME")]
    fn test_market_record_uses_requested_contract_year(
        #[case] digits: u8,
        #[case] native: &str,
        #[case] requested: &str,
        #[case] expected: InstrumentId,
    ) {
        let mut msg = dbn::BboMsg::default_for_schema(dbn::Schema::Bbo1S);
        msg.hd.publisher_id = 1;
        msg.ts_recv = "2027-02-01T00:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_nanosecond() as u64;
        let id = InstrumentId::new(Symbol::from(native), Venue::from("XCME"));
        let symbols = AHashMap::from_iter([(Symbol::from(requested), Venue::from("XCME"))]);

        let result = resolve_futures_instrument_id(
            &dbn::RecordRef::from(&msg),
            id,
            digits.try_into().unwrap(),
            &symbols,
            None,
        )
        .unwrap();

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(2026, "CLF26.XCME")]
    #[case(0, "CLF6.XCME")]
    #[case(u16::MAX, "CLF6.XCME")]
    fn test_definition_maturity_year_resolves_without_expiration(
        #[case] maturity_year: u16,
        #[case] expected: InstrumentId,
    ) {
        let msg = dbn::InstrumentDefMsg {
            hd: dbn::RecordHeader::new::<dbn::InstrumentDefMsg>(
                dbn::rtype::INSTRUMENT_DEF,
                1,
                42,
                0,
            ),
            instrument_class: b'F' as std::ffi::c_char,
            maturity_year,
            expiration: dbn::UNDEF_TIMESTAMP,
            ..Default::default()
        };

        let id = InstrumentId::from("CLF6.XCME");
        let result = resolve_futures_instrument_id(
            &dbn::RecordRef::from(&msg),
            id,
            FuturesYearDigits::Two,
            &AHashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(result, expected);
        assert_eq!(
            resolve_futures_instrument_id(
                &dbn::RecordRef::from(&msg),
                id,
                FuturesYearDigits::One,
                &AHashMap::new(),
                None
            )
            .unwrap(),
            id
        );
    }
}
