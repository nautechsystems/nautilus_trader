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

use thiserror::Error;

pub(crate) const ENCODE_PAIR: [[u8; 2]; 256] = {
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
pub(crate) const DECODE_NIBBLE: [u8; 256] = {
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
/// Panics if the output capacity exceeds `isize::MAX` bytes.
/// The output buffer is built from ASCII `ENCODE_PAIR` entries, so [`String::from_utf8`] always succeeds.
#[must_use]
pub fn encode(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let buf = bytes
        .iter()
        .flat_map(|&byte| ENCODE_PAIR[byte as usize])
        .collect();
    String::from_utf8(buf).expect("hex pairs are ASCII")
}

/// Encodes a byte slice as a `"0x"`-prefixed lowercase hexadecimal string.
///
/// # Panics
///
/// Panics if the output capacity exceeds `isize::MAX` bytes.
/// The output buffer is built from ASCII (`"0x"` plus `ENCODE_PAIR` entries), so [`String::from_utf8`] always succeeds.
#[must_use]
pub fn encode_prefixed(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let buf = b"0x"
        .iter()
        .copied()
        .chain(bytes.iter().flat_map(|&byte| ENCODE_PAIR[byte as usize]))
        .collect();
    String::from_utf8(buf).expect("hex pairs are ASCII")
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

    if hex.is_empty() {
        return Ok(Vec::new());
    }

    if hex.len() < 32 {
        let mut out = Vec::with_capacity(hex.len() / 2);
        decode_pairs_into(hex, &mut out)?;
        return Ok(out);
    }
    decode_packed(hex)
}

fn decode_packed(hex: &[u8]) -> Result<Vec<u8>, DecodeError> {
    // Reject errors in the first chunk before allocating an output buffer.
    let first = decode_4_bytes(
        hex[..8]
            .try_into()
            .expect("packed hex has at least eight characters"),
    )?;
    let mut out = vec![0; hex.len() / 2];
    out[..4].copy_from_slice(&first);
    decode_into(&hex[8..], &mut out[4..])?;
    Ok(out)
}

fn decode_pairs_into(hex: &[u8], out: &mut Vec<u8>) -> Result<(), DecodeError> {
    for pair in hex.as_chunks::<2>().0 {
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
    Ok(())
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
    decode_into(hex, &mut out)?;
    Ok(out)
}

// Callers establish exactly two input characters per output byte before entering this kernel
#[inline]
fn decode_into(hex: &[u8], out: &mut [u8]) -> Result<(), DecodeError> {
    debug_assert_eq!(
        hex.len(),
        out.len() * 2,
        "Invariant: hex has two characters per output byte"
    );
    let (chunks, tail) = hex.as_chunks::<8>();
    let (out_chunks, out_tail) = out.as_chunks_mut::<4>();
    for (decoded, chunk) in out_chunks.iter_mut().zip(chunks) {
        *decoded = decode_4_bytes(*chunk)?;
    }

    for (decoded, pair) in out_tail.iter_mut().zip(tail.as_chunks::<2>().0) {
        let hi = DECODE_NIBBLE[pair[0] as usize];
        let lo = DECODE_NIBBLE[pair[1] as usize];
        if (hi | lo) & 0xF0 != 0 {
            return Err(if hi == 0xFF {
                DecodeError::InvalidChar(pair[0])
            } else {
                DecodeError::InvalidChar(pair[1])
            });
        }
        *decoded = (hi << 4) | lo;
    }
    Ok(())
}

#[inline(always)]
fn decode_4_bytes(input: [u8; 8]) -> Result<[u8; 4], DecodeError> {
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;
    let word = u64::from_le_bytes(input);
    let ascii = word & !HIGH_BITS;
    let lowercase = ascii | 0x2020_2020_2020_2020;
    // These subtractions cannot borrow between lanes; bit 7 tests the inclusive ASCII ranges.
    let digits_low = (ascii | HIGH_BITS) - 0x3030_3030_3030_3030;
    let digits_high = 0xb9b9_b9b9_b9b9_b9b9 - ascii;
    let letters_low = (lowercase | HIGH_BITS) - 0x6161_6161_6161_6161;
    let letters_high = 0xe6e6_e6e6_e6e6_e6e6 - lowercase;
    let valid = ((digits_low & digits_high) | (letters_low & letters_high)) & HIGH_BITS;
    let invalid = (!valid | word) & HIGH_BITS;
    if invalid != 0 {
        // Little-endian lanes put the first invalid input byte at the lowest set bit.
        let index = (invalid.trailing_zeros() / 8) as usize;
        return Err(DecodeError::InvalidChar(input[index]));
    }

    // For validated ASCII hex, bit 6 distinguishes letters; each nibble fits its byte lane.
    let nibbles = (word & 0x0f0f_0f0f_0f0f_0f0f) + ((word >> 6) & 0x0303_0303_0303_0303) * 9;
    let pairs = ((nibbles & 0x000f_000f_000f_000f) << 4) | ((nibbles >> 8) & 0x000f_000f_000f_000f);
    let bytes = pairs.to_le_bytes();
    Ok([bytes[0], bytes[2], bytes[4], bytes[6]])
}

/// Errors from hex decoding.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// Input has an odd number of characters.
    #[error("odd number of hex characters")]
    OddLength,
    /// Input contains a non-hex byte.
    #[error("invalid hex character: {0:#04x}")]
    InvalidChar(u8),
    /// Input length does not match expected size.
    #[error("expected {expected} hex characters, was {actual}")]
    LengthMismatch {
        /// Expected hex string length.
        expected: usize,
        /// Actual hex string length.
        actual: usize,
    },
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::hex;

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

    fn decode_oracle(input: &[u8]) -> Result<Vec<u8>, DecodeError> {
        if !input.len().is_multiple_of(2) {
            return Err(DecodeError::OddLength);
        }
        input
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let hi = char::from(pair[0])
                    .to_digit(16)
                    .ok_or(DecodeError::InvalidChar(pair[0]))?;
                let lo = char::from(pair[1])
                    .to_digit(16)
                    .ok_or(DecodeError::InvalidChar(pair[1]))?;
                Ok(u8::try_from((hi << 4) | lo).unwrap())
            })
            .collect()
    }

    #[rstest]
    fn test_decode_checks_every_byte_at_each_packed_position() {
        for position in 0..8 {
            for byte in 0..=u8::MAX {
                let mut input = *b"01aBcDeF";
                input[position] = byte;
                let expected = decode_oracle(&input);
                assert_eq!(hex::decode(input), expected);
                assert_eq!(hex::decode_array::<4>(input).map(Vec::from), expected);
            }
        }
    }

    #[rstest]
    fn test_decode_checks_every_byte_across_wide_chunks() {
        for position in 0..48 {
            for byte in 0..=u8::MAX {
                let mut input = *b"01aBcDeF01aBcDeF01aBcDeF01aBcDeF01aBcDeF01aBcDeF";
                input[position] = byte;
                let expected = decode_oracle(&input);
                assert_eq!(hex::decode(input), expected);
                assert_eq!(hex::decode_array::<24>(input).map(Vec::from), expected);
            }
        }
    }

    #[rstest]
    fn test_decode_reports_first_invalid_byte_across_wide_chunks_and_tail() {
        for position in 0..50 {
            let mut input = *b"01aBcDeF01aBcDeF01aBcDeF01aBcDeF01aBcDeF01aBcDeF01";
            input[position] = 0xff;
            if position + 1 < input.len() {
                input[position + 1] = b'z';
            }
            assert_eq!(hex::decode(input), Err(DecodeError::InvalidChar(0xff)));
            assert_eq!(
                hex::decode_array::<25>(input),
                Err(DecodeError::InvalidChar(0xff))
            );
        }
    }

    #[rstest]
    fn test_decode_reports_first_invalid_byte_after_complete_chunks() {
        for position in 0..24 {
            let mut input = *b"01aBcDeF01aBcDeF01aBcDeF";
            input[position] = 0xff;
            if position + 1 < input.len() {
                input[position + 1] = b'z';
            }
            assert_eq!(hex::decode(input), Err(DecodeError::InvalidChar(0xff)));
            assert_eq!(
                hex::decode_array::<12>(input),
                Err(DecodeError::InvalidChar(0xff))
            );
        }
    }

    #[rstest]
    fn test_decode_length_errors_precede_invalid_characters() {
        assert_eq!(hex::decode([0xff; 9]), Err(DecodeError::OddLength));
        assert_eq!(
            hex::decode_array::<4>([0xff; 9]),
            Err(DecodeError::LengthMismatch {
                expected: 8,
                actual: 9
            })
        );
    }

    fn assert_array_roundtrip<const N: usize>() {
        let bytes = std::array::from_fn::<_, N, _>(|i| u8::try_from(i * 7).unwrap());
        let text: String = format_hex_reference(&bytes).to_ascii_uppercase();
        assert_eq!(hex::decode_array::<N>(&text), Ok(bytes));
        assert_eq!(hex::decode(&text), Ok(bytes.to_vec()));
    }

    #[rstest]
    fn test_decode_array_handles_empty_short_and_partial_chunks() {
        assert_array_roundtrip::<0>();
        assert_array_roundtrip::<1>();
        assert_array_roundtrip::<2>();
        assert_array_roundtrip::<3>();
        assert_array_roundtrip::<4>();
        assert_array_roundtrip::<5>();
        assert_array_roundtrip::<6>();
        assert_array_roundtrip::<7>();
        assert_array_roundtrip::<8>();
        assert_array_roundtrip::<9>();
        assert_array_roundtrip::<32>();
    }

    proptest! {
        #[rstest]
        fn prop_decode_preserves_values_and_error_priority(input in prop::collection::vec(any::<u8>(), 0..256)) {
            prop_assert_eq!(hex::decode(&input), decode_oracle(&input));
        }

        #[rstest]
        fn prop_decode_matches_numeric_formatting(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
            let text: String = format_hex_reference(&bytes).to_ascii_uppercase();
            prop_assert_eq!(hex::decode(&text), Ok(bytes));
        }
    }

    #[rstest]
    fn test_hex_covers_every_byte() {
        let bytes: Vec<u8> = (0..=u8::MAX).collect();
        let expected: String = format_hex_reference(&bytes);
        assert_eq!(hex::encode(&bytes), expected);
        assert_eq!(hex::encode_prefixed(&bytes), format!("0x{expected}"));
        assert_eq!(hex::decode(expected.to_uppercase()).unwrap(), bytes);
    }

    proptest! {
        #[rstest]
        fn prop_hex_matches_numeric_formatting(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
            let expected: String = format_hex_reference(&bytes);
            prop_assert_eq!(hex::encode(&bytes), expected);
        }
    }

    fn format_hex_reference(bytes: &[u8]) -> String {
        let mut text = String::with_capacity(bytes.len() * 2);

        for byte in bytes {
            write!(text, "{byte:02x}").unwrap();
        }
        text
    }
}
