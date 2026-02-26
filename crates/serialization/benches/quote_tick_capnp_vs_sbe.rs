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

//! Direct `QuoteTick` decode comparison: Cap'n Proto vs SBE.
//!
//! This benchmark compares end-to-end decode into `QuoteTick` from:
//! - Cap'n Proto bytes using existing `FromCapnp` conversions.
//! - A compact SBE wire layout decoded with `SbeCursor`.

use std::hint::black_box;

use capnp::{
    message::{Builder, ReaderOptions},
    serialize,
};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_model::{
    data::QuoteTick,
    identifiers::{InstrumentId, Symbol, Venue},
    types::{Price, Quantity},
};
use nautilus_serialization::{
    capnp::{FromCapnp, ToCapnp, market_capnp},
    sbe::{SbeCursor, SbeDecodeError},
};

const QUOTE_TICK_TEMPLATE_ID: u16 = 30_001;
const QUOTE_TICK_SCHEMA_ID: u16 = 1;
const QUOTE_TICK_SCHEMA_VERSION: u16 = 0;
const QUOTE_TICK_BLOCK_LENGTH: u16 = 50;

fn create_quote_tick() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::new(Symbol::new("AAPL"), Venue::new("XNAS")),
        bid_price: Price::from("100.50"),
        ask_price: Price::from("100.55"),
        bid_size: Quantity::from("100"),
        ask_size: Quantity::from("100"),
        ts_event: 1_609_459_200_000_000_000.into(),
        ts_init: 1_609_459_200_000_000_000.into(),
    }
}

fn encode_quote_tick_capnp(quote: &QuoteTick) -> Vec<u8> {
    let mut message = Builder::new_default();
    let builder = message.init_root::<market_capnp::quote_tick::Builder>();
    quote.to_capnp(builder);

    let mut bytes = Vec::new();
    serialize::write_message(&mut bytes, &message).unwrap();
    bytes
}

fn encode_quote_tick_sbe(quote: &QuoteTick) -> Vec<u8> {
    let symbol = quote.instrument_id.symbol.as_str().as_bytes();
    let venue = quote.instrument_id.venue.as_str().as_bytes();
    let symbol_len = u8::try_from(symbol.len()).expect("symbol must fit in varString8");
    let venue_len = u8::try_from(venue.len()).expect("venue must fit in varString8");

    let mut buf = Vec::with_capacity(
        8 + usize::from(QUOTE_TICK_BLOCK_LENGTH)
            + usize::from(symbol_len)
            + usize::from(venue_len)
            + 2,
    );

    // Header: blockLength, templateId, schemaId, version
    buf.extend_from_slice(&QUOTE_TICK_BLOCK_LENGTH.to_le_bytes());
    buf.extend_from_slice(&QUOTE_TICK_TEMPLATE_ID.to_le_bytes());
    buf.extend_from_slice(&QUOTE_TICK_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&QUOTE_TICK_SCHEMA_VERSION.to_le_bytes());

    // Body
    buf.push(quote.bid_price.precision);
    buf.push(quote.bid_size.precision);

    let bid_price_raw: i64 = quote
        .bid_price
        .raw
        .try_into()
        .expect("benchmark bid price raw must fit i64");
    let ask_price_raw: i64 = quote
        .ask_price
        .raw
        .try_into()
        .expect("benchmark ask price raw must fit i64");
    let bid_size_raw: u64 = quote
        .bid_size
        .raw
        .try_into()
        .expect("benchmark bid size raw must fit u64");
    let ask_size_raw: u64 = quote
        .ask_size
        .raw
        .try_into()
        .expect("benchmark ask size raw must fit u64");

    buf.extend_from_slice(&bid_price_raw.to_le_bytes());
    buf.extend_from_slice(&ask_price_raw.to_le_bytes());
    buf.extend_from_slice(&bid_size_raw.to_le_bytes());
    buf.extend_from_slice(&ask_size_raw.to_le_bytes());
    buf.extend_from_slice(&(*quote.ts_event).to_le_bytes());
    buf.extend_from_slice(&(*quote.ts_init).to_le_bytes());

    // symbol, venue as varString8
    buf.push(symbol_len);
    buf.extend_from_slice(symbol);
    buf.push(venue_len);
    buf.extend_from_slice(venue);

    buf
}

fn decode_quote_tick_capnp(bytes: &[u8]) -> QuoteTick {
    let reader = serialize::read_message(&mut &bytes[..], ReaderOptions::new()).unwrap();
    let root = reader
        .get_root::<market_capnp::quote_tick::Reader>()
        .unwrap();
    QuoteTick::from_capnp(root).unwrap()
}

fn decode_quote_tick_sbe(bytes: &[u8]) -> Result<QuoteTick, SbeDecodeError> {
    let mut cursor = SbeCursor::new(bytes);

    let block_length = cursor.read_u16_le()?;
    let template_id = cursor.read_u16_le()?;
    let schema_id = cursor.read_u16_le()?;
    let version = cursor.read_u16_le()?;

    if block_length != QUOTE_TICK_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: QUOTE_TICK_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    if template_id != QUOTE_TICK_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(template_id));
    }

    if schema_id != QUOTE_TICK_SCHEMA_ID {
        return Err(SbeDecodeError::SchemaMismatch {
            expected: QUOTE_TICK_SCHEMA_ID,
            actual: schema_id,
        });
    }

    if version != QUOTE_TICK_SCHEMA_VERSION {
        return Err(SbeDecodeError::VersionMismatch {
            expected: QUOTE_TICK_SCHEMA_VERSION,
            actual: version,
        });
    }

    let price_precision = cursor.read_u8()?;
    let qty_precision = cursor.read_u8()?;

    let bid_price_raw = cursor.read_i64_le()?;
    let ask_price_raw = cursor.read_i64_le()?;
    let bid_size_raw = cursor.read_u64_le()?;
    let ask_size_raw = cursor.read_u64_le()?;
    let ts_event = cursor.read_u64_le()?;
    let ts_init = cursor.read_u64_le()?;

    let symbol = Symbol::new(cursor.read_var_string8_ref()?);
    let venue = Venue::new(cursor.read_var_string8_ref()?);

    Ok(QuoteTick {
        instrument_id: InstrumentId::new(symbol, venue),
        bid_price: Price::from_raw(bid_price_raw as _, price_precision),
        ask_price: Price::from_raw(ask_price_raw as _, price_precision),
        bid_size: Quantity::from_raw(bid_size_raw as _, qty_precision),
        ask_size: Quantity::from_raw(ask_size_raw as _, qty_precision),
        ts_event: ts_event.into(),
        ts_init: ts_init.into(),
    })
}

fn bench_quote_tick_decode_direct_compare(c: &mut Criterion) {
    let quote = create_quote_tick();
    let capnp_bytes = encode_quote_tick_capnp(&quote);
    let sbe_bytes = encode_quote_tick_sbe(&quote);

    // Sanity-check both decode paths produce the same value.
    assert_eq!(decode_quote_tick_capnp(&capnp_bytes), quote);
    assert_eq!(decode_quote_tick_sbe(&sbe_bytes).unwrap(), quote);

    let mut group = c.benchmark_group("QuoteTick::decode_direct_compare");
    group.bench_function("capnp", |b| {
        b.iter(|| black_box(decode_quote_tick_capnp(black_box(&capnp_bytes))));
    });
    group.bench_function("sbe", |b| {
        b.iter(|| black_box(decode_quote_tick_sbe(black_box(&sbe_bytes)).unwrap()));
    });
    group.finish();
}

criterion_group!(
    quote_tick_decode_compare,
    bench_quote_tick_decode_direct_compare
);
criterion_main!(quote_tick_decode_compare);
