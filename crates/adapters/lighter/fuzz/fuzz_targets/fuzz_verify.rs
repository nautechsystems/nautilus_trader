#![no_main]

//! Fuzz the Schnorr verify path against arbitrary `(pk, msg, sig)` bytes.
//!
//! Verification MUST never panic on adversarial input from a peer or the
//! venue. The target slices a 200-byte input into:
//!
//! - 40 bytes for the public-key `Fp5` (limbs reduced into canonical range,
//!   then wrapped via `PublicKey::from_fp5` which deliberately skips the
//!   curve-membership check; `verify` is responsible for rejecting).
//! - 40 bytes for the hashed-message `Fp5`.
//! - 80 bytes for the wire signature, decoded through the reducing decoder.
//! - 40 padding bytes ignored.
//!
//! Inputs shorter than 200 bytes are skipped. The fuzz harness asserts the
//! verify call returns a `bool` (no panic, no UB) — any divergence is a bug.

use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::{
    field::Fp5,
    schnorr::{PublicKey, SIG_BYTES, Signature},
};

const PK_BYTES: usize = 40;
const MSG_BYTES: usize = 40;
const INPUT_LEN: usize = PK_BYTES + MSG_BYTES + SIG_BYTES;

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
    if data.len() < INPUT_LEN {
        return;
    }

    let mut pk_buf = [0u8; PK_BYTES];
    pk_buf.copy_from_slice(&data[..PK_BYTES]);
    let mut msg_buf = [0u8; MSG_BYTES];
    msg_buf.copy_from_slice(&data[PK_BYTES..PK_BYTES + MSG_BYTES]);
    let mut sig_buf = [0u8; SIG_BYTES];
    sig_buf.copy_from_slice(&data[PK_BYTES + MSG_BYTES..INPUT_LEN]);

    let pk = PublicKey::from_fp5(fp5_from_bytes(&pk_buf));
    let msg = fp5_from_bytes(&msg_buf);
    let sig = Signature::from_le_bytes_reduce(sig_buf);

    let _ = pk.verify(msg, &sig);
});
