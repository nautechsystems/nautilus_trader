use iai::black_box;
use nautilus_model::types::fixed::f64_to_fixed_i64;

fn iai_fixed_precision_benchmark() -> i64 {
    f64_to_fixed_i64(black_box(-0.000_000_001), black_box(9))
}

iai::main!(iai_fixed_precision_benchmark);
