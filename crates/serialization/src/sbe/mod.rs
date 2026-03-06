//! Generic SBE (Simple Binary Encoding) decode utilities.
//!
//! This module provides:
//! - [`SbeCursor`]: Zero-copy byte cursor with typed little-endian readers.
//! - [`SbeDecodeError`]: Common decode errors for malformed or truncated payloads.
//! - [`GroupSizeEncoding`] and [`GroupSize16Encoding`]: Group header decoders.
//! - [`decode_var_string8`]: varString8 decoder helper.

pub mod cursor;
pub mod error;
pub mod primitives;

pub use cursor::SbeCursor;
pub use error::{MAX_GROUP_SIZE, SbeDecodeError};
pub use primitives::{GroupSize16Encoding, GroupSizeEncoding, decode_var_string8};
