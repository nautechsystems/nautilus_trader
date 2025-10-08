// -------------------------------------------------------------------------------------------------
//  Integration tests for MoneyFlowIndex correctness.
// -------------------------------------------------------------------------------------------------

use nautilus_indicators::average::mfi::MoneyFlowIndex;

fn approx_eq(a: f64, b: f64, tol: f64) {
    assert!(
        (a - b).abs() <= tol,
        "expected {b:.12}, got {a:.12}, tol={tol}"
    );
}

#[test]
fn mfi_neutral_on_first_sample() {
    let mut mfi = MoneyFlowIndex::new(10);
    // First update has no prior delta -> neutral 0.5
    mfi.update_raw(10.0, 100.0);
    approx_eq(mfi.value, 0.5, 1e-12);
}

#[test]
fn mfi_directionality_up_then_down() {
    // period=3 with unit volumes; typical prices: 1 -> 2 -> 3 -> 2
    // flows: [0, +2, +3, -2] (sign determined by price delta; value = tp*vol)
    let mut mfi = MoneyFlowIndex::new(3);
    mfi.update_raw(1.0, 1.0); // seed, neutral
    approx_eq(mfi.value, 0.5, 1e-12);

    mfi.update_raw(2.0, 1.0); // up: pos=2
    approx_eq(mfi.value, 1.0, 1e-12);

    mfi.update_raw(3.0, 1.0); // up: pos=2+3
    approx_eq(mfi.value, 1.0, 1e-12);

    mfi.update_raw(2.0, 1.0); // down: neg=2 (window keeps last 3 flow samples)
    // pos_sum=2+3=5, neg_sum=2 => mfi = 1 - 1/(1 + 5/2) = 1 - 1/3.5 ≈ 0.7142857142857
    approx_eq(mfi.value, 0.714_285_714_285_714_3, 1e-12);
}

#[test]
fn mfi_all_down_moves_heads_to_zero() {
    let mut mfi = MoneyFlowIndex::new(3);
    mfi.update_raw(10.0, 10.0); // seed
    mfi.update_raw(9.5, 10.0); // neg
    mfi.update_raw(9.0, 10.0); // neg
    // With positive flow == 0 and negative flow > 0 → mfi = 0.0
    approx_eq(mfi.value, 0.0, 1e-12);
}

#[test]
fn mfi_window_eviction_behaviour() {
    // Ensure we evict old flows when exceeding the period
    let mut mfi = MoneyFlowIndex::new(2);
    mfi.update_raw(100.0, 1.0); // seed
    mfi.update_raw(101.0, 1.0); // pos=101
    approx_eq(mfi.value, 1.0, 1e-12);

    mfi.update_raw(102.0, 1.0); // pos=102 (window keeps last 2 flow samples → still saturated)
    approx_eq(mfi.value, 1.0, 1e-12);

    mfi.update_raw(101.0, 1.0); // neg=101 (window now contains [pos=102, neg=101])
    let pos_sum = 102.0f64;
    let neg_sum = 101.0f64;
    let expected = 1.0 - (1.0 / (1.0 + pos_sum / neg_sum));
    approx_eq(mfi.value, expected, 1e-12);
}


