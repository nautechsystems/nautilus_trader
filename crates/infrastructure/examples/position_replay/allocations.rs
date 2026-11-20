// -------------------------------------------------------------------------------------------------
// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// https://nautechsystems.io
// Licensed under the GNU Lesser General Public License Version 3.0.
// https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Measures whole-engine synchronous fill allocations separately from normal timings.
//!
//! Requested bytes and a live-byte high-water mark are not allocator capacity or process RSS.
//! Only the memory backend is supported to exclude asynchronous Redis worker allocations.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::events::OrderEventAny;

mod comparison;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;
struct CountedAllocator;

fn allocated(size: usize) {
    TOTAL.fetch_add(size, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: System receives unchanged pointers and layouts; counters observe successful allocations.
unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }

        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            allocated(size);
        }

        ptr
    }
}

fn main() -> std::process::ExitCode {
    assert_eq!(std::env::args().nth(2).as_deref(), Some("memory"));
    comparison::run(process, "allocated_bytes")
}

fn process(engine: &mut ExecutionEngine, event: &OrderEventAny) -> u128 {
    let live = LIVE.load(Ordering::Relaxed);
    let total = TOTAL.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    engine.process(std::hint::black_box(event));
    let requested = TOTAL.load(Ordering::Relaxed) - total;
    let peak_extra = PEAK.load(Ordering::Relaxed).saturating_sub(live);
    let retained = LIVE.load(Ordering::Relaxed) as i128 - live as i128;
    eprintln!(
        "ALLOC requested_bytes={requested} peak_extra_live_bytes={peak_extra} retained_delta_bytes={retained}"
    );
    requested as u128
}
