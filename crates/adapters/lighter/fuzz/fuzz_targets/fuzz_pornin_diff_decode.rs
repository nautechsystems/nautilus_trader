#![no_main]

//! Differential soak: `Point::decode` must agree with Pornin's reference
//! on every `Fp5` candidate, both on success/failure and on the recovered
//! point.
//!
//! `decode` is reachable from `verify`'s public-key parse step on any
//! adversarial peer input, so its panic-freedom and decision boundary are
//! both load-bearing. Pornin's upstream Rust reference returns a `(point,
//! mask)` pair with `mask = u64::MAX` on success and `0` on rejection;
//! ours returns `Option<Point>`. The target asserts both implementations
//! land on the same decision and on byte-identical encodings when both
//! accept.
//!
//! Input layout (40 bytes): little-endian limbs of an `Fp5` candidate,
//! reduced into the canonical range before being fed to either decoder.

use ecgfp5::{curve::Point as PorninPoint, field::GFp5 as PorninFp5};
use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::{curve::Point, field::Fp5};

const INPUT_LEN: usize = 40;

fn fp5_from_bytes(bytes: &[u8; INPUT_LEN]) -> Fp5 {
    let mut limbs = [0u64; 5];
    for (i, slot) in limbs.iter_mut().enumerate() {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&bytes[i * 8..(i + 1) * 8]);
        *slot = u64::from_le_bytes(chunk);
    }
    Fp5::from_u64s_reduce(limbs)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }
    let mut buf = [0u8; INPUT_LEN];
    buf.copy_from_slice(&data[..INPUT_LEN]);

    let ours_w = fp5_from_bytes(&buf);
    let canonical_bytes = ours_w.to_le_bytes();
    let (theirs_w, theirs_decode_ok) = PorninFp5::decode(&canonical_bytes);
    assert_eq!(
        theirs_decode_ok,
        u64::MAX,
        "our canonical Fp5 bytes must decode under Pornin's reference",
    );

    let ours_pt = Point::decode(ours_w);
    let (theirs_pt, theirs_ok) = PorninPoint::decode(theirs_w);

    match (ours_pt, theirs_ok) {
        (Some(p), u64::MAX) => {
            assert_eq!(
                p.encode().to_le_bytes(),
                theirs_pt.encode().encode(),
                "Point::decode value diverged from Pornin reference",
            );
        }
        (None, 0) => {} // Both reject non-residues.
        (ours_state, theirs_mask) => {
            panic!(
                "Point::decode decision diverged: ours={} theirs_ok={:#x}",
                ours_state.is_some(),
                theirs_mask,
            );
        }
    }
});
