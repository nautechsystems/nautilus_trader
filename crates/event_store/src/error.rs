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

//! Error types for the event store.

use thiserror::Error;

/// Errors returned by event store backends, the writer, the reader, and the verifier.
///
/// The variants form a typed surface for the operational policies described in the SPEC: disk
/// pressure halts the kernel, hash mismatch quarantines the affected run, and a crashed
/// predecessor is sealed by the caller before a new run is opened.
#[derive(Debug, Error)]
pub enum EventStoreError {
    /// A backend operation failed for a reason that does not fit any other variant.
    ///
    /// Typically wraps a redb error or an in-memory backend invariant violation.
    #[error("backend error: {0}")]
    Backend(String),
    /// A run file is structurally damaged and cannot be opened safely.
    ///
    /// Surfaces redb header-region corruption and any backend-detected structural failure.
    #[error("corrupted run: {0}")]
    Corrupted(String),
    /// The backing storage refused the write because of disk pressure.
    ///
    /// Maps to redb `Io(FileTooLarge)` and equivalent host-level failures (ENOSPC,
    /// `RLIMIT_FSIZE`). Triggers the kernel halt path; the writer fail-stops.
    #[error("disk error: {0}")]
    Disk(String),
    /// The canonical entry hash recorded with a row does not match the recomputed hash.
    ///
    /// Quarantines the affected run.
    #[error("entry hash mismatch at seq {seq}")]
    HashMismatch {
        /// The sequence number whose stored hash did not match.
        seq: u64,
    },
    /// The writer received an entry whose sequence number is not contiguous after the
    /// current durable high-watermark.
    #[error(
        "out-of-order seq: received {seq}, expected contiguous after high-watermark {high_watermark}"
    )]
    OutOfOrder {
        /// The current durable high-watermark.
        high_watermark: u64,
        /// The offending sequence number.
        seq: u64,
    },
    /// The run is sealed and cannot accept further writes.
    #[error("run is closed")]
    Closed,
    /// A reader processing `seq=N+1` did not observe `seq=N`.
    ///
    /// One of the four idempotency primitives: gap detection on read.
    #[error("gap detected: missing seq {missing} between {prev} and {next}")]
    Gap {
        /// The last seq the reader observed.
        prev: u64,
        /// The next seq the reader observed.
        next: u64,
        /// The first missing seq.
        missing: u64,
    },
    /// Opening a run found a predecessor whose status is `Running` and that lacks a
    /// `RunEnded` entry.
    ///
    /// The kernel is expected to seal the predecessor (as `CrashedRecovered`, or
    /// `Quarantined` if hash check fails) and then open a new run that records the
    /// predecessor's `run_id` as `parent_run_id`.
    #[error("crashed predecessor run requires sealing")]
    CrashedPredecessor,
}
