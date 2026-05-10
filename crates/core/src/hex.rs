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

//! Hexadecimal encoding and decoding for byte slices.

use std::fmt::Display;

const ENCODE_PAIR: [[u8; 2]; 256] = {
    const NIBBLE: [u8; 16] = *b"0123456789abcdef";
    let mut table = [[0u8; 2]; 256];
    let mut i = 0u16;
    while i < 256 {
        table[i as usize] = [NIBBLE[(i >> 4) as usize], NIBBLE[(i & 0x0f) as usize]];
        i += 1;
    }
    table
};

// 0xFF sentinel marks invalid hex characters
const DECODE_NIBBLE: [u8; 256] = {
    let mut table = [0xFFu8; 256];
    let mut i = 0u8;
    while i < 10 {
        table[(b'0' + i) as usize] = i;
        i += 1;
    }
    i = 0;
    while i < 6 {
        table[(b'a' + i) as usize] = 10 + i;
        table[(b'A' + i) as usize] = 10 + i;
        i += 1;
    }
    table
};

/// Encodes a byte slice as a lowercase hexadecimal string.
///
/// # Panics
///
/// Never panics in practice: the output buffer is built from ASCII hex pairs in
/// `ENCODE_PAIR`, so [`String::from_utf8`] always succeeds.
#[must_use]
pub fn encode(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let mut buf = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        buf.extend_from_slice(&ENCODE_PAIR[b as usize]);
    }
    String::from_utf8(buf).unwrap()
}

/// Encodes a byte slice as a `"0x"`-prefixed lowercase hexadecimal string.
///
/// # Panics
///
/// Never panics in practice: the output buffer is built from ASCII (`"0x"` plus
/// `ENCODE_PAIR` entries), so [`String::from_utf8`] always succeeds.
#[must_use]
pub fn encode_prefixed(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let mut buf = Vec::with_capacity(2 + bytes.len() * 2);
    buf.extend_from_slice(b"0x");
    for &b in bytes {
        buf.extend_from_slice(&ENCODE_PAIR[b as usize]);
    }
    String::from_utf8(buf).unwrap()
}

/// Decodes a hexadecimal string into bytes.
///
/// # Errors
///
/// Returns [`DecodeError`] if the input length is odd or contains non-hex characters.
pub fn decode(data: impl AsRef<[u8]>) -> Result<Vec<u8>, DecodeError> {
    let hex = data.as_ref();
    if hex.len() % 2 != 0 {
        return Err(DecodeError::OddLength);
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
        let hi = DECODE_NIBBLE[pair[0] as usize];
        let lo = DECODE_NIBBLE[pair[1] as usize];
        if (hi | lo) & 0xF0 != 0 {
            return Err(if hi == 0xFF {
                DecodeError::InvalidChar(pair[0])
            } else {
                DecodeError::InvalidChar(pair[1])
            });
        }
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Decodes a hexadecimal string into a fixed-size byte array.
///
/// # Errors
///
/// Returns [`DecodeError`] if the input length is not exactly `2 * N` or contains
/// non-hex characters.
pub fn decode_array<const N: usize>(data: impl AsRef<[u8]>) -> Result<[u8; N], DecodeError> {
    let hex = data.as_ref();
    if hex.len() != N * 2 {
        return Err(DecodeError::LengthMismatch {
            expected: N * 2,
            actual: hex.len(),
        });
    }
    let mut out = [0u8; N];

    for (i, pair) in hex.chunks_exact(2).enumerate() {
        let hi = DECODE_NIBBLE[pair[0] as usize];
        let lo = DECODE_NIBBLE[pair[1] as usize];
        if (hi | lo) & 0xF0 != 0 {
            return Err(if hi == 0xFF {
                DecodeError::InvalidChar(pair[0])
            } else {
                DecodeError::InvalidChar(pair[1])
            });
        }
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

/// Errors from hex decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Input has an odd number of characters.
    OddLength,
    /// Input contains a non-hex byte.
    InvalidChar(u8),
    /// Input length does not match expected size.
    LengthMismatch {
        /// Expected hex string length.
        expected: usize,
        /// Actual hex string length.
        actual: usize,
    },
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OddLength => f.write_str("odd number of hex characters"),
            Self::InvalidChar(b) => write!(f, "invalid hex character: {b:#04x}"),
            Self::LengthMismatch { expected, actual } => {
                write!(f, "expected {expected} hex characters, was {actual}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(b"", "")]
    #[case(b"\x00", "00")]
    #[case(b"\xff", "ff")]
    #[case(b"\xde\xad\xbe\xef", "deadbeef")]
    #[case(b"hello", "68656c6c6f")]
    fn test_encode(#[case] input: &[u8], #[case] expected: &str) {
        assert_eq!(encode(input), expected);
    }

    #[rstest]
    #[case("", b"")]
    #[case("00", b"\x00")]
    #[case("ff", b"\xff")]
    #[case("FF", b"\xff")]
    #[case("deadBEEF", b"\xde\xad\xbe\xef")]
    #[case("68656c6c6f", b"hello")]
    fn test_decode(#[case] input: &str, #[case] expected: &[u8]) {
        assert_eq!(decode(input).unwrap(), expected);
    }

    #[rstest]
    fn test_decode_odd_length() {
        assert_eq!(decode("abc"), Err(DecodeError::OddLength));
    }

    #[rstest]
    #[case("zz", DecodeError::InvalidChar(b'z'))]
    #[case("z0", DecodeError::InvalidChar(b'z'))]
    #[case("0z", DecodeError::InvalidChar(b'z'))]
    fn test_decode_invalid_char(#[case] input: &str, #[case] expected: DecodeError) {
        assert_eq!(decode(input), Err(expected));
    }

    #[rstest]
    #[case(b"", "0x")]
    #[case(b"\xde\xad", "0xdead")]
    #[case(b"hello", "0x68656c6c6f")]
    fn test_encode_prefixed(#[case] input: &[u8], #[case] expected: &str) {
        assert_eq!(encode_prefixed(input), expected);
    }

    #[rstest]
    fn test_decode_array() {
        let result: [u8; 4] = decode_array("deadbeef").unwrap();
        assert_eq!(result, [0xde, 0xad, 0xbe, 0xef]);
    }

    #[rstest]
    fn test_decode_array_invalid_char() {
        assert_eq!(
            decode_array::<2>("xxff"),
            Err(DecodeError::InvalidChar(b'x'))
        );
    }

    #[rstest]
    fn test_decode_array_length_mismatch() {
        let result = decode_array::<4>("aabb");
        assert_eq!(
            result,
            Err(DecodeError::LengthMismatch {
                expected: 8,
                actual: 4
            })
        );
    }

    #[rstest]
    #[case(DecodeError::OddLength, "odd number of hex characters")]
    #[case(DecodeError::InvalidChar(b'z'), "invalid hex character: 0x7a")]
    #[case(
        DecodeError::LengthMismatch { expected: 8, actual: 4 },
        "expected 8 hex characters, was 4"
    )]
    fn test_decode_error_display(#[case] error: DecodeError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_roundtrip() {
        let data = b"The quick brown fox jumps over the lazy dog";
        assert_eq!(decode(encode(data)).unwrap(), data);
    }
}
