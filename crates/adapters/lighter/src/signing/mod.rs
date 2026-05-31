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

//! Lighter L2 transaction signing.
//!
//! Lighter signs transactions with **Schnorr signatures over the ecgfp5 curve**
//! (Pornin, 2022), defined over the quintic extension field of the Goldilocks
//! prime `p = 2^64 - 2^32 + 1`, with **Poseidon2** as the binding hash.
//!
//! This module is an original in-tree Rust implementation written from public
//! specifications and permissively licensed references. For the field and
//! curve, Thomas Pornin's MIT-licensed Rust code at
//! [`pornin/ecgfp5`](https://github.com/pornin/ecgfp5) serves as a reading
//! reference alongside the curve's design paper. For Poseidon2 and the Schnorr
//! binding Lighter applies, the Go library
//! [`elliottech/poseidon_crypto`](https://github.com/elliottech/poseidon_crypto)
//! is the behavioural reference: parameter sets (round constants, MDS matrices)
//! are pulled from there as facts, and test vectors derived from it are
//! reproduced as fixtures so equivalence with the upstream behaviour is
//! verifiable end-to-end.
//!
//! Pornin's reference is also pulled in as a `#[cfg(test)]` dev-dep
//! (zero transitive deps; pinned by commit) and consumed by the
//! `pornin_diff` proptest module plus the `fuzz_pornin_diff_*` fuzz
//! targets. Every public algebra operation (`Fp5` add/sub/mul/neg/invert,
//! `Scalar` add/sub/mul/neg, `Point` decode/double/add, scalar mul on
//! arbitrary bases) is asserted byte-for-byte against the reference on
//! every random sample. The two implementations share no code lineage:
//! Pornin's accompanies the design paper (IACR ePrint 2022/274) and has
//! been public and reused by downstream zero-knowledge projects since
//! 2022; ours is written from the paper using his code as a reading
//! reference. A bug that slips the differential gate would have to be
//! present in both implementations in the same way, which is concretely
//! unlikely rather than merely theoretically possible.
//!
//! The official `lighter-go` and `lighter-python` SDKs wrap a closed-source
//! compiled signer; round-trip testing against testnet provides the final
//! correctness gate for what the Lighter sequencer accepts.
//!
//! See `crates/adapters/lighter/licenses/THIRD_PARTY_LICENSES.md` for the
//! attribution covering reference parameters and test vectors.

pub mod auth_token;
pub mod curve;
pub mod field;
pub mod hash;
pub mod nonce;
pub mod schnorr;
pub mod tx;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod pornin_diff;
