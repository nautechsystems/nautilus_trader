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

//! Allocation counts for the canonical Kalshi pipeline calls.
//!
//! Counts are requested allocations while the probe is active. Freed bytes are not subtracted, and
//! the counts are not resident memory.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use common::{
    BookReady, DispatchReady, credential, decode_quote, decode_trade, limit_order, live_instrument,
    quote_frame, sign_submit, trade_frame,
};

const WARMUP: usize = 100;
const SAMPLES: usize = 1_000;

struct CountingAlloc {
    active: AtomicBool,
    count: AtomicUsize,
    bytes: AtomicUsize,
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        self.record(ptr, layout.size());
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        self.record(ptr, layout.size());
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, new_size) };
        self.record(ptr, new_size);
        ptr
    }
}

impl CountingAlloc {
    fn record(&self, ptr: *mut u8, size: usize) {
        if !ptr.is_null() && self.active.load(Ordering::Relaxed) {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.bytes.fetch_add(size, Ordering::Relaxed);
        }
    }

    fn measure(&self, mut call: impl FnMut()) -> (usize, usize) {
        for _ in 0..WARMUP {
            call();
        }

        self.count.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);

        for _ in 0..SAMPLES {
            call();
        }

        self.active.store(false, Ordering::Relaxed);

        (
            self.count.load(Ordering::Relaxed) / SAMPLES,
            self.bytes.load(Ordering::Relaxed) / SAMPLES,
        )
    }

    fn measure_prepared<T>(
        &self,
        mut prepared: Vec<T>,
        mut call: impl FnMut(&mut T),
    ) -> (usize, usize) {
        self.count.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);

        for item in &mut prepared {
            call(item);
        }

        self.active.store(false, Ordering::Relaxed);
        drop(prepared);

        (
            self.count.load(Ordering::Relaxed) / SAMPLES,
            self.bytes.load(Ordering::Relaxed) / SAMPLES,
        )
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc {
    active: AtomicBool::new(false),
    count: AtomicUsize::new(0),
    bytes: AtomicUsize::new(0),
};

fn report(name: &str, count: usize, bytes: usize) {
    println!("alloc {name} count={count} bytes={bytes}");
}

fn main() {
    let instrument = live_instrument();
    let quote = quote_frame();
    let trade = trade_frame();

    let (count, bytes) = ALLOC.measure(|| {
        black_box(decode_quote(&quote, &instrument));
    });

    report("quotes", count, bytes);

    let (count, bytes) = ALLOC.measure(|| {
        black_box(decode_trade(&trade, &instrument));
    });

    report("trades", count, bytes);

    let prepared = (0..SAMPLES)
        .map(|_| {
            let ready = BookReady::new();
            let frame = ready.snapshot_frame().to_vec();
            (ready, frame)
        })
        .collect();

    let (count, bytes) = ALLOC.measure_prepared(prepared, |item| {
        let (ready, frame) = item;
        ready.apply_frame(frame);
        black_box(ready);
    });

    report("book_snapshot", count, bytes);

    let prepared = (0..SAMPLES)
        .map(|_| {
            let ready = BookReady::new();
            let snapshot = ready.snapshot_frame().to_vec();
            ready.apply_frame(&snapshot);
            let frame = ready.delta_frame().to_vec();
            (ready, frame)
        })
        .collect();

    let (count, bytes) = ALLOC.measure_prepared(prepared, |item| {
        let (ready, frame) = item;
        ready.apply_frame(frame);
        black_box(ready);
    });

    report("book_deltas", count, bytes);

    let order = limit_order(&instrument);
    let rsa = credential(true);
    let ed25519 = credential(false);

    let (count, bytes) = ALLOC.measure(|| {
        black_box(sign_submit(&rsa, &order));
    });

    report("submit_rsa_pss", count, bytes);

    let (count, bytes) = ALLOC.measure(|| {
        black_box(sign_submit(&ed25519, &order));
    });

    report("submit_ed25519", count, bytes);

    let prepared = (0..SAMPLES).map(|_| DispatchReady::tracked()).collect();

    let (count, bytes) = ALLOC.measure_prepared(prepared, |ready| {
        ready.dispatch();
        black_box(ready);
    });

    report("fill_tracked", count, bytes);
    let prepared = (0..SAMPLES).map(|_| DispatchReady::external()).collect();

    let (count, bytes) = ALLOC.measure_prepared(prepared, |ready| {
        ready.dispatch();
        black_box(ready);
    });

    report("fill_external", count, bytes);
}
