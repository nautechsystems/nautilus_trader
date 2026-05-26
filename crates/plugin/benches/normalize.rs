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

#![allow(unsafe_code)]

use std::{hint::black_box, ptr};

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{OrderBookDelta, OrderBookDeltas, QuoteTick, order::BookOrder},
    enums::{BookAction, OrderSide},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny, stubs as instrument_stubs},
    orderbook::OrderBook,
    stubs as model_stubs,
    types::{Price, Quantity},
};
use nautilus_plugin::{
    BorrowedStr, HostContext, HostVTable, Slice,
    surfaces::{
        actor::{PluginActor, actor_vtable},
        book::{OrderBookDeltasHandle, OrderBookHandle},
        instrument::InstrumentAnyHandle,
    },
};

struct BoundaryBenchActor;

impl PluginActor for BoundaryBenchActor {
    const TYPE_NAME: &'static str = "BoundaryBenchActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
        Self
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        black_box(instrument.id());
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        black_box(deltas.instrument_id);
        black_box(deltas.deltas.len());
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        black_box(book.instrument_id);
        black_box(book.update_count);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        black_box(quote.instrument_id);
        Ok(())
    }

    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        black_box(quotes.len());
        Ok(())
    }
}

fn quote_tick() -> QuoteTick {
    QuoteTick::new(
        InstrumentId::from("ETH-USDT.BINANCE"),
        Price::from("100.00"),
        Price::from("100.50"),
        Quantity::from("1.0"),
        Quantity::from("2.0"),
        UnixNanos::from(1u64),
        UnixNanos::from(1u64),
    )
}

fn quote_ticks(count: usize) -> Vec<QuoteTick> {
    (0..count).map(|_| quote_tick()).collect()
}

fn order_book_deltas(count: usize) -> OrderBookDeltas {
    let instrument_id = InstrumentId::from("ETH-USDT.BINANCE");
    let deltas = (0..count)
        .map(|i| {
            let side = if i % 2 == 0 {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    side,
                    Price::new(100.0 + (i as f64 * 0.01), 2),
                    Quantity::new(1.0 + (i as f64 * 0.001), 3),
                    i as u64 + 1,
                ),
                0,
                i as u64,
                UnixNanos::from(i as u64),
                UnixNanos::from(i as u64),
            )
        })
        .collect();
    OrderBookDeltas::new(instrument_id, deltas)
}

fn large_instrument() -> InstrumentAny {
    InstrumentAny::Betting(instrument_stubs::betting())
}

fn order_book_snapshot() -> OrderBook {
    model_stubs::stub_order_book_mbp_appl_xnas()
}

fn bench_actor_boundary_normalization(c: &mut Criterion) {
    // SAFETY: actor_vtable returns a static vtable for this concrete actor type.
    let vtable = unsafe { &*actor_vtable::<BoundaryBenchActor>() };
    let create = vtable.create.expect("actor vtable has create");
    let drop_handle = vtable.drop_handle.expect("actor vtable has drop_handle");
    // SAFETY: the bench actor ignores host and context pointers in `new`.
    let handle = unsafe { create(ptr::null(), ptr::null(), BorrowedStr::empty()) };
    assert!(!handle.is_null(), "actor vtable create returned null");

    let on_instrument = vtable
        .on_instrument
        .expect("actor vtable has on_instrument");
    let instrument = InstrumentAnyHandle::new(large_instrument());
    c.bench_function("plugin_normalize/large_instrument_any", |b| {
        b.iter(|| {
            // SAFETY: handle comes from this vtable and instrument lives for the call.
            unsafe { on_instrument(handle, black_box(&instrument)) }
                .into_result()
                .expect("instrument callback succeeds");
        });
    });

    let on_book_deltas = vtable
        .on_book_deltas
        .expect("actor vtable has on_book_deltas");
    let deltas = OrderBookDeltasHandle::new(order_book_deltas(100));
    c.bench_function("plugin_normalize/order_book_deltas_100", |b| {
        b.iter(|| {
            // SAFETY: handle comes from this vtable and deltas lives for the call.
            unsafe { on_book_deltas(handle, black_box(&deltas)) }
                .into_result()
                .expect("book deltas callback succeeds");
        });
    });

    let on_book = vtable.on_book.expect("actor vtable has on_book");
    let book = order_book_snapshot();
    c.bench_function("plugin_cross/order_book_snapshot_10x10", |b| {
        b.iter(|| {
            let book_handle = OrderBookHandle::new(black_box(book.clone()));
            // SAFETY: handle comes from this vtable and book_handle lives for the call.
            unsafe { on_book(handle, black_box(&book_handle)) }
                .into_result()
                .expect("order book callback succeeds");
        });
    });

    let on_quote = vtable.on_quote.expect("actor vtable has on_quote");
    let quote = quote_tick();
    c.bench_function("plugin_normalize/quote_tick", |b| {
        b.iter(|| {
            // SAFETY: handle comes from this vtable and quote lives for the call.
            unsafe { on_quote(handle, black_box(&quote)) }
                .into_result()
                .expect("quote callback succeeds");
        });
    });

    let on_historical_quotes = vtable
        .on_historical_quotes
        .expect("actor vtable has on_historical_quotes");
    let quotes = quote_ticks(128);
    c.bench_function("plugin_normalize/historical_quotes_128", |b| {
        b.iter(|| {
            // SAFETY: handle comes from this vtable and quotes lives for the call.
            unsafe { on_historical_quotes(handle, black_box(Slice::from_slice(&quotes))) }
                .into_result()
                .expect("historical quotes callback succeeds");
        });
    });

    // SAFETY: handle was allocated by this vtable's create slot.
    unsafe { drop_handle(handle) };
}

criterion_group!(benches, bench_actor_boundary_normalization);
criterion_main!(benches);
