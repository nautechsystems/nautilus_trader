#![no_main]

//! Fuzz the Poseidon2 sponge across arbitrary input lengths and squeeze
//! sizes.
//!
//! The first byte of the input selects `num_outputs in 0..=255`; the
//! remainder is interpreted as a little-endian sequence of 8-byte chunks
//! reduced into Goldilocks. Inputs whose remainder is not a multiple of 8
//! are truncated.
//!
//! Asserts that:
//!
//! - The sponge does not panic on any input length / squeeze count.
//! - Output length matches the requested `num_outputs`.
//! - Two invocations on the same input produce the same digest
//!   (determinism).

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::{field::Fp, hash::hash_n_to_m_no_pad};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let num_outputs = data[0] as usize;
    let body = &data[1..];

    let chunk_count = body.len() / 8;
    let mut input = Vec::with_capacity(chunk_count);
    for i in 0..chunk_count {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&body[i * 8..(i + 1) * 8]);
        input.push(Fp::from_u64_reduce(u64::from_le_bytes(chunk)));
    }

    let out1 = hash_n_to_m_no_pad(&input, num_outputs);
    assert_eq!(out1.len(), num_outputs, "sponge output length mismatch");

    let out2 = hash_n_to_m_no_pad(&input, num_outputs);
    assert_eq!(out1, out2, "sponge non-deterministic on identical input");
});
