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

use std::{hint::black_box, sync::Arc};

use ahash::AHashMap;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use nautilus_bitmex::websocket::{
    enums::BitmexWsTopic,
    messages::{BitmexTableMessage, BitmexWsMessage},
    parse::{parse_trade_bin_msg_vec, parse_trade_msg_vec},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{InstrumentAny, crypto_perpetual::CryptoPerpetual},
    types::{Currency, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use serde_json::from_str;
use ustr::Ustr;

const TRADES: &str = include_str!("../test_data/ws_trade.json");
const TRADE_BIN_1M: &str = include_str!("../test_data/ws_trade_bin_1m.json");

fn instrument_cache() -> Arc<AHashMap<Ustr, InstrumentAny>> {
    let instrument = CryptoPerpetual::new(
        InstrumentId::from("XBTUSD.BITMEX"),
        Symbol::from("XBTUSD"),
        Currency::from("XBT"),
        Currency::from("USD"),
        Currency::from("XBT"),
        true,
        1,
        0,
        Price::from("0.5"),
        Quantity::from(1),
        Some(Quantity::from(1)),
        Some(Quantity::from(1)),
        None,
        Some(Quantity::from(1)),
        None,
        None,
        Some(Price::from("1000000")),
        Some(Price::from("1")),
        Some(Decimal::from_f64(0.01).expect("margin_init")),
        Some(Decimal::from_f64(0.005).expect("margin_maint")),
        Some(Decimal::from_f64(0.00025).expect("maker_fee")),
        Some(Decimal::from_f64(0.00075).expect("taker_fee")),
        UnixNanos::from(0_u64),
        UnixNanos::from(0_u64),
    );

    let mut map = AHashMap::with_capacity(1);
    map.insert(
        Ustr::from("XBTUSD"),
        InstrumentAny::CryptoPerpetual(instrument),
    );
    Arc::new(map)
}

fn bench_trades(c: &mut Criterion) {
    let instruments = instrument_cache();

    c.bench_function("bitmex_parse_trades", |b| {
        let instruments = instruments.clone();
        b.iter_batched(
            || from_str::<BitmexWsMessage>(TRADES).expect("trade message"),
            |message| match message {
                BitmexWsMessage::Table(BitmexTableMessage::Trade { data, .. }) => {
                    let payload = parse_trade_msg_vec(data, &instruments, UnixNanos::default());
                    black_box(payload);
                }
                other => panic!("unexpected message variant: {other:?}"),
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_trade_bins(c: &mut Criterion) {
    let instruments = instrument_cache();

    c.bench_function("bitmex_parse_trade_bin_1m", |b| {
        let instruments = instruments.clone();
        b.iter_batched(
            || from_str::<BitmexWsMessage>(TRADE_BIN_1M).expect("trade bin message"),
            |message| match message {
                BitmexWsMessage::Table(BitmexTableMessage::TradeBin1m { data, .. }) => {
                    let payload = parse_trade_bin_msg_vec(
                        data,
                        BitmexWsTopic::TradeBin1m,
                        &instruments,
                        UnixNanos::default(),
                    );
                    black_box(payload);
                }
                other => panic!("unexpected message variant: {other:?}"),
            },
            BatchSize::SmallInput,
        );
    });
}

fn benches(c: &mut Criterion) {
    bench_trades(c);
    bench_trade_bins(c);
}

criterion_group!(bitmex_ws_trades, benches);
criterion_main!(bitmex_ws_trades);
