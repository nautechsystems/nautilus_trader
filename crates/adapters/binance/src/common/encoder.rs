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
//! The Binance broker ID is automatically prefixed to supported system-generated
//! client order IDs placed through the Binance adapter. Prefixing is transparent
//! to strategies and requires no user configuration.
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
//! [Link and Trade]: https://www.binance.com/en/support/faq/detail/a78a065d0c4846aaa1af474d8e712ab9
//!
//! # Wire format
//!
//! ```text
//! Legacy:    x-TD67BGP9-{signal}{base62_payload}
//! Short tag: x-TD67BGP9A{base62_payload}
//! ```
//!
//! The prefix `x-{BROKER_ID}-` is 11 chars (for an 8-char broker ID), leaving
//! 25 chars for the encoded component. Spot and Futures use separate broker
//! IDs defined in [`consts`](super::consts). The supplementary `A` format omits
//! the separator after the broker ID and occupies 24 characters in total.
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
//! | `A`    | O-format with a short tag  | 13 base62      | 24    |
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
//! # Short-tag O-format packing (77 bits -> 13 base62 chars)
//!
//! The `A` format preserves hyphenated factory IDs such as
//! `O-20260922-160119-V2-000-8`. One tag contains one or two ASCII alphanumeric
//! characters; the other is a canonical numeric tag in `[0, 1023]`, padded to
//! at least three digits. The count is in `[0, 4194303]` without leading zeros.
//! Timestamps cover `[2020-01-01 00:00:00, 2156-02-07 06:28:15]` UTC.
//!
//! ```text
//! bits [76:45] (32 bits): seconds since 2020-01-01 epoch
//! bit  [44]    (1 bit):  short tag is the strategy tag
//! bits [43:32] (12 bits): one/two-character base62 tag, with length
//! bits [31:22] (10 bits): numeric tag
//! bits [21:0]  (22 bits): count
//! ```
//!
//! Exact reconstruction preserves tag spelling and rejects normalized dates.
//! Existing numeric, UUID, and raw encodings take precedence and retain their
//! wire representation. Historical unprefixed fallback outputs contain at least
//! 25 characters, so the 24-character supplementary format cannot be mistaken
//! for any historical encoder output. A custom ID resembling this format is
//! short enough to use the existing `R` wrapper.
//!
//! IDs outside the supported compact formats and raw prefix budget are sent
//! unchanged with a warning; Binance's length and character limits still apply.
//!
//! # UUID packing (128 bits -> 22 base62 chars)
//!
//! The UUID is parsed from hex into a 128-bit integer and base62-encoded.
//!
//! # Decoding
//!
//! The decoder recognizes the 24-character `x-{BROKER_ID}A` format first,
//! then the legacy `x-{BROKER_ID}-` signal formats. Other strings pass through
//! unchanged, including historical unprefixed IDs. This keeps a longer custom
//! ID starting with `x-{BROKER_ID}A` distinct from the short-tag format.
//!
//! Existing-order operations without a venue order ID query both supplementary
//! and historical unprefixed identities. Different matching venue IDs are
//! rejected as ambiguous before modification or cancellation.
//!
//! # Performance
//!
//! Base62 encoding uses a stack buffer and ten-digit chunks to limit wide
//! integer division. Prefix construction writes directly into the result string.
//! The `bench_encode_decode_timing` test provides a local timing check.
//! Existing-order compatibility resolution may add two HTTP lookups.
//!
//! An isolated codec comparison compiled with `opt-level=3` measures the following
//! median nanoseconds per operation over 25 interleaved batches of 100,000 calls.
//! Timings are machine-dependent and exclude HTTP requests.
//!
//! | Format                   | Encode before | Encode after | Decode before | Decode after |
//! | ------------------------ | ------------- | ------------ | ------------- | ------------ |
//! | O-format with hyphens    | 102           | 44           | 201           | 124          |
//! | O-format without hyphens | 99            | 41           | 192           | 116          |
//! | UUID with hyphens        | 243           | 60           | 72            | 47           |
//! | UUID without hyphens     | 240           | 57           | 67            | 43           |
//! | Raw prefixed ID          | 34            | 16           | 39            | 21           |
//! | Short-tag O-format       | N/A           | 174          | N/A           | 117          |
//!
//! Short-tag IDs previously used unprefixed passthrough (37 ns encode, 38 ns decode),
//! which performs no compression and is not equivalent to short-tag encoding.

use std::fmt::Write;

use anyhow::Context;
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
const SIGNAL_O_TAGS: u8 = b'A';
const O_TAGS_B62_LEN: usize = 13;
const O_TAGS_COUNT_BITS: u32 = 22;
const O_TAGS_COUNT_MAX: u32 = (1 << O_TAGS_COUNT_BITS) - 1;

/// Encodes a `ClientOrderId` into a Binance-compatible string with broker ID
/// prefix.
///
/// The encoding is deterministic and reversible with [`decode_broker_id`].
#[must_use]
pub fn encode_broker_id(client_order_id: &ClientOrderId, broker_id: &str) -> String {
    let id_str = client_order_id.as_str();
    let budget = MAX_CLIENT_ORDER_ID_LEN - (3 + broker_id.len());

    if let Some((packed, has_hyphens)) = pack_o_format(id_str) {
        let signal = if has_hyphens {
            SIGNAL_O_HYPHENS
        } else {
            SIGNAL_O_NO_HYPHENS
        };
        let b62 = encode_base62::<O_FORMAT_B62_LEN>(packed);
        return build_encoded(broker_id, signal, &b62);
    }

    if let Some((value, has_hyphens)) = parse_uuid_hex(id_str) {
        let signal = if has_hyphens {
            SIGNAL_UUID_HYPHENS
        } else {
            SIGNAL_UUID_NO_HYPHENS
        };
        let b62 = encode_base62::<UUID_B62_LEN>(value);
        return build_encoded(broker_id, signal, &b62);
    }

    if id_str.len() < budget {
        return build_encoded(broker_id, SIGNAL_RAW, id_str.as_bytes());
    }

    // Keep the new format shorter than any historical passthrough ID
    if 3 + broker_id.len() + O_TAGS_B62_LEN < budget
        && let Some(packed) = pack_o_tags(id_str)
    {
        return build_encoded(
            broker_id,
            SIGNAL_O_TAGS,
            &encode_base62::<O_TAGS_B62_LEN>(packed),
        );
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
    match decode_broker_id_checked(encoded, broker_id) {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("Failed to decode broker client order ID: {e}");
            encoded.to_string()
        }
    }
}

/// Decodes and validates an inbound Binance client order ID.
///
/// Strings without the expected broker prefix are treated as legacy IDs and
/// validated without decoding.
///
/// # Errors
///
/// Returns an error if the broker-prefixed encoding is malformed or the
/// decoded client order ID is invalid.
pub(crate) fn decode_client_order_id(
    encoded: &str,
    broker_id: &str,
) -> anyhow::Result<ClientOrderId> {
    let decoded = decode_broker_id_checked(encoded, broker_id)?;
    ClientOrderId::new_checked(decoded)
        .with_context(|| format!("invalid Binance client order ID '{encoded}'"))
}

fn decode_broker_id_checked(encoded: &str, broker_id: &str) -> anyhow::Result<String> {
    if let Some(data) = tagged_payload(encoded, broker_id) {
        let packed =
            decode_base62(data).context("invalid tagged O-format broker client order ID")?;
        return unpack_o_tags(packed);
    }

    let Some(payload) = encoded
        .strip_prefix("x-")
        .and_then(|s| s.strip_prefix(broker_id))
        .and_then(|s| s.strip_prefix('-'))
    else {
        return Ok(encoded.to_string());
    };

    let Some((&signal, data)) = payload.as_bytes().split_first() else {
        anyhow::bail!("missing broker client order ID signal");
    };

    match signal {
        SIGNAL_O_HYPHENS | SIGNAL_O_NO_HYPHENS => {
            anyhow::ensure!(
                data.len() == O_FORMAT_B62_LEN,
                "invalid O-format broker client order ID payload length"
            );
            let packed = decode_base62(data).context("invalid O-format broker client order ID")?;
            Ok(unpack_o_format(packed, signal == SIGNAL_O_HYPHENS))
        }
        SIGNAL_UUID_HYPHENS | SIGNAL_UUID_NO_HYPHENS => {
            anyhow::ensure!(
                data.len() == UUID_B62_LEN,
                "invalid UUID broker client order ID payload length"
            );
            let value = decode_base62(data).context("invalid UUID broker client order ID")?;
            Ok(format_uuid(value, signal == SIGNAL_UUID_HYPHENS))
        }
        SIGNAL_RAW => {
            let raw = std::str::from_utf8(data).context("invalid raw broker client order ID")?;
            anyhow::ensure!(
                !raw.is_empty(),
                "missing raw broker client order ID payload"
            );
            Ok(raw.to_string())
        }
        _ => anyhow::bail!(
            "unknown broker client order ID signal byte '{}'",
            signal as char
        ),
    }
}

pub(crate) fn legacy_client_order_id(encoded: &str, broker_id: &str) -> Option<String> {
    let packed = decode_base62(tagged_payload(encoded, broker_id)?)?;
    unpack_o_tags(packed).ok()
}

fn tagged_payload<'a>(encoded: &'a str, broker_id: &str) -> Option<&'a [u8]> {
    if encoded.len() != 3 + broker_id.len() + O_TAGS_B62_LEN
        || encoded.len() >= MAX_CLIENT_ORDER_ID_LEN - (3 + broker_id.len())
    {
        return None;
    }

    let payload = encoded.strip_prefix("x-")?.strip_prefix(broker_id)?;
    let (&signal, data) = payload.as_bytes().split_first()?;
    (signal == SIGNAL_O_TAGS).then_some(data)
}

fn build_encoded(broker_id: &str, signal: u8, payload: &[u8]) -> String {
    let tagged = signal == SIGNAL_O_TAGS;
    let mut result =
        String::with_capacity(3 + broker_id.len() + usize::from(!tagged) + payload.len());
    result.push_str("x-");
    result.push_str(broker_id);

    if !tagged {
        result.push('-');
    }

    result.push(signal as char);
    result.push_str(std::str::from_utf8(payload).expect("encoded ID is valid UTF-8"));
    result
}

fn encode_base62<const N: usize>(mut value: u128) -> [u8; N] {
    const DIGITS: usize = 10;
    const RADIX: u128 = 62u128.pow(DIGITS as u32);
    let mut buf = [b'0'; N];
    for chunk in buf.rchunks_mut(DIGITS) {
        let mut part = (value % RADIX) as u64;
        value /= RADIX;

        for byte in chunk.iter_mut().rev() {
            *byte = BASE62_CHARS[(part % 62) as usize];
            part /= 62;
        }
    }
    buf
}

fn decode_base62(encoded: &[u8]) -> Option<u128> {
    let mut value: u128 = 0;

    for &byte in encoded {
        let digit = BASE62_DECODE[byte as usize & 0x7F];

        if digit == 0xFF || !byte.is_ascii() {
            return None;
        }
        value = value.checked_mul(62)?.checked_add(digit as u128)?;
    }
    Some(value)
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

fn unpack_o_format(packed: u128, has_hyphens: bool) -> String {
    let count = (packed & 0xF_FFFF) as u32;
    let strategy = ((packed >> 20) & 0x3FF) as u32;
    let trader = ((packed >> 30) & 0x3FF) as u32;
    let secs_since_epoch = (packed >> 40) as i64;

    let timestamp = secs_since_epoch + O_FORMAT_EPOCH;
    let Some((year, month, day, hour, minute, second)) = epoch_to_civil(timestamp) else {
        log::warn!("Failed to decode O-format timestamp: {timestamp}");
        return format!("DECODE_ERROR_{packed}");
    };

    let mut result = String::with_capacity(MAX_CLIENT_ORDER_ID_LEN);

    if has_hyphens {
        write!(result, "O-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{trader:03}-{strategy:03}-{count}")
            .expect("writing to String should not fail");
    } else {
        write!(result, "O{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}{trader:03}{strategy:03}{count}")
            .expect("writing to String should not fail");
    }

    result
}

fn pack_o_tags(id: &str) -> Option<u128> {
    let mut parts = id.split('-');
    if parts.next()? != "O" {
        return None;
    }

    let date = parts.next()?.as_bytes();
    let time = parts.next()?.as_bytes();
    let trader = parts.next()?;
    let strategy = parts.next()?;
    let count = parts.next()?;
    if parts.next().is_some() || date.len() != 8 || time.len() != 6 {
        return None;
    }

    let epoch = civil_to_epoch(
        parse_digits(&date[..4])?,
        parse_digits(&date[4..6])?,
        parse_digits(&date[6..])?,
        parse_digits(&time[..2])?,
        parse_digits(&time[2..4])?,
        parse_digits(&time[4..])?,
    )?;
    let seconds = u32::try_from(epoch - O_FORMAT_EPOCH).ok()?;
    let count = count.parse::<u32>().ok()?;
    if count > O_TAGS_COUNT_MAX {
        return None;
    }

    for swapped in [false, true] {
        let (tag, numeric) = if swapped {
            (strategy, trader)
        } else {
            (trader, strategy)
        };

        let Some(tag) = pack_tag(tag) else {
            continue;
        };

        let Ok(numeric) = numeric.parse::<u32>() else {
            continue;
        };

        if numeric > 1023 {
            continue;
        }

        let packed = ((seconds as u128) << 45)
            | ((swapped as u128) << 44)
            | ((tag as u128) << 32)
            | ((numeric as u128) << O_TAGS_COUNT_BITS)
            | count as u128;

        if unpack_o_tags(packed).ok()?.as_str() == id {
            return Some(packed);
        }
    }

    None
}

fn unpack_o_tags(packed: u128) -> anyhow::Result<String> {
    anyhow::ensure!(
        packed >> 77 == 0,
        "tagged O-format payload exceeds its bit budget"
    );
    let count = (packed & O_TAGS_COUNT_MAX as u128) as u32;
    let numeric = ((packed >> O_TAGS_COUNT_BITS) & 0x3FF) as u32;
    let tag = unpack_tag(((packed >> 32) & 0xFFF) as u32)?;
    let swapped = ((packed >> 44) & 1) != 0;
    let (year, month, day, hour, minute, second) =
        epoch_to_civil((packed >> 45) as i64 + O_FORMAT_EPOCH)
            .context("invalid tagged O-format timestamp")?;
    let mut result = String::with_capacity(MAX_CLIENT_ORDER_ID_LEN);
    write!(
        result,
        "O-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-"
    )
    .expect("writing to String should not fail");

    if swapped {
        write!(result, "{numeric:03}-{tag}-{count}").expect("writing to String should not fail");
    } else {
        write!(result, "{tag}-{numeric:03}-{count}").expect("writing to String should not fail");
    }

    Ok(result)
}

fn pack_tag(tag: &str) -> Option<u32> {
    if !(1..=2).contains(&tag.len()) {
        return None;
    }

    let mut value = 0;

    for byte in tag.bytes() {
        let digit = *BASE62_DECODE.get(byte as usize)?;
        if digit == 0xFF {
            return None;
        }

        value = value * 62 + u32::from(digit);
    }

    match tag.len() {
        1 => Some(value + 1),
        2 => Some(value + 63),
        _ => None,
    }
}

fn unpack_tag(value: u32) -> anyhow::Result<String> {
    anyhow::ensure!((1..=3906).contains(&value), "invalid tagged O-format tag");
    let mut tag = String::with_capacity(2);

    if value <= 62 {
        tag.push(BASE62_CHARS[(value - 1) as usize] as char);
    } else {
        let value = value - 63;
        tag.push(BASE62_CHARS[(value / 62) as usize] as char);
        tag.push(BASE62_CHARS[(value % 62) as usize] as char);
    }

    Ok(tag)
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

fn format_uuid(value: u128, has_hyphens: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
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
    #[case("O-20260922-160119-V2-000-8")]
    #[case("O-20260922-200130-V2-001-24")]
    #[case("O-20260923-000138-V2-001-1")]
    #[case("O-20260923-000138-V2-001-4194303")]
    #[case("O-20260923-000138-001-aZ-42")]
    fn test_tagged_id_preserves_exact_identity(#[case] original: &str) {
        let id = ClientOrderId::from(original);
        let encoded = encode_broker_id(&id, TEST_BROKER_ID);
        assert_eq!(encoded.len(), 24);
        assert!(encoded.starts_with("x-TD67BGP9A"));
        assert_eq!(
            decode_client_order_id(&encoded, TEST_BROKER_ID).unwrap(),
            id
        );

        // Historical custom IDs matching this format were R-wrapped
        let custom = ClientOrderId::from(encoded.as_str());
        let wrapped = encode_broker_id(&custom, TEST_BROKER_ID);
        assert_eq!(wrapped, format!("x-TD67BGP9-R{encoded}"));
        assert_eq!(
            decode_client_order_id(&wrapped, TEST_BROKER_ID).unwrap(),
            custom
        );
    }

    #[rstest]
    #[case("O-20260923-000138-V2-001-4194304")]
    #[case("O-20260923-000138-LONG-001-1")]
    #[case("O-20260923-000138-V2-001-01")]
    #[case("O-20260230-000138-V2-001-1")]
    fn test_unsupported_tagged_id_remains_unchanged(#[case] original: &str) {
        assert_eq!(
            encode_broker_id(&ClientOrderId::from(original), TEST_BROKER_ID),
            original
        );
    }

    #[rstest]
    fn test_historical_raw_id_does_not_enter_short_namespace() {
        let original = "x-TD67BGP9A00000000000000";
        assert_eq!(original.len(), 25);
        assert_eq!(
            encode_broker_id(&ClientOrderId::from(original), TEST_BROKER_ID),
            original
        );
        assert_eq!(
            decode_client_order_id(original, TEST_BROKER_ID)
                .unwrap()
                .as_str(),
            original
        );
    }

    #[rstest]
    #[case("O-20260305-120000-001-001-100", "x-TD67BGP9-T047IBdyLevrb2")]
    #[case("O20260305120000001001100", "x-TD67BGP9-t047IBdyLevrb2")]
    #[case(
        "550e8400-e29b-41d4-a716-446655440000",
        "x-TD67BGP9-U2aUyqjCzEIiEcYMKj7TZtw"
    )]
    #[case(
        "550e8400e29b41d4a716446655440000",
        "x-TD67BGP9-u2aUyqjCzEIiEcYMKj7TZtw"
    )]
    #[case("my-order-123", "x-TD67BGP9-Rmy-order-123")]
    fn test_historical_wire_vectors(#[case] original: &str, #[case] encoded: &str) {
        let id = ClientOrderId::from(original);
        assert_eq!(encode_broker_id(&id, TEST_BROKER_ID), encoded);
        assert_eq!(decode_client_order_id(encoded, TEST_BROKER_ID).unwrap(), id);
    }

    #[rstest]
    fn test_base62_chunked_encoding_matches_digit_reference() {
        let mut state = 0x1234_5678_90ab_cdef_2345_6789_abcd_ef01u128;

        for value in [0, 1, 61, 62, 62u128.pow(10) - 1, 62u128.pow(10), u128::MAX]
            .into_iter()
            .chain((0..10_000).map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state
            }))
        {
            let mut expected = [b'0'; 22];
            let mut remaining = value;

            for byte in expected.iter_mut().rev() {
                *byte = BASE62_CHARS[(remaining % 62) as usize];
                remaining /= 62;
            }

            assert_eq!(encode_base62::<22>(value), expected);
            assert_eq!(encode_base62::<13>(value).as_slice(), &expected[9..]);
            assert_eq!(decode_base62(&expected), Some(value));
        }
    }

    #[rstest]
    #[case("O-20200101-000000-0-000-0")]
    #[case("O-21560207-062815-zz-1023-4194303")]
    #[case("O-20260922-160119-000-00-1")]
    fn test_tagged_packing_boundaries(#[case] original: &str) {
        let packed = pack_o_tags(original).unwrap();
        assert_eq!(unpack_o_tags(packed).unwrap(), original);
    }

    #[rstest]
    fn test_base62_roundtrip_zero() {
        let encoded = encode_base62::<13>(0);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, Some(0));
    }

    #[rstest]
    fn test_base62_roundtrip_max_72_bit() {
        let value: u128 = (1u128 << 72) - 1;
        let encoded = encode_base62::<13>(value);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, Some(value));
    }

    #[rstest]
    fn test_base62_roundtrip_max_128_bit() {
        let value: u128 = u128::MAX;
        let encoded = encode_base62::<22>(value);
        let decoded = decode_base62(&encoded);
        assert_eq!(decoded, Some(value));
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
    #[case::empty("", "invalid Binance client order ID ''")]
    #[case::whitespace("   ", "invalid Binance client order ID '   '")]
    #[case::non_ascii("client-é", "invalid Binance client order ID 'client-é'")]
    #[case::missing_signal("x-TD67BGP9-", "missing broker client order ID signal")]
    #[case::missing_raw_payload("x-TD67BGP9-R", "missing raw broker client order ID payload")]
    #[case::invalid_o_payload(
        "x-TD67BGP9-T000000000000!",
        "invalid O-format broker client order ID"
    )]
    #[case::unknown_signal(
        "x-TD67BGP9-Xlegacy-order",
        "unknown broker client order ID signal byte 'X'"
    )]
    fn test_decode_client_order_id_rejects_invalid_input(
        #[case] encoded: &str,
        #[case] expected: &str,
    ) {
        let error = decode_client_order_id(encoded, TEST_BROKER_ID).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_decode_client_order_id_preserves_valid_prefixed_id() {
        let original = ClientOrderId::from("O-20260305-120000-001-001-100");
        let encoded = encode_broker_id(&original, TEST_BROKER_ID);

        let decoded = decode_client_order_id(&encoded, TEST_BROKER_ID).unwrap();

        assert_eq!(decoded, original);
    }

    #[rstest]
    fn test_decode_client_order_id_preserves_valid_legacy_id() {
        let decoded = decode_client_order_id("legacy-order-1", TEST_BROKER_ID).unwrap();

        assert_eq!(decoded, ClientOrderId::from("legacy-order-1"));
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
        let encoded = encode_broker_id(&ClientOrderId::from("test"), TEST_BROKER_ID);
        assert_eq!(encoded, "x-TD67BGP9-Rtest");
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
