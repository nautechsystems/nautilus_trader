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
use iai::black_box;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_okx::websocket::{
    messages::{OKXOrderMsg, OKXWebSocketEvent},
    parse::parse_order_msg,
};
use serde_json::{from_str, from_value};
use ustr::Ustr;

const ORDERS: &str = include_str!("../test_data/ws_orders.json");

fn build_context() -> (Vec<OKXOrderMsg>, AHashMap<Ustr, InstrumentAny>, AccountId) {
    let event: OKXWebSocketEvent = from_str(ORDERS).expect("orders event");
    let data = match event {
        OKXWebSocketEvent::Data { data, .. } => data,
        _ => unreachable!(),
    };
    let orders: Vec<OKXOrderMsg> = from_value(data).expect("orders payload");

    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
    let instrument = CryptoPerpetual::new(
        instrument_id,
        Symbol::from("BTC-USDT-SWAP"),
        Currency::BTC(),
        Currency::USDT(),
        Currency::USDT(),
        false,
        2,
        8,
        Price::from("0.01"),
        Quantity::from("0.00000001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let mut instruments = AHashMap::new();
    instruments.insert(
        Ustr::from("BTC-USDT-SWAP"),
        InstrumentAny::CryptoPerpetual(instrument),
    );

    (orders, instruments, AccountId::new("OKX-001"))
}

fn bench_parse_order_msg() {
    let (orders, instruments, account_id) = build_context();
    let fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
    let filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
    let ts_init = UnixNanos::default();

    for msg in &orders {
        let report = parse_order_msg(
            msg,
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            ts_init,
        )
        .expect("order report");
        black_box(report);
    }
}

iai::main!(bench_parse_order_msg);
