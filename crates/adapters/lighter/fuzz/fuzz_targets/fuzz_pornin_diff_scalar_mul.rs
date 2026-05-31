#![no_main]

//! Differential soak: `Point::scalar_mul` on an arbitrary base must match
//! Pornin's upstream Rust reference (`pornin/ecgfp5`, MIT-licensed) on
//! every `(scalar, seed)` pair.
//!
//! Pornin's crate is the upstream reference implementation that accompanies
//! the curve's design paper and shares no code lineage with our in-tree
//! port. Coverage-guided fuzz against it is the strongest single
//! correctness gate available offline: any divergence in the windowed
//! signed-digit recoding, the lookup, the addition formulas, or the
//! Frobenius-based field operations surfaces immediately as a byte
//! mismatch. Pair this target with `fuzz_pornin_diff_decode` for the
//! public-input curve-decode surface.
//!
//! Input layout (80 bytes):
//!
//! - `[0..40]`: scalar `s` bytes, fed to both implementations through
//!   their reducing decoders.
//! - `[40..80]`: seed bytes, used to derive the base point as `seed * G`
//!   in both implementations.

use ecgfp5::{curve::Point as PorninPoint, scalar::Scalar as PorninScalar};
use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::curve::{Point, SCALAR_BYTES, Scalar};

const INPUT_LEN: usize = 2 * SCALAR_BYTES;

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }
    let mut s_buf = [0u8; SCALAR_BYTES];
    s_buf.copy_from_slice(&data[..SCALAR_BYTES]);
    let mut seed_buf = [0u8; SCALAR_BYTES];
    seed_buf.copy_from_slice(&data[SCALAR_BYTES..INPUT_LEN]);

    let ours_seed = Scalar::from_le_bytes_reduce(seed_buf);
    let ours_s = Scalar::from_le_bytes_reduce(s_buf);
    let theirs_seed = PorninScalar::decode_reduce(&seed_buf);
    let theirs_s = PorninScalar::decode_reduce(&s_buf);

    let ours_base = Point::GENERATOR * ours_seed;
    let theirs_base = PorninPoint::mulgen(theirs_seed);

    let ours = (ours_base * ours_s).encode().to_le_bytes();
    let theirs = (theirs_base * theirs_s).encode().encode();

    assert_eq!(ours, theirs, "scalar_mul diverged from Pornin reference");
});
