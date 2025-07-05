//! Criterion benchmark template.
//!
//! Copy this file into `crates/<my_crate>/benches/` and adjust the names and
//! imports.  Compile with
//!
//! ```bash
//! cargo bench -p <my_crate> --bench <file_stem>
//! ```

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

// -----------------------------------------------------------------------------
// Replace `my_function` and set-up code with your real workload.
// -----------------------------------------------------------------------------

fn my_function(input: &[u8]) -> usize {
    input.iter().map(|b| *b as usize).sum()
}

fn prepare_data() -> Vec<u8> {
    (0u8..=255).collect()
}

fn bench_my_function(c: &mut Criterion) {
    let data = prepare_data();

    c.bench_function("my_function", |b| {
        b.iter(|| {
            let _ = black_box(my_function(&data));
        });
    });
}

criterion_group!(benches, bench_my_function);
criterion_main!(benches);
