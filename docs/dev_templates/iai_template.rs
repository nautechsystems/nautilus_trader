//! iai benchmark template.
//!
//! Copy this file into `crates/<my_crate>/benches/` and adjust names and
//! imports.

use std::hint::black_box;

// -----------------------------------------------------------------------------
// Replace `fast_add` with the real function you want to measure.
// -----------------------------------------------------------------------------

fn fast_add() -> i32 {
    let a = black_box(1);
    let b = black_box(2);
    a + b
}

iai::main!(fast_add);
