#![no_main]

//! Fuzz the Schnorr signature byte codec.
//!
//! `Signature::from_le_bytes_reduce` accepts any 80 bytes and reduces each
//! 40-byte half into the canonical scalar field. The fuzz target asserts:
//!
//! - The decoded signature is canonical (`is_canonical` is the post-condition
//!   of `from_le_bytes_reduce`).
//! - Re-encoding via `to_le_bytes` produces canonical limbs whose decode is
//!   idempotent.
//!
//! Catches any decoder regression that leaves the result non-canonical or
//! breaks the encode-decode round trip.

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::schnorr::{SIG_BYTES, Signature};

fuzz_target!(|data: &[u8]| {
    if data.len() < SIG_BYTES {
        return;
    }
    let mut buf = [0u8; SIG_BYTES];
    buf.copy_from_slice(&data[..SIG_BYTES]);

    let sig = Signature::from_le_bytes_reduce(buf);
    assert!(
        sig.is_canonical(),
        "from_le_bytes_reduce must always yield canonical scalars",
    );

    let bytes = sig.to_le_bytes();
    let sig2 = Signature::from_le_bytes_reduce(bytes);
    assert_eq!(
        sig, sig2,
        "encode-decode round trip must be idempotent on canonical sigs",
    );
});
