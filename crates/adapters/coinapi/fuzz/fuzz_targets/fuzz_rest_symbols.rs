#![no_main]

//! Fuzz REST symbol list decode and instrument parsing.
//!
//! Every symbol of a supported type yields exactly one instrument or error. Parsed instruments have
//! unique IDs, round-trip their native ID to the CoinAPI exchange symbol, and carry increments and
//! multipliers equal to the listed values.

use std::str::FromStr;

use ahash::AHashSet;
use nautilus_coinapi::{
    common::symbol::{CoinApiSymbolType, VenueSymbol},
    http::{models::Symbol, parse::parse_instruments},
};
use nautilus_core::UnixNanos;
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::instruments::{Instrument, InstrumentAny};

fuzz_target!(|data: &[u8]| {
    let Ok(symbols) = serde_json::from_slice::<Vec<Symbol>>(data) else {
        return;
    };

    let ts_init = UnixNanos::from(1);
    let (instruments, errors) = parse_instruments(&symbols, ts_init);

    let supported = symbols
        .iter()
        .filter(|symbol| CoinApiSymbolType::from_str(&symbol.symbol_type).is_ok())
        .count();
    assert_eq!(instruments.len() + errors.len(), supported);

    let mut instrument_ids = AHashSet::new();

    for parsed in &instruments {
        let instrument = &parsed.instrument;
        assert!(
            instrument_ids.insert(instrument.id()),
            "repeated instrument ID"
        );
        assert_eq!(instrument.ts_event(), ts_init);
        assert_eq!(instrument.ts_init(), ts_init);

        let matches = symbols
            .iter()
            .filter(|symbol| symbol.symbol_id == parsed.symbol_id)
            .any(|symbol| describes(symbol, instrument));
        assert!(matches, "no listed symbol describes {}", parsed.symbol_id);
    }
});

fn describes(symbol: &Symbol, instrument: &InstrumentAny) -> bool {
    let Ok(symbol_type) = CoinApiSymbolType::from_str(&symbol.symbol_type) else {
        return false;
    };

    let Some(symbol_id_exchange) = symbol.symbol_id_exchange else {
        return false;
    };

    let venue_symbol = VenueSymbol {
        exchange_id: symbol.exchange_id,
        symbol_type,
        symbol: symbol_id_exchange,
    };

    let multiplier = match symbol_type {
        CoinApiSymbolType::Spot => true,
        CoinApiSymbolType::Perpetual | CoinApiSymbolType::Futures => {
            symbol.future_contract_unit == Some(instrument.multiplier().as_decimal())
        }
    };

    VenueSymbol::from_native(instrument.id()) == Ok(venue_symbol)
        && instrument.raw_symbol().inner() == symbol_id_exchange
        && symbol.price_precision == Some(instrument.price_increment().as_decimal())
        && symbol.size_precision == Some(instrument.size_increment().as_decimal())
        && multiplier
}
