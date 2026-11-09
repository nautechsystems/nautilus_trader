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

//! Allocation probe for Databento symbol spelling.
//!
//! Historical requests and DBN loading call `nautilus_symbol` once per record. With share-class
//! spacing left on, a symbol that needs no conversion must not allocate.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use nautilus_databento::symbology::nautilus_symbol;
use rstest::rstest;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `layout` is a valid allocation request from the caller
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `layout` is a valid allocation request from the caller
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `ptr` came from this allocator and `layout` describes that block
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator and `layout` describes that block
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn allocations_while_repeating(raw: &str) -> usize {
    // Intern the symbol first. A repeated record must not allocate again.
    black_box(nautilus_symbol(black_box(raw), true));

    ALLOCATIONS.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    let symbol = black_box(nautilus_symbol(black_box(raw), true));
    COUNTING.store(false, Ordering::SeqCst);

    assert_eq!(symbol.as_str(), raw);
    ALLOCATIONS.load(Ordering::SeqCst)
}

#[rstest]
fn test_unchanged_symbol_does_not_allocate() {
    for raw in [
        "AAPL",
        "ESM4",
        "123456",
        "ES.FUT",
        "SPX.OPT",
        "ES.c.0",
        "ES.n.0",
        "SPY   240319P00511000",
    ] {
        let allocations = allocations_while_repeating(raw);
        assert_eq!(allocations, 0, "{raw} allocated {allocations} times");
    }
}
