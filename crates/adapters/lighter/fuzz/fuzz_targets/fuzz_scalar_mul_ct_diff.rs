#![no_main]

//! Differential fuzz: `Point::scalar_mul_ct` MUST agree with
//! `Point::scalar_mul` on every `(scalar, base)` pair.
//!
//! The constant-time path used by the Schnorr signer cannot diverge from
//! the variable-time path used by the verifier; any divergence breaks
//! sign-verify round trips end-to-end. The proptest already covers ~256
//! random pairs per run; this fuzz target lets a coverage-guided fuzzer
//! hunt for the corner inputs the random sampler misses (e.g. specific
//! recoding-window carry boundaries).
//!
//! 80-byte input: 40 bytes of scalar bytes (reduced into the curve's
//! scalar field) and 40 bytes of base-point seed (mapped through `s * G`
//! to land on a uniform group element).

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::curve::{Point, SCALAR_BYTES, Scalar};

const INPUT_LEN: usize = SCALAR_BYTES + SCALAR_BYTES;

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }
    let mut scalar_buf = [0u8; SCALAR_BYTES];
    scalar_buf.copy_from_slice(&data[..SCALAR_BYTES]);
    let mut seed_buf = [0u8; SCALAR_BYTES];
    seed_buf.copy_from_slice(&data[SCALAR_BYTES..INPUT_LEN]);

    let s = Scalar::from_le_bytes_reduce(scalar_buf);
    let seed = Scalar::from_le_bytes_reduce(seed_buf);
    let base = Point::GENERATOR.scalar_mul(seed);

    let var = base.scalar_mul(s);
    let ct = base.scalar_mul_ct(s);
    assert!(var.eq_point(ct), "scalar_mul_ct diverged from scalar_mul");
    assert_eq!(
        var.encode().to_le_bytes(),
        ct.encode().to_le_bytes(),
        "encoded outputs diverged",
    );
});
