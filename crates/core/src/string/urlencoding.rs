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

//! URL percent-encoding and decoding per [RFC 3986].
//!
//! The unreserved set is `ALPHA / DIGIT / "-" / "." / "_" / "~"`; every other
//! byte is percent-encoded as `%HH` using uppercase hexadecimal as recommended
//! by [RFC 3986 Section 2.1].
//!
//! Decoding accepts both uppercase and lowercase hex. A `%` that is not
//! followed by two hex digits is passed through literally, matching the
//! behaviour of the `urlencoding` crate that this module replaces.
//!
//! [RFC 3986]: https://datatracker.ietf.org/doc/html/rfc3986
//! [RFC 3986 Section 2.1]: https://datatracker.ietf.org/doc/html/rfc3986#section-2.1

use std::{borrow::Cow, fmt::Display, string::FromUtf8Error};

const UNRESERVED: [bool; 256] = {
    let mut table = [false; 256];
    let mut i = b'0';
    while i <= b'9' {
        table[i as usize] = true;
        i += 1;
    }
    i = b'A';
    while i <= b'Z' {
        table[i as usize] = true;
        i += 1;
    }
    i = b'a';
    while i <= b'z' {
        table[i as usize] = true;
        i += 1;
    }
    table[b'-' as usize] = true;
    table[b'.' as usize] = true;
    table[b'_' as usize] = true;
    table[b'~' as usize] = true;
    table
};

const ENCODE_PAIR: [[u8; 2]; 256] = {
    const NIBBLE: [u8; 16] = *b"0123456789ABCDEF";
    let mut table = [[0u8; 2]; 256];
    let mut i = 0u16;
    while i < 256 {
        table[i as usize] = [NIBBLE[(i >> 4) as usize], NIBBLE[(i & 0x0f) as usize]];
        i += 1;
    }
    table
};

// 0xFF sentinel marks non-hex characters
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

/// Percent-encodes a string per RFC 3986.
///
/// Returns the input borrowed when every byte is already in the unreserved
/// set, otherwise an owned encoded copy.
///
/// # Panics
///
/// Never panics in practice: [`encode_bytes`] only emits ASCII bytes
/// (unreserved characters or `%HH` pairs), so [`String::from_utf8`] always
/// succeeds.
#[must_use]
pub fn encode(input: &str) -> Cow<'_, str> {
    match encode_bytes(input.as_bytes()) {
        Cow::Borrowed(_) => Cow::Borrowed(input),
        Cow::Owned(bytes) => Cow::Owned(String::from_utf8(bytes).expect("encoded output is ASCII")),
    }
}

/// Percent-encodes a byte slice per RFC 3986.
///
/// Returns the input borrowed when every byte is already in the unreserved
/// set, otherwise an owned encoded copy.
#[must_use]
pub fn encode_bytes(input: &[u8]) -> Cow<'_, [u8]> {
    let Some(first) = input.iter().position(|&b| !UNRESERVED[b as usize]) else {
        return Cow::Borrowed(input);
    };

    // Slack for payloads dominated by reserved chars without over-allocating
    // on mostly-unreserved inputs; Vec's geometric growth covers the rest.
    let mut out = Vec::with_capacity(input.len() + input.len() / 2 + 16);
    out.extend_from_slice(&input[..first]);

    let mut rest = &input[first..];
    while let Some(&byte) = rest.first() {
        if UNRESERVED[byte as usize] {
            let run_end = rest
                .iter()
                .position(|&b| !UNRESERVED[b as usize])
                .unwrap_or(rest.len());
            out.extend_from_slice(&rest[..run_end]);
            rest = &rest[run_end..];
        } else {
            out.push(b'%');
            out.extend_from_slice(&ENCODE_PAIR[byte as usize]);
            rest = &rest[1..];
        }
    }
    Cow::Owned(out)
}

/// Percent-decodes a string per RFC 3986.
///
/// Returns the input borrowed when no `%` is present. Otherwise decodes
/// `%HH` pairs (hex is case-insensitive) and leaves any `%` that is not
/// followed by two hex digits in place.
///
/// # Errors
///
/// Returns [`DecodeError::InvalidUtf8`] if the decoded bytes are not valid
/// UTF-8.
pub fn decode(input: &str) -> Result<Cow<'_, str>, DecodeError> {
    match decode_bytes(input.as_bytes()) {
        Cow::Borrowed(_) => Ok(Cow::Borrowed(input)),
        Cow::Owned(bytes) => String::from_utf8(bytes)
            .map(Cow::Owned)
            .map_err(DecodeError::InvalidUtf8),
    }
}

/// Percent-decodes a byte slice.
///
/// Returns the input borrowed when no `%` is present. A `%` that is not
/// followed by two hex digits is left in place.
#[must_use]
pub fn decode_bytes(input: &[u8]) -> Cow<'_, [u8]> {
    let Some(first) = input.iter().position(|&b| b == b'%') else {
        return Cow::Borrowed(input);
    };

    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&input[..first]);

    let mut i = first;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 < input.len() {
                let hi = DECODE_NIBBLE[input[i + 1] as usize];
                let lo = DECODE_NIBBLE[input[i + 2] as usize];
                if (hi | lo) & 0xF0 == 0 {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
            }
            // Malformed or trailing `%`: pass through literally.
            out.push(b'%');
            i += 1;
        } else {
            let run_start = i;
            while i < input.len() && input[i] != b'%' {
                i += 1;
            }
            out.extend_from_slice(&input[run_start..i]);
        }
    }
    Cow::Owned(out)
}

/// Errors from URL percent-decoding.
#[derive(Debug)]
pub enum DecodeError {
    /// Decoded bytes are not valid UTF-8.
    InvalidUtf8(FromUtf8Error),
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUtf8(err) => write!(f, "invalid UTF-8 in decoded bytes: {err}"),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUtf8(err) => Some(err),
        }
    }
}

impl From<FromUtf8Error> for DecodeError {
    fn from(err: FromUtf8Error) -> Self {
        Self::InvalidUtf8(err)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // RFC 3986 Section 2.3: unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"
    const UNRESERVED_CHARS: &str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

    // RFC 3986 Section 2.2: reserved chars (gen-delims + sub-delims) must be
    // percent-encoded when used as data.
    const RESERVED_CHARS: &str = ":/?#[]@!$&'()*+,;=";

    #[rstest]
    #[case("", "")]
    #[case("abc", "abc")]
    #[case("ABC-xyz_0.9~", "ABC-xyz_0.9~")]
    #[case(" ", "%20")]
    #[case("+", "%2B")]
    #[case("/", "%2F")]
    #[case("?", "%3F")]
    #[case("#", "%23")]
    #[case("&", "%26")]
    #[case("=", "%3D")]
    #[case("%", "%25")]
    #[case("hello world", "hello%20world")]
    #[case("a b+c/d", "a%20b%2Bc%2Fd")]
    // Uppercase hex per RFC 3986 Section 2.1
    #[case("\x7f", "%7F")]
    fn test_encode_ascii_vectors(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(encode(input), expected);
    }

    #[rstest]
    fn test_encode_all_unreserved_unchanged() {
        // Every char in the unreserved set should pass through.
        let out = encode(UNRESERVED_CHARS);
        assert_eq!(out, UNRESERVED_CHARS);
        // And the Cow should be Borrowed (zero-copy).
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[rstest]
    fn test_encode_all_reserved_percent_encoded() {
        let out = encode(RESERVED_CHARS);
        // Each of the 18 chars becomes a 3-byte `%HH` sequence.
        assert_eq!(out.len(), RESERVED_CHARS.len() * 3);
        // None of the unreserved chars, `%`, or digits A-F should appear raw
        // in the output except as part of a `%HH` triple.
        for byte in out.bytes() {
            assert!(
                matches!(byte, b'%' | b'0'..=b'9' | b'A'..=b'F'),
                "unexpected byte {byte:#04x} in encoded reserved output"
            );
        }
    }

    #[rstest]
    fn test_encode_hex_is_uppercase() {
        // Verify RFC 3986 Section 2.1: producers SHOULD emit uppercase hex.
        let out = encode("/");
        assert_eq!(out, "%2F");
        assert!(!out.contains('f'));
    }

    #[rstest]
    fn test_encode_every_byte_position() {
        // For each byte 0x00..=0xFF, encode a one-byte slice and verify that
        // the output matches the spec expectation.
        for byte in 0u8..=255 {
            let input = [byte];
            let out = encode_bytes(&input);

            if UNRESERVED[byte as usize] {
                assert!(
                    matches!(out, Cow::Borrowed(_)),
                    "unreserved byte {byte:#04x} should not allocate"
                );
                assert_eq!(out.as_ref(), &[byte]);
            } else {
                let expected = format!("%{byte:02X}").into_bytes();
                assert_eq!(out.as_ref(), expected.as_slice(), "byte {byte:#04x}");
            }
        }
    }

    #[rstest]
    fn test_encode_utf8_multibyte() {
        // U+00E9 encoded as UTF-8 is `0xC3 0xA9` (two bytes).
        assert_eq!(encode("\u{00E9}"), "%C3%A9");
        // U+4E2D encoded as UTF-8 is `0xE4 0xB8 0xAD` (three bytes).
        assert_eq!(encode("\u{4E2D}"), "%E4%B8%AD");
        // Grinning face emoji U+1F600 is `0xF0 0x9F 0x98 0x80` (four bytes).
        assert_eq!(encode("\u{1F600}"), "%F0%9F%98%80");
    }

    #[rstest]
    fn test_encode_mixed_ascii_and_utf8() {
        assert_eq!(encode("a é/"), "a%20%C3%A9%2F");
    }

    #[rstest]
    fn test_encode_returns_borrowed_when_no_work() {
        let out = encode("safe-string_123.xyz~");
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[rstest]
    fn test_encode_returns_owned_when_encoding_needed() {
        let out = encode("needs encoding");
        assert!(matches!(out, Cow::Owned(_)));
    }

    #[rstest]
    #[case("", "")]
    #[case("abc", "abc")]
    #[case("%20", " ")]
    #[case("%2F", "/")]
    #[case("%2f", "/")] // lowercase hex must be accepted
    #[case("%2b", "+")]
    #[case("%25", "%")]
    #[case("hello%20world", "hello world")]
    #[case("a%20b%2Bc%2Fd", "a b+c/d")]
    #[case("%C3%A9", "\u{00E9}")]
    #[case("%E4%B8%AD", "\u{4E2D}")]
    #[case("%F0%9F%98%80", "\u{1F600}")]
    fn test_decode_ascii_and_utf8_vectors(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(decode(input).unwrap(), expected);
    }

    #[rstest]
    #[case("%", "%")] // bare `%` at end passes through
    #[case("%2", "%2")] // one hex digit at end
    #[case("%GG", "%GG")] // non-hex digits
    #[case("%2G", "%2G")] // second nibble invalid
    #[case("%G2", "%G2")] // first nibble invalid
    #[case("%%20", "% ")] // first `%` literal, then `%20` decodes
    #[case("100%", "100%")] // `%` at end after ASCII
    fn test_decode_malformed_percent_passes_through(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(decode(input).unwrap(), expected);
    }

    #[rstest]
    fn test_decode_returns_borrowed_when_no_percent() {
        let out = decode("no-percent-here").unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[rstest]
    fn test_decode_returns_owned_when_percent_present() {
        let out = decode("a%20b").unwrap();
        assert!(matches!(out, Cow::Owned(_)));
    }

    #[rstest]
    #[case("this%2x%26that", "this%2x&that")]
    #[case("%%25", "%%")]
    #[case("%2%26", "%2&")]
    #[case("a%2Zb%20c", "a%2Zb c")]
    fn test_decode_malformed_then_valid(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(decode(input).unwrap(), expected);
    }

    #[rstest]
    fn test_decode_invalid_utf8_errors() {
        // `0xFF` is not valid UTF-8 on its own.
        let err = decode("%FF").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidUtf8(_)));
    }

    #[rstest]
    fn test_decode_invalid_utf8_bytes_ok() {
        // `decode_bytes` does not validate UTF-8.
        let out = decode_bytes(b"%FF");
        assert_eq!(out.as_ref(), &[0xFF]);
    }

    #[rstest]
    fn test_decode_consecutive_percent_triples() {
        // Three consecutive `%HH` sequences decoding multi-byte UTF-8.
        assert_eq!(decode("%e2%98%83").unwrap(), "\u{2603}"); // snowman U+2603
    }

    #[rstest]
    fn test_decode_nul_byte() {
        // `%00` decodes to the NUL byte, which is valid UTF-8 (U+0000).
        let decoded = decode("a%00b").unwrap();
        assert_eq!(decoded.as_bytes(), &[b'a', 0x00, b'b']);
    }

    #[rstest]
    fn test_roundtrip_every_byte() {
        // For every byte 0x00..=0xFF, encoding then decoding must recover
        // the original byte exactly.
        for byte in 0u8..=255 {
            let input = [byte];
            let encoded = encode_bytes(&input);
            let decoded = decode_bytes(encoded.as_ref());
            assert_eq!(
                decoded.as_ref(),
                input.as_slice(),
                "round-trip failed for byte {byte:#04x}"
            );
        }
    }

    #[rstest]
    #[case("hello")]
    #[case("a b c")]
    #[case("https://example.com/path?q=1&x=2")]
    #[case("\u{00E9}\u{00E0}\u{00FC}")]
    #[case("\u{4E2D}\u{6587}\u{6D4B}\u{8BD5}")]
    #[case("mix 123 !@# %^&*()")]
    #[case("\u{1F600}\u{1F680}\u{1F3C6}")]
    fn test_roundtrip_string(#[case] input: &str) {
        let encoded = encode(input);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[rstest]
    fn test_encoded_output_only_ascii() {
        // Encoded output must always be pure ASCII (unreserved bytes + `%HH`).
        let encoded = encode("\u{00E9}\u{4E2D}\u{1F600}");
        assert!(encoded.is_ascii(), "encoded output must be ASCII-only");
    }

    #[rstest]
    fn test_encode_bytes_arbitrary_binary() {
        // Encoding arbitrary bytes (including non-UTF-8) yields a valid
        // percent-encoded ASCII sequence.
        let input: Vec<u8> = (0u8..=255).collect();
        let encoded = encode_bytes(&input);
        assert!(encoded.iter().all(u8::is_ascii));
        let decoded = decode_bytes(encoded.as_ref());
        assert_eq!(decoded.as_ref(), input.as_slice());
    }

    #[rstest]
    fn test_decode_error_display_and_source() {
        let err = decode("%FF").unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("invalid UTF-8"), "got: {msg}");
        assert!(std::error::Error::source(&err).is_some());
    }

    // Independent reference implementation used to cross-check our tuned
    // implementation on random inputs. Pure-Rust, loop-based, no table
    // lookups: if both agree across thousands of random inputs we have
    // strong evidence the tuned version is spec-correct.
    fn reference_encode(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            let is_unreserved =
                b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~';
            if is_unreserved {
                out.push(b);
            } else {
                out.push(b'%');
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
        }
        out
    }

    fn reference_decode(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        let mut i = 0;
        while i < input.len() {
            if input[i] == b'%' && i + 2 < input.len() {
                let a = input[i + 1];
                let b = input[i + 2];
                if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() {
                    let hi = if a.is_ascii_digit() {
                        a - b'0'
                    } else {
                        (a | 0x20) - b'a' + 10
                    };
                    let lo = if b.is_ascii_digit() {
                        b - b'0'
                    } else {
                        (b | 0x20) - b'a' + 10
                    };
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
            }
            out.push(input[i]);
            i += 1;
        }
        out
    }

    proptest::proptest! {
        #[rstest]
        fn prop_encode_matches_reference(input: Vec<u8>) {
            let actual = encode_bytes(&input);
            let expected = reference_encode(&input);
            proptest::prop_assert_eq!(actual.as_ref(), expected.as_slice());
        }

        #[rstest]
        fn prop_decode_matches_reference(input: Vec<u8>) {
            let actual = decode_bytes(&input);
            let expected = reference_decode(&input);
            proptest::prop_assert_eq!(actual.as_ref(), expected.as_slice());
        }

        #[rstest]
        fn prop_bytes_roundtrip(input: Vec<u8>) {
            let encoded = encode_bytes(&input);
            let decoded = decode_bytes(encoded.as_ref());
            proptest::prop_assert_eq!(decoded.as_ref(), input.as_slice());
        }

        #[rstest]
        fn prop_string_roundtrip(input: String) {
            let encoded = encode(&input);
            let decoded = decode(&encoded).unwrap();
            proptest::prop_assert_eq!(decoded.as_ref(), input.as_str());
        }

        #[rstest]
        fn prop_encoded_output_ascii(input: Vec<u8>) {
            let encoded = encode_bytes(&input);
            proptest::prop_assert!(encoded.iter().all(u8::is_ascii));
        }
    }
}
