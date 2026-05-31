#![no_main]

//! Fuzz `Point::decode` against arbitrary `Fp5` encodings.
//!
//! `decode` is reachable from `verify`'s public-key parse step on any
//! adversarial peer input. It MUST never panic and MUST satisfy the
//! invariant `decode(encode(p)) == p` whenever `decode` succeeds.
//!
//! The 40-byte input is reduced into a canonical `Fp5` (uniform over the
//! field) and fed to `Point::decode`. On success, the recovered point is
//! re-encoded and the round trip is asserted; the check pins the
//! `decode -> encode` cycle on any input that decodes.

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::{curve::Point, field::Fp5};

fn fp5_from_bytes(bytes: &[u8; 40]) -> Fp5 {
    let mut limbs = [0u64; 5];
    for (i, slot) in limbs.iter_mut().enumerate() {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&bytes[i * 8..(i + 1) * 8]);
        *slot = u64::from_le_bytes(chunk);
    }
    Fp5::from_u64s_reduce(limbs)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 40 {
        return;
    }
    let mut buf = [0u8; 40];
    buf.copy_from_slice(&data[..40]);

    let w = fp5_from_bytes(&buf);
    if let Some(p) = Point::decode(w) {
        let w_back = p.encode();
        let p_back = Point::decode(w_back).expect("re-encode must decode");
        assert!(
            p.eq_point(p_back),
            "decode->encode->decode round trip diverged",
        );
    }
});
