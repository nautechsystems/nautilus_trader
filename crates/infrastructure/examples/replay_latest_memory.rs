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

//! Measures requested live heap allocations, not process RSS or peak memory.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::OrderFilled,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{CurrencyPair, InstrumentAny},
    position::{Position, PositionReplayEvent},
    types::{Currency, Price, Quantity},
};
const POSITION_ID: &str = "AUDUSD-NETTING";

fn same_cycle(n: usize) -> Position {
    let mut p = Position::new(&instrument(), fill(0, OrderSide::Buy, Quantity::from(1)));
    for i in 1..n {
        p.apply(&fill(i, OrderSide::Buy, Quantity::from(1)));
    }
    assert_eq!(p.events.len(), n);
    assert_eq!(p.replay_events.len(), n);
    assert!(!p.id.is_virtual());
    p
}
fn carried(n: usize) -> Position {
    let mut prior = same_cycle(n);
    prior.apply(&fill(n, OrderSide::Sell, Quantity::from(n as u64)));
    assert!(prior.is_closed());
    let mut p = Position::new(
        &instrument(),
        fill(n + 1, OrderSide::Buy, Quantity::from(1)),
    );
    p.transfer_replay_state_from(&mut prior);
    assert_eq!(p.events.len(), 1);
    assert_eq!(p.replay_events.len(), n + 2);
    assert!(prior.replay_events.is_empty());
    p
}
fn instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("AUD/USD.SIM"))
            .raw_symbol(Symbol::from("AUD/USD"))
            .base_currency(Currency::AUD())
            .quote_currency(Currency::USD())
            .price_precision(5)
            .size_precision(0)
            .price_increment(Price::from("0.00001"))
            .size_increment(Quantity::from(1))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn fill(index: usize, side: OrderSide, quantity: Quantity) -> OrderFilled {
    OrderFilled::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("AUD/USD.SIM"),
        ClientOrderId::from(format!("O-{index}").as_str()),
        VenueOrderId::from(format!("V-{index}").as_str()),
        AccountId::from("SIM-001"),
        TradeId::from(format!("T-{index}").as_str()),
        side,
        OrderType::Market,
        quantity,
        Price::from("1.00000"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::from(index as u64),
        UnixNanos::from(index as u64),
        false,
        Some(PositionId::from(POSITION_ID)),
        None,
        None,
    )
}

struct CountedAllocator;
static BYTES: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;
// SAFETY: All allocation operations delegate to System with unchanged pointers and layouts,
// and the atomic counter only observes requested sizes; it does not alter ownership.
unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            BYTES.fetch_add(size, Ordering::Relaxed);
            BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        new
    }
}

fn main() {
    let missing = TradeId::from("MISSING");
    let _ = missing;
    println!("inline_position_bytes={}", std::mem::size_of::<Position>());
    println!(
        "history,copy_owned_heap_bytes,first_query_alloc_bytes,second_copy_owned_heap_bytes,snapshot_alloc_bytes"
    );

    for n in [8, 64, 4096, 36000] {
        let p = carried(n);
        assert!(!p.has_replay_trade_id(missing));
        let start = BYTES.load(Ordering::Relaxed);
        let mut q = p.clone();
        let cloned = BYTES.load(Ordering::Relaxed);
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::to_value(&q).unwrap()
        );
        let before = BYTES.load(Ordering::Relaxed);
        assert!(!q.has_replay_trade_id(missing));
        let queried = BYTES.load(Ordering::Relaxed);
        let second_start = BYTES.load(Ordering::Relaxed);
        let r = q.clone();
        let second_end = BYTES.load(Ordering::Relaxed);
        let snap_start = BYTES.load(Ordering::Relaxed);
        let snap = q.clone_for_snapshot();
        let snap_end = BYTES.load(Ordering::Relaxed);
        println!(
            "{n},{},{},{},{}",
            cloned - start,
            queried - before,
            second_end - second_start,
            snap_end - snap_start
        );

        if let PositionReplayEvent::Filled(f) = &mut q.replay_events[0] {
            f.trade_id = TradeId::from("EDITED-CLONE");
        }
        assert!(!q.has_replay_trade_id(TradeId::from("T-0")));
        assert!(q.has_replay_trade_id(TradeId::from("EDITED-CLONE")));
        assert!(p.has_replay_trade_id(TradeId::from("T-0")));
        assert!(r.has_replay_trade_id(TradeId::from("T-0")));
        let encoded = serde_json::to_vec(&r).unwrap();
        let loaded: Position = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(&r).unwrap()
        );
        std::hint::black_box(snap);
    }
    eprintln!(
        "PASS actual clone helper: serialized equivalence, independent equal-length mutation, original and second clone intact, snapshot, JSON reload"
    );
}
