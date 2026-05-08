// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Canonical entry hashing for the event store.
//!
//! Every captured entry carries a 32-byte BLAKE3 hash computed from a domain-separated,
//! length-prefixed serialization of `(seq, ts_init, ts_publish, topic, payload_type, payload,
//! headers)`. The hash is recomputed on every read; a mismatch quarantines the run.
//!
//! BLAKE3 is the SPEC default for the integrity-first reading: it is fast enough for the
//! capture path and outruns xxhash3 on the verifier scan over GiB-scale runs while remaining
//! a cryptographic hash for tamper detection.

use bytes::Bytes;
use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::headers::Headers;

const HASH_DOMAIN: &[u8] = b"nautilus-event-store/entry/v1";

/// The 32-byte canonical hash of an event store entry.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryHash(pub [u8; 32]);

impl EntryHash {
    /// Returns the hash as a borrowed byte slice.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the hash as a lowercase hexadecimal string.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);

        for byte in self.0 {
            out.push(nibble_to_hex(byte >> 4));
            out.push(nibble_to_hex(byte & 0x0F));
        }
        out
    }
}

const fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!(),
    }
}

/// Computes the canonical hash of an event store entry.
///
/// The hash is domain-separated by a fixed crate-internal prefix and uses big-endian
/// fixed-width framing for every variable-length field so the output depends only on the
/// logical content of the entry, not on the host endianness or the runtime serialization
/// format.
#[must_use]
pub fn compute_entry_hash(
    seq: u64,
    ts_init: UnixNanos,
    ts_publish: UnixNanos,
    topic: &str,
    payload_type: &str,
    payload: &Bytes,
    headers: &Headers,
) -> EntryHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(&seq.to_be_bytes());
    hasher.update(&ts_init.as_u64().to_be_bytes());
    hasher.update(&ts_publish.as_u64().to_be_bytes());
    write_str(&mut hasher, topic);
    write_str(&mut hasher, payload_type);
    write_bytes(&mut hasher, payload);
    write_optional_uuid(&mut hasher, headers.intent_id.as_ref());
    write_optional_uuid(&mut hasher, headers.correlation_id.as_ref());
    write_optional_uuid(&mut hasher, headers.caused_by.as_ref());
    EntryHash(*hasher.finalize().as_bytes())
}

fn write_str(hasher: &mut blake3::Hasher, value: &str) {
    let bytes = value.as_bytes();
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn write_bytes(hasher: &mut blake3::Hasher, value: &Bytes) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn write_optional_uuid(hasher: &mut blake3::Hasher, value: Option<&UUID4>) {
    match value {
        Some(uuid) => {
            hasher.update(&[1u8]);
            hasher.update(&uuid.as_bytes());
        }
        None => {
            hasher.update(&[0u8]);
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Canonical hash inputs used as the baseline across sensitivity tests.
    #[derive(Clone)]
    struct HashInput {
        seq: u64,
        ts_init: UnixNanos,
        ts_publish: UnixNanos,
        topic: String,
        payload_type: String,
        payload: Bytes,
        headers: Headers,
    }

    fn baseline() -> HashInput {
        HashInput {
            seq: 42,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000),
            ts_publish: UnixNanos::from(1_700_000_000_000_000_001),
            topic: "exec.command".to_string(),
            payload_type: "SubmitOrder".to_string(),
            payload: Bytes::from_static(b"\x01\x02\x03"),
            headers: Headers::empty(),
        }
    }

    fn hash_of(input: &HashInput) -> EntryHash {
        compute_entry_hash(
            input.seq,
            input.ts_init,
            input.ts_publish,
            &input.topic,
            &input.payload_type,
            &input.payload,
            &input.headers,
        )
    }

    #[rstest]
    fn hash_is_deterministic() {
        let input = baseline();

        assert_eq!(hash_of(&input), hash_of(&input));
    }

    #[rstest]
    #[case::seq(|i: &mut HashInput| i.seq = 99)]
    #[case::ts_init(|i: &mut HashInput| i.ts_init = UnixNanos::from(1))]
    #[case::ts_publish(|i: &mut HashInput| i.ts_publish = UnixNanos::from(1))]
    #[case::topic(|i: &mut HashInput| i.topic = "other".to_string())]
    #[case::payload_type(|i: &mut HashInput| i.payload_type = "Other".to_string())]
    #[case::payload(|i: &mut HashInput| i.payload = Bytes::from_static(b"\xFF"))]
    #[case::intent_id(|i: &mut HashInput| i.headers.intent_id = Some(UUID4::new()))]
    #[case::correlation_id(|i: &mut HashInput| i.headers.correlation_id = Some(UUID4::new()))]
    #[case::caused_by(|i: &mut HashInput| i.headers.caused_by = Some(UUID4::new()))]
    fn every_input_field_affects_hash(#[case] mutate: fn(&mut HashInput)) {
        let input = baseline();
        let mut mutated = input.clone();
        mutate(&mut mutated);

        assert_ne!(hash_of(&input), hash_of(&mutated));
    }

    #[rstest]
    fn hash_separates_topic_from_payload_type() {
        // Length-prefixed framing must prevent (topic="ab", payload_type="c") from
        // colliding with (topic="a", payload_type="bc"); without the prefix, both
        // would hash the same flattened byte stream.
        let payload = Bytes::from_static(b"x");
        let a = compute_entry_hash(
            1,
            UnixNanos::from(0),
            UnixNanos::from(0),
            "ab",
            "c",
            &payload,
            &Headers::empty(),
        );
        let b = compute_entry_hash(
            1,
            UnixNanos::from(0),
            UnixNanos::from(0),
            "a",
            "bc",
            &payload,
            &Headers::empty(),
        );

        assert_ne!(a, b);
    }

    #[rstest]
    fn compute_entry_hash_known_vector() {
        // Pinning the BLAKE3 wire format. Any change to the domain prefix, write
        // order, or endianness of the framing flips this expected value.
        let input = baseline();

        assert_eq!(
            hash_of(&input).to_hex(),
            "df93fd9a07f7b88a2ecd9b018fe52192510519a90da7b2b9b06371cc4c0da5be",
        );
    }

    #[rstest]
    fn to_hex_is_lowercase_big_endian_nibbles() {
        // Pinning EntryHash::to_hex's output: high nibble first, lowercase, fixed
        // 64 chars. Catches nibble-swap or uppercase regressions that the prior
        // length-and-charclass check let through.
        let hash = EntryHash([0xABu8; 32]);

        assert_eq!(hash.to_hex(), "ab".repeat(32));
    }
}
