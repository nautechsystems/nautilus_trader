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

#![no_main]

//! Untrusted REST decoding, identity round trips, exact raw values, and bounded OHLCV volume.

#[path = "../../src/data/parse.rs"]
mod parse;

use nautilus_core::UnixNanos;
use nautilus_finfeed::{common, http};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    data::BarType, enums::AggressorSide, instruments::BinaryOption,
    orderbook::analysis::book_check_integrity,
};
use rust_decimal::{Decimal, RoundingStrategy};

fuzz_target!(|data: &[u8]| {
    let market =
        serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
    let instrument = http::parse::parse_instrument(&market, UnixNanos::default()).unwrap();

    if let Ok(source) = serde_json::from_slice::<http::models::ActivityBatch>(data) {
        for record in source.quotes.iter().flatten() {
            quote(record, &instrument);
        }

        for record in source.trades.iter().flatten() {
            trade(record, &instrument);
        }
    }

    if let Ok(records) = serde_json::from_slice::<Vec<http::models::Quote>>(data) {
        for record in records {
            quote(&record, &instrument);
        }
    }

    if let Ok(records) = serde_json::from_slice::<Vec<http::models::Trade>>(data) {
        for record in records {
            trade(&record, &instrument);
        }
    }

    if let Ok(source) = serde_json::from_slice::<http::models::Book>(data) {
        book(&source, &instrument);
    }

    if let Ok(records) = serde_json::from_slice::<Vec<http::models::Ohlcv>>(data) {
        for record in records {
            bar(&record, &instrument);
        }
    }

    if let Ok(source) = serde_json::from_slice::<http::models::Market>(data) {
        definition(&source);
    }

    if let Ok(source) = serde_json::from_slice::<http::models::MarketsPage>(data) {
        for record in source.data.iter().flatten() {
            definition(record);
        }
    }
});

fn quote(source: &http::models::Quote, instrument: &BinaryOption) {
    let Ok(tick) = parse::quote(source, instrument) else {
        return;
    };

    assert_eq!(tick.instrument_id, instrument.id);
    assert_eq!(tick.bid_price.as_decimal(), source.bid);
    assert_eq!(tick.ask_price.as_decimal(), source.ask);
    assert_eq!(tick.bid_size.as_decimal(), source.bid_volume);
    assert_eq!(tick.ask_size.as_decimal(), source.ask_volume);
    assert_eq!(
        i128::from(tick.ts_event.as_u64()),
        source.entry_time.as_nanosecond()
    );
    assert_eq!(
        i128::from(tick.ts_init.as_u64()),
        source.recv_time.as_nanosecond()
    );
    assert!((Decimal::ZERO..=Decimal::ONE).contains(&source.bid));
    assert!((Decimal::ZERO..=Decimal::ONE).contains(&source.ask));
}

fn trade(source: &http::models::Trade, instrument: &BinaryOption) {
    let Ok(tick) = parse::trade(source, instrument) else {
        return;
    };

    assert_eq!(tick.instrument_id, instrument.id);
    assert_eq!(tick.aggressor_side, AggressorSide::NoAggressor);
    assert_eq!(Some(tick.trade_id.as_str()), source.id.as_deref());
    assert_eq!(
        Some(tick.instrument_id.symbol.as_str()),
        source.market_id.as_deref()
    );
    assert_eq!(tick.price.as_decimal(), source.price);
    assert_eq!(tick.size.as_decimal(), source.quantity);
    assert!(source.quantity > Decimal::ZERO);
    assert!((Decimal::ZERO..=Decimal::ONE).contains(&source.price));
    assert_eq!(
        i128::from(tick.ts_event.as_u64()),
        source.timestamp.as_nanosecond()
    );
    assert_eq!(tick.ts_init, tick.ts_event);
}

fn book(source: &http::models::Book, instrument: &BinaryOption) {
    let Ok(book) = parse::book(source, instrument, None) else {
        return;
    };

    book_check_integrity(&book).unwrap();
    assert_eq!(book.instrument_id, instrument.id);
    let bids = source.bids.as_ref().unwrap();
    let asks = source.asks.as_ref().unwrap();
    assert_eq!(book.bids_as_map(None).len(), bids.len());
    assert_eq!(book.asks_as_map(None).len(), asks.len());

    for (levels, actual) in [
        (bids, book.bids_as_map(None)),
        (asks, book.asks_as_map(None)),
    ] {
        for level in levels {
            assert_eq!(actual[&level.price], level.size);
            assert!(level.size > Decimal::ZERO);
        }
    }

    for depth in [1, 10] {
        let limited = parse::book(source, instrument, Some(depth)).unwrap();
        assert_eq!(limited.bids_as_map(None), book.bids_as_map(Some(depth)));
        assert_eq!(limited.asks_as_map(None), book.asks_as_map(Some(depth)));
    }
}

fn bar(source: &http::models::Ohlcv, instrument: &BinaryOption) {
    let period = http::models::OhlcvPeriod {
        period_id: Some("1MIN".into()),
        length_seconds: 60,
        length_months: 0,
        unit_count: Some(1),
        unit_name: Some("minute".into()),
        display_name: None,
    };

    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id));

    let Ok(Some(bar)) = parse::bar(source, bar_type, instrument, &period) else {
        return;
    };

    assert_eq!(bar.bar_type, bar_type);
    assert_eq!(Some(bar.open.as_decimal()), source.price_open);
    assert_eq!(Some(bar.high.as_decimal()), source.price_high);
    assert_eq!(Some(bar.low.as_decimal()), source.price_low);
    assert_eq!(Some(bar.close.as_decimal()), source.price_close);
    let rounded = source.volume_traded.round_dp_with_strategy(
        u32::from(instrument.size_precision),
        RoundingStrategy::MidpointNearestEven,
    );
    assert_eq!(bar.volume.as_decimal(), rounded);
    assert_eq!(bar.volume.precision, instrument.size_precision);

    if rounded != source.volume_traded {
        assert!(source.volume_traded > Decimal::ZERO);
        assert!(source.volume_traded < Decimal::new(500_000_000, 0));
        assert!(
            (source.volume_traded - rounded).abs() * Decimal::new(1_000_000_000_000_000, 0)
                <= source.volume_traded
        );
    }

    assert!(bar.low <= bar.open && bar.open <= bar.high);
    assert!(bar.low <= bar.close && bar.close <= bar.high);
    assert_eq!(
        i128::from(bar.ts_event.as_u64()),
        source.time_period_end.as_nanosecond()
    );
    assert_eq!(bar.ts_init, bar.ts_event);
}

fn definition(source: &http::models::Market) {
    let Ok(instrument) = http::parse::parse_instrument(source, UnixNanos::default()) else {
        return;
    };

    let (exchange, market) = common::symbol::from_instrument_id(instrument.id).unwrap();
    assert_eq!(Some(exchange.as_str()), source.exchange_id.as_deref());
    assert_eq!(Some(market.as_str()), source.market_id.as_deref());
    assert_eq!(
        common::symbol::to_instrument_id(&exchange, &market).unwrap(),
        instrument.id
    );
    assert_eq!(instrument.raw_symbol, instrument.id.symbol);
    assert!(instrument.expiration_ns > instrument.activation_ns);
    assert_eq!(instrument.price_precision, 4);
    assert_eq!(instrument.size_precision, 6);
}
