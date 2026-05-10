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

//! Deterministic two-way encoder for Binance Link broker ID prefixing.
//!
//! The Binance broker ID is automatically prefixed to all system-generated
//! client order IDs for every order placed through the Binance adapter. This
//! prefixing is transparent to strategies and requires no user configuration.
//! Inbound order events are decoded back to the original `ClientOrderId`
//! before reaching the trading system.
//!
//! Binance's [Link and Trade] program requires the `newClientOrderId`
//! parameter to start with `x-{BROKER_ID}` for order attribution. Binance
//! enforces a 36-character limit on this field with the regex
//! `^[\.A-Z\:/a-z0-9_-]{1,36}$`.
//!
//! Internal Nautilus `ClientOrderId` values (O-format: 23+ chars, UUID: 32-36
//! chars) exceed the 36-char limit when combined with the broker prefix. This
//! module provides compact, deterministic, two-way encoding via pure functions.
//!
//! [Link and Trade]: https://developers.binance.com/docs/binance_link/link-and-trade
//!
//! # Wire format
//!
//! ```text
//! x-TD67BGP9-{signal}{base62_payload}
//! |-- prefix -||- encoded component -|
//! ```
//!
//! The prefix `x-{BROKER_ID}-` is 11 chars (for an 8-char broker ID), leaving
//! 25 chars for the encoded component. Spot and Futures use separate broker
//! IDs defined in [`consts`](super::consts).
//!
//! # Signal chars
//!
//! The first character after the prefix identifies the original format so the
//! decoder can reconstruct the exact original `ClientOrderId` string.
//!
//! | Signal | Original format            | Payload length | Total |
//! |--------|----------------------------|----------------|-------|
//! | `T`    | O-format with hyphens      | 13 base62      | 25    |
//! | `t`    | O-format without hyphens   | 13 base62      | 25    |
//! | `U`    | UUID with hyphens          | 22 base62      | 34    |
//! | `u`    | UUID without hyphens       | 22 base62      | 34    |
//! | `R`    | Raw passthrough            | variable       | <= 36 |
//!
//! # O-format packing (72 bits -> 13 base62 chars)
//!
//! The O-format `ClientOrderId` `O-YYYYMMDD-HHMMSS-TTT-SSS-CCC` is packed
//! into a 72-bit integer:
//!
//! ```text
//! bits [71:40] (32 bits): seconds since 2020-01-01 epoch
//! bits [39:30] (10 bits): trader tag (0-1023)
//! bits [29:20] (10 bits): strategy tag (0-1023)
//! bits [19:0]  (20 bits): count (0-1048575)
//! ```
//!
//! # UUID packing (128 bits -> 22 base62 chars)
//!
//! The UUID is parsed from hex into a 128-bit integer and base62-encoded.
//!
//! # Decoding
//!
//! If the encoded string starts with the broker prefix, the decoder strips
//! it, reads the signal char, and reconstructs the original `ClientOrderId`.
//! Strings without the prefix are returned as-is for backward compatibility
//! with orders placed before broker ID support.
//!
//! # Performance
//!
//! Encoding adds sub-microsecond overhead per order operation, negligible
//! compared to network round-trip latency (typically 1-10 ms). Measured on
//! AMD Ryzen 9 7950X (release build, 100k iterations):
//!
//! | Operation          | ns/op |
//! |--------------------|-------|
//! | encode O-format    |  ~70  |
//! | decode O-format    | ~178  |
//! | encode UUID        | ~208  |
//! | decode UUID        |  ~46  |
//! | encode raw         |  ~14  |
//! | decode raw         |  ~14  |
//! | decode passthrough |  ~13  |
//!
//! Uses stack-allocated base62 output, manual civil time arithmetic (no
//! chrono), and direct byte-level hex/digit parsing to avoid heap allocations
//! on the hot path.
//!
//! Note: `cargo bench` cannot currently run in this workspace due to a
//! cdylib output filename collision (see <https://github.com/rust-lang/cargo/issues/6313>).
//! Use `cargo test --release -p nautilus-binance --lib -- bench_encode_decode_timing --nocapture`
//! to reproduce these numbers.

use nautilus_model::identifiers::ClientOrderId;

/// Base62 encoding alphabet: `0-9 A-Z a-z`.
const BASE62_CHARS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Lookup table mapping ASCII byte values to base62 digit values.
/// Invalid characters map to `0xFF`.
const BASE62_DECODE: [u8; 128] = {
    let mut table = [0xFFu8; 128];
    let mut i = 0u8;
    while i < 62 {
        table[BASE62_CHARS[i as usize] as usize] = i;
        i += 1;
    }
    table
};

/// Base epoch for O-format timestamp encoding: 2020-01-01 00:00:00 UTC.
const O_FORMAT_EPOCH: i64 = 1_577_836_800;

/// Fixed base62 output length for O-format packed values (72 bits).
const O_FORMAT_B62_LEN: usize = 13;

/// Fixed base62 output length for UUID packed values (128 bits).
const UUID_B62_LEN: usize = 22;

/// Maximum `newClientOrderId` length allowed by the Binance API.
const MAX_CLIENT_ORDER_ID_LEN: usize = 36;

const SIGNAL_O_HYPHENS: u8 = b'T';
const SIGNAL_O_NO_HYPHENS: u8 = b't';
const SIGNAL_UUID_HYPHENS: u8 = b'U';
const SIGNAL_UUID_NO_HYPHENS: u8 = b'u';
const SIGNAL_RAW: u8 = b'R';

/// Formats a broker prefix string from a broker ID: `x-{broker_id}-`.
#[must_use]
fn broker_prefix(broker_id: &str) -> String {
    format!("x-{broker_id}-")
}

/// Encodes a `ClientOrderId` into a Binance-compatible string with broker ID
/// prefix.
///
/// The encoding is deterministic and reversible with [`decode_broker_id`].
#[must_use]
pub fn encode_broker_id(client_order_id: &ClientOrderId, broker_id: &str) -> String {
    let id_str = client_order_id.as_str();
    let prefix = broker_prefix(broker_id);
    let budget = MAX_CLIENT_ORDER_ID_LEN - prefix.len();

    if let Some((packed, has_hyphens)) = pack_o_format(id_str) {
        let signal = if has_hyphens {
            SIGNAL_O_HYPHENS
        } else {
            SIGNAL_O_NO_HYPHENS
        };
        let b62 = encode_base62::<O_FORMAT_B62_LEN>(packed);
        return build_encoded(&prefix, signal, &b62);
    }

    if let Some((value, has_hyphens)) = parse_uuid_hex(id_str) {
        let signal = if has_hyphens {
            SIGNAL_UUID_HYPHENS
        } else {
            SIGNAL_UUID_NO_HYPHENS
        };
        let b62 = encode_base62::<UUID_B62_LEN>(value);
        return build_encoded(&prefix, signal, &b62);
    }

    if id_str.len() < budget {
        let mut result = String::with_capacity(prefix.len() + 1 + id_str.len());
        result.push_str(&prefix);
        result.push(SIGNAL_RAW as char);
        result.push_str(id_str);
        return result;
    }

    log::warn!(
        "ClientOrderId '{id_str}' ({} chars) exceeds broker ID encoding budget ({budget} chars), sending without prefix",
        id_str.len(),
    );
    id_str.to_string()
}

/// Decodes an encoded string back to the original `ClientOrderId` value.
///
/// If the string starts with a known broker prefix, the payload is decoded and
/// the original ID is reconstructed. Strings without a recognized prefix are
/// returned as-is for backward compatibility.
#[must_use]
pub fn decode_broker_id(encoded: &str, broker_id: &str) -> String {
    let prefix = broker_prefix(broker_id);
    let Some(payload) = encoded.strip_prefix(&prefix) else {
        return encoded.to_string();
    };

    if payload.is_empty() {
        return encoded.to_string();
    }

    let signal = payload.as_bytes()[0];
    let data = &payload[1..];

    match signal {
        SIGNAL_O_HYPHENS => unpack_o_format(data, true),
        SIGNAL_O_NO_HYPHENS => unpack_o_format(data, false),
        SIGNAL_UUID_HYPHENS => format_uuid(data, true),
        SIGNAL_UUID_NO_HYPHENS => format_uuid(data, false),
        SIGNAL_RAW => data.to_string(),
        _ => {
            log::warn!("Unknown broker ID signal byte '{signal}', returning raw");
            encoded.to_string()
        }
    }
}

fn build_encoded(prefix: &str, signal: u8, b62: &[u8]) -> String {
    let mut result = String::with_capacity(prefix.len() + 1 + b62.len());
    result.push_str(prefix);
    result.push(signal as char);
    // base62 output is always valid ASCII
    result.push_str(std::str::from_utf8(b62).expect("base62 is valid UTF-8"));
    result
}

fn encode_base62<const N: usize>(mut value: u128) -> [u8; N] {
    let mut buf = [b'0'; N];
    for i in (0..N).rev() {
        buf[i] = BASE62_CHARS[(value % 62) as usize];
        value /= 62;
    }
    buf
}

fn decode_base62(encoded: &[u8]) -> u128 {
    let mut value: u128 = 0;

    for &byte in encoded {
        let digit = BASE62_DECODE[byte as usize & 0x7F];

        if digit == 0xFF {
            log::warn!("Invalid base62 character: {byte}");
            return 0;
        }
        value = value * 62 + digit as u128;
    }
    value
}

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    let mut n: u32 = 0;

    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u32;
    }
    Some(n)
}

fn pack_o_format(id_str: &str) -> Option<(u128, bool)> {
    let b = id_str.as_bytes();

    if b.first() != Some(&b'O') {
        return None;
    }

    let (year, month, day, hour, minute, second, trader, strategy, count, has_hyphens) =
        if b.get(1) == Some(&b'-') {
            // With hyphens: O-YYYYMMDD-HHMMSS-TTT-SSS-CCC
            // Find hyphen positions manually to avoid Vec allocation
            if b.len() < 23 || b[10] != b'-' || b[17] != b'-' {
                return None;
            }
            let h4 = memchr_byte(b'-', &b[18..])?;
            let trader_end = 18 + h4;
            let h5 = memchr_byte(b'-', &b[trader_end + 1..])?;
            let strategy_end = trader_end + 1 + h5;

            (
                parse_digits(&b[2..6])?,
                parse_digits(&b[6..8])?,
                parse_digits(&b[8..10])?,
                parse_digits(&b[11..13])?,
                parse_digits(&b[13..15])?,
                parse_digits(&b[15..17])?,
                parse_digits(&b[18..trader_end])?,
                parse_digits(&b[trader_end + 1..strategy_end])?,
                parse_digits(&b[strategy_end + 1..])?,
                true,
            )
        } else {
            // Without hyphens: OYYYYMMDDHHMMSSTTTSSSCC...
            if b.len() < 22 {
                return None;
            }
            (
                parse_digits(&b[1..5])?,
                parse_digits(&b[5..7])?,
                parse_digits(&b[7..9])?,
                parse_digits(&b[9..11])?,
                parse_digits(&b[11..13])?,
                parse_digits(&b[13..15])?,
                parse_digits(&b[15..18])?,
                parse_digits(&b[18..21])?,
                parse_digits(&b[21..])?,
                false,
            )
        };

    if trader > 1023 || strategy > 1023 || count > 0xF_FFFF {
        return None;
    }

    let secs_since_epoch = civil_to_epoch(year, month, day, hour, minute, second)? - O_FORMAT_EPOCH;

    if secs_since_epoch < 0 {
        return None;
    }

    let packed = (secs_since_epoch as u128) << 40
        | (trader as u128) << 30
        | (strategy as u128) << 20
        | (count as u128);

    Some((packed, has_hyphens))
}

fn unpack_o_format(data: &str, has_hyphens: bool) -> String {
    let packed = decode_base62(data.as_bytes());

    let count = (packed & 0xF_FFFF) as u32;
    let strategy = ((packed >> 20) & 0x3FF) as u32;
    let trader = ((packed >> 30) & 0x3FF) as u32;
    let secs_since_epoch = (packed >> 40) as i64;

    let timestamp = secs_since_epoch + O_FORMAT_EPOCH;
    let Some((year, month, day, hour, minute, second)) = epoch_to_civil(timestamp) else {
        log::warn!("Failed to decode O-format timestamp: {timestamp}");
        return format!("DECODE_ERROR_{packed}");
    };

    if has_hyphens {
        format!(
            "O-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{trader:03}-{strategy:03}-{count}",
        )
    } else {
        format!(
            "O{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}{trader:03}{strategy:03}{count}",
        )
    }
}

fn parse_uuid_hex(id_str: &str) -> Option<(u128, bool)> {
    let b = id_str.as_bytes();

    if b.len() == 36 && b[8] == b'-' {
        // UUID with hyphens: 8-4-4-4-12
        if b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
            return None;
        }
        let mut value: u128 = 0;

        for &byte in b {
            if byte == b'-' {
                continue;
            }
            let nibble = hex_digit(byte)?;
            value = (value << 4) | nibble as u128;
        }
        Some((value, true))
    } else if b.len() == 32 {
        let mut value: u128 = 0;

        for &byte in b {
            let nibble = hex_digit(byte)?;
            value = (value << 4) | nibble as u128;
        }
        Some((value, false))
    } else {
        None
    }
}

fn format_uuid(data: &str, has_hyphens: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let value = decode_base62(data.as_bytes());
    let bytes = value.to_be_bytes();

    if has_hyphens {
        let mut buf = [0u8; 36];
        let mut pos = 0;

        for (i, &b) in bytes.iter().enumerate() {
            if i == 4 || i == 6 || i == 8 || i == 10 {
                buf[pos] = b'-';
                pos += 1;
            }
            buf[pos] = HEX[(b >> 4) as usize];
            buf[pos + 1] = HEX[(b & 0x0F) as usize];
            pos += 2;
        }
        std::str::from_utf8(&buf)
            .expect("hex is valid UTF-8")
            .to_string()
    } else {
        let mut buf = [0u8; 32];
        for (i, &b) in bytes.iter().enumerate() {
            buf[i * 2] = HEX[(b >> 4) as usize];
            buf[i * 2 + 1] = HEX[(b & 0x0F) as usize];
        }
        std::str::from_utf8(&buf)
            .expect("hex is valid UTF-8")
            .to_string()
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn memchr_byte(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Converts civil date/time to Unix timestamp (seconds since 1970-01-01).
fn civil_to_epoch(year: u32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    // Days from civil date using the algorithm from Howard Hinnant
    let y = if month <= 2 {
        year as i64 - 1
    } else {
        year as i64
    };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u64;
    let m = if month > 2 { month - 3 } else { month + 9 } as u64;
    let doy = (153 * m + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;
    Some(days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64)
}

/// Converts Unix timestamp to civil date/time components.
fn epoch_to_civil(timestamp: i64) -> Option<(u32, u32, u32, u32, u32, u32)> {
    if timestamp < 0 {
        return None;
    }
    let secs_of_day = (timestamp % 86400) as u32;
    let days = timestamp / 86400;

    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Civil date from day count using Howard Hinnant's algorithm
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y } as u32;

    Some((year, month, day, hour, minute, second))
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use rstest::rstest;

    use super::{super::consts::BINANCE_NAUTILUS_SPOT_BROKER_ID, *};

    const TEST_BROKER_ID: &str = BINANCE_NAUTILUS_SPOT_BROKER_ID;

    #[rstest]
    fn test_base62_roundtrip_zero() {
        let encoded = encode_base62::<13>(0);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, 0);
    }

    #[rstest]
    fn test_base62_roundtrip_max_72_bit() {
        let value: u128 = (1u128 << 72) - 1;
        let encoded = encode_base62::<13>(value);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, value);
    }

    #[rstest]
    fn test_base62_roundtrip_max_128_bit() {
        let value: u128 = u128::MAX;
        let encoded = encode_base62::<22>(value);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, value);
    }

    #[rstest]
    #[case("O-20200101-000000-000-000-0")]
    #[case("O-20200101-000001-001-001-1")]
    #[case("O-20260131-174827-001-001-1")]
    #[case("O-20260131-235959-999-999-4095")]
    #[case("O-20251215-123456-123-456-789")]
    #[case("O-20260305-120000-001-001-100")]
    #[case("O-20260305-120000-001-001-99999")]
    #[case("O-20260305-120000-001-001-1048575")]
    fn test_roundtrip_o_format_with_hyphens(#[case] id_str: &str) {
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert!(encoded.starts_with("x-TD67BGP9-T"), "got: {encoded}");
        assert!(encoded.len() <= 36, "len {} > 36: {encoded}", encoded.len());

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    #[case("O202001010000000000000")]
    #[case("O202601311748270010011")]
    #[case("O202601312359599999994095")]
    fn test_roundtrip_o_format_without_hyphens(#[case] id_str: &str) {
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert!(encoded.starts_with("x-TD67BGP9-t"), "got: {encoded}");
        assert!(encoded.len() <= 36);

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_roundtrip_uuid_with_hyphens() {
        let id_str = "550e8400-e29b-41d4-a716-446655440000";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert!(encoded.starts_with("x-TD67BGP9-U"), "got: {encoded}");
        assert!(encoded.len() <= 36, "len {} > 36: {encoded}", encoded.len());

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_roundtrip_uuid_without_hyphens() {
        let id_str = "550e8400e29b41d4a716446655440000";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert!(encoded.starts_with("x-TD67BGP9-u"), "got: {encoded}");
        assert!(encoded.len() <= 36);

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_roundtrip_uuid_all_zeros() {
        let id_str = "00000000-0000-0000-0000-000000000000";
        let coid = ClientOrderId::from(id_str);

        let decoded = decode_broker_id(&encode_broker_id(&coid, TEST_BROKER_ID), TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_roundtrip_uuid_all_f() {
        let id_str = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        let coid = ClientOrderId::from(id_str);

        let decoded = decode_broker_id(&encode_broker_id(&coid, TEST_BROKER_ID), TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_raw_passthrough_short_id() {
        let id_str = "my-order-123";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert!(encoded.starts_with("x-TD67BGP9-R"), "got: {encoded}");
        assert!(encoded.len() <= 36);

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_raw_passthrough_max_length() {
        let id_str = "abcdefghijklmnopqrstuvwx"; // 24 chars = max raw budget
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert_eq!(encoded.len(), 36);
        assert!(encoded.starts_with("x-TD67BGP9-R"));

        let decoded = decode_broker_id(&encoded, TEST_BROKER_ID);
        assert_eq!(decoded, id_str);
    }

    #[rstest]
    fn test_decode_non_prefixed_returns_as_is() {
        let raw = "O-20260131-174827-001-001-1";
        assert_eq!(decode_broker_id(raw, TEST_BROKER_ID), raw);
    }

    #[rstest]
    fn test_decode_different_prefix_returns_as_is() {
        let raw = "x-OTHERBROKER-T0000000000000";
        assert_eq!(decode_broker_id(raw, TEST_BROKER_ID), raw);
    }

    #[rstest]
    fn test_o_format_trader_overflow_sends_without_prefix() {
        // trader=1024 exceeds 10-bit limit, and hyphenated O-format (28 chars)
        // exceeds raw budget too, so the ID is sent without prefix
        let id_str = "O-20260131-174827-1024-001-1";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);
        assert_eq!(encoded, id_str);
    }

    #[rstest]
    fn test_o_format_count_overflow_sends_without_prefix() {
        // count=1048576 exceeds 20-bit limit, and hyphenated O-format (32 chars)
        // exceeds raw budget too, so the ID is sent without prefix
        let id_str = "O-20260131-174827-001-001-1048576";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);
        assert_eq!(encoded, id_str);
    }

    #[rstest]
    fn test_too_long_id_sends_without_prefix() {
        let id_str = "this-is-a-very-long-order-id-that-exceeds-everything";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);

        assert_eq!(encoded, id_str);
    }

    #[rstest]
    fn test_o_format_always_25_chars() {
        let test_cases = [
            "O-20200101-000000-000-000-0",
            "O-20260131-235959-999-999-4095",
            "O-20260305-120000-001-001-1048575",
        ];

        for id_str in test_cases {
            let coid = ClientOrderId::from(id_str);
            let encoded = encode_broker_id(&coid, TEST_BROKER_ID);
            assert_eq!(
                encoded.len(),
                25,
                "got {} for {id_str}: {encoded}",
                encoded.len()
            );
        }
    }

    #[rstest]
    fn test_uuid_always_34_chars() {
        let id_str = "550e8400-e29b-41d4-a716-446655440000";
        let coid = ClientOrderId::from(id_str);
        let encoded = encode_broker_id(&coid, TEST_BROKER_ID);
        assert_eq!(encoded.len(), 34, "got {}", encoded.len());
    }

    #[rstest]
    fn test_broker_prefix_format() {
        let prefix = broker_prefix(TEST_BROKER_ID);
        assert_eq!(prefix, "x-TD67BGP9-");
    }

    #[rstest]
    fn test_encoded_chars_are_binance_valid() {
        let valid = |c: char| {
            c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '/' || c == '_' || c == '-'
        };

        let ids = [
            "O-20260131-174827-001-001-1",
            "550e8400-e29b-41d4-a716-446655440000",
            "short-id",
        ];

        for id_str in ids {
            let coid = ClientOrderId::from(id_str);
            let encoded = encode_broker_id(&coid, TEST_BROKER_ID);
            assert!(
                encoded.chars().all(valid),
                "'{encoded}' contains invalid Binance characters"
            );
        }
    }

    #[rstest]
    fn test_civil_time_roundtrip() {
        let epoch = civil_to_epoch(2020, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(epoch, O_FORMAT_EPOCH);
        let (y, m, d, h, mi, s) = epoch_to_civil(epoch).unwrap();
        assert_eq!((y, m, d, h, mi, s), (2020, 1, 1, 0, 0, 0));
    }

    #[rstest]
    #[case("O-20260305-120000-001-001-1")]
    #[case("O-20260131-174827-001-001-1")]
    #[case("O-20260305-120000-001-001-1048575")]
    #[case("550e8400-e29b-41d4-a716-446655440000")]
    #[case("550e8400e29b41d4a716446655440000")]
    #[case("my-order-42")]
    #[case("short")]
    fn test_end_to_end_submit_and_receive(#[case] original_id: &str) {
        let broker_id = TEST_BROKER_ID;
        let client_order_id = ClientOrderId::from(original_id);

        // Simulate submit: encode the client order ID for Binance
        let encoded = encode_broker_id(&client_order_id, broker_id);
        assert!(encoded.len() <= 36, "encoded len {} > 36", encoded.len());

        // Simulate receive: Binance echoes the encoded ID back in a response
        let decoded = decode_broker_id(&encoded, broker_id);

        // Must recover the original ID exactly
        assert_eq!(decoded, original_id);
        assert_eq!(ClientOrderId::new(decoded), client_order_id);
    }

    #[rstest]
    fn bench_encode_decode_timing() {
        let o_coid = ClientOrderId::from("O-20260305-120000-001-001-100");
        let uuid_coid = ClientOrderId::from("550e8400-e29b-41d4-a716-446655440000");
        let raw_coid = ClientOrderId::from("my-order-123");

        let iterations = 100_000;

        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(encode_broker_id(black_box(&o_coid), TEST_BROKER_ID));
        }
        let encode_o = start.elapsed();

        let o_encoded = encode_broker_id(&o_coid, TEST_BROKER_ID);
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(decode_broker_id(black_box(&o_encoded), TEST_BROKER_ID));
        }
        let decode_o = start.elapsed();

        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(encode_broker_id(black_box(&uuid_coid), TEST_BROKER_ID));
        }
        let encode_uuid = start.elapsed();

        let uuid_encoded = encode_broker_id(&uuid_coid, TEST_BROKER_ID);
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(decode_broker_id(black_box(&uuid_encoded), TEST_BROKER_ID));
        }
        let decode_uuid = start.elapsed();

        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(encode_broker_id(black_box(&raw_coid), TEST_BROKER_ID));
        }
        let encode_raw = start.elapsed();

        let raw_encoded = encode_broker_id(&raw_coid, TEST_BROKER_ID);
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(decode_broker_id(black_box(&raw_encoded), TEST_BROKER_ID));
        }
        let decode_raw = start.elapsed();

        let passthrough = "O-20260305-120000-001-001-100";
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            black_box(decode_broker_id(black_box(passthrough), TEST_BROKER_ID));
        }
        let decode_pass = start.elapsed();

        println!("\n--- Broker ID Encoder Performance ({iterations} iterations) ---");
        println!(
            "encode O-format:     {:>8.1} ns/op",
            encode_o.as_nanos() as f64 / iterations as f64
        );
        println!(
            "decode O-format:     {:>8.1} ns/op",
            decode_o.as_nanos() as f64 / iterations as f64
        );
        println!(
            "encode UUID:         {:>8.1} ns/op",
            encode_uuid.as_nanos() as f64 / iterations as f64
        );
        println!(
            "decode UUID:         {:>8.1} ns/op",
            decode_uuid.as_nanos() as f64 / iterations as f64
        );
        println!(
            "encode raw:          {:>8.1} ns/op",
            encode_raw.as_nanos() as f64 / iterations as f64
        );
        println!(
            "decode raw:          {:>8.1} ns/op",
            decode_raw.as_nanos() as f64 / iterations as f64
        );
        println!(
            "decode passthrough:  {:>8.1} ns/op",
            decode_pass.as_nanos() as f64 / iterations as f64
        );
    }
}
