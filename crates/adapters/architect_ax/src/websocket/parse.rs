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

//! Fast JSON message parsers for Ax WebSocket streams.
//!
//! Both parsers use byte-level prefix scanning to identify the message type
//! tag before dispatching to the correct serde target struct, avoiding the
//! intermediate `serde_json::Value` allocation on the hot path.

use serde::de::Error;

use super::{
    error::AxWsErrorResponse,
    messages::{AxMdErrorResponse, AxMdMessage, AxWsOrderResponse, AxWsRawMessage},
};

#[inline]
fn peek_type_tag(bytes: &[u8]) -> Option<u8> {
    if bytes.len() > 7
        && bytes[0] == b'{'
        && bytes[1] == b'"'
        && bytes[2] == b't'
        && bytes[3] == b'"'
        && bytes[4] == b':'
        && bytes[5] == b'"'
        && bytes[7] == b'"'
    {
        Some(bytes[6])
    } else {
        None
    }
}

#[inline]
fn has_type_tag_prefix(bytes: &[u8]) -> bool {
    bytes.len() > 5 && bytes[0] == b'{' && bytes[1] == b'"' && bytes[2] == b't' && bytes[3] == b'"'
}

/// Parses a raw JSON string into an [`AxMdMessage`].
///
/// Uses a fast byte-scan to extract the type discriminator without
/// allocating an intermediate `serde_json::Value` tree, then dispatches
/// directly to the target struct deserializer.
///
/// # Errors
///
/// Returns an error if the JSON is malformed or has an unknown type tag.
pub fn parse_md_message(raw: &str) -> Result<AxMdMessage, serde_json::Error> {
    if let Some(tag) = peek_type_tag(raw.as_bytes()) {
        return match tag {
            b'1' => serde_json::from_str(raw).map(AxMdMessage::BookL1),
            b'2' => serde_json::from_str(raw).map(AxMdMessage::BookL2),
            b'3' => serde_json::from_str(raw).map(AxMdMessage::BookL3),
            b's' => serde_json::from_str(raw).map(AxMdMessage::Ticker),
            b't' => serde_json::from_str(raw).map(AxMdMessage::Trade),
            b'c' => serde_json::from_str(raw).map(AxMdMessage::Candle),
            b'h' => serde_json::from_str(raw).map(AxMdMessage::Heartbeat),
            b'e' => serde_json::from_str::<AxWsErrorResponse>(raw)
                .map(|resp| AxMdMessage::Error(resp.into())),
            tag => Err(serde_json::Error::custom(format!(
                "unknown MD message type tag: '{}'",
                tag as char
            ))),
        };
    }

    // Slow path: subscription responses and errors (no "t" field, rare)
    let value: serde_json::Value = serde_json::from_str(raw)?;

    if value.get("result").is_some() {
        return serde_json::from_value(value).map(AxMdMessage::SubscriptionResponse);
    }

    if value.get("error").is_some() {
        return serde_json::from_value::<AxMdErrorResponse>(value)
            .map(|resp| AxMdMessage::Error(resp.into()));
    }

    // Fallback: "t" exists but wasn't at position 0
    if let Some(t) = value.get("t").and_then(|v| v.as_str()) {
        match t {
            "1" => serde_json::from_value(value).map(AxMdMessage::BookL1),
            "2" => serde_json::from_value(value).map(AxMdMessage::BookL2),
            "3" => serde_json::from_value(value).map(AxMdMessage::BookL3),
            "s" => serde_json::from_value(value).map(AxMdMessage::Ticker),
            "t" => serde_json::from_value(value).map(AxMdMessage::Trade),
            "c" => serde_json::from_value(value).map(AxMdMessage::Candle),
            "h" => serde_json::from_value(value).map(AxMdMessage::Heartbeat),
            "e" => serde_json::from_value::<AxWsErrorResponse>(value)
                .map(|resp| AxMdMessage::Error(resp.into())),
            other => Err(serde_json::Error::custom(format!(
                "unknown MD message type: {other}"
            ))),
        }
    } else {
        Err(serde_json::Error::custom(
            "MD message has no 't', 'result', or 'error' field",
        ))
    }
}

/// Parses a raw JSON string into an [`AxWsRawMessage`].
///
/// Events (most frequent) get a fast byte-scan to detect the `"t"` field
/// and dispatch directly to `AxWsOrderEvent` (internally tagged).
/// Responses and errors (infrequent) use a single `Value` parse with
/// field inspection, avoiding the sequential-try overhead of `untagged`.
pub(crate) fn parse_order_message(raw: &str) -> Result<AxWsRawMessage, serde_json::Error> {
    // Fast path: event messages start with {"t":"
    if has_type_tag_prefix(raw.as_bytes()) {
        return serde_json::from_str(raw).map(|e| AxWsRawMessage::Event(Box::new(e)));
    }

    // Slow path: responses and errors (infrequent, use Value dispatch)
    let value: serde_json::Value = serde_json::from_str(raw)?;

    if value.get("err").is_some() {
        return serde_json::from_value(value).map(AxWsRawMessage::Error);
    }

    if let Some(res) = value.get("res") {
        if res.is_array() {
            return serde_json::from_value(value)
                .map(|r| AxWsRawMessage::Response(AxWsOrderResponse::OpenOrders(r)));
        }

        if res.get("oid").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxWsRawMessage::Response(AxWsOrderResponse::PlaceOrder(r)));
        }

        if res.get("cxl_rx").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxWsRawMessage::Response(AxWsOrderResponse::CancelOrder(r)));
        }

        if res.get("li").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxWsRawMessage::Response(AxWsOrderResponse::List(r)));
        }

        return Err(serde_json::Error::custom(
            "unrecognized order response shape",
        ));
    }

    // Fallback: may be an event with "t" not at position 0
    if value.get("t").is_some() {
        return serde_json::from_value(value).map(|e| AxWsRawMessage::Event(Box::new(e)));
    }

    Err(serde_json::Error::custom(
        "order WS message has no 't', 'err', or 'res' field",
    ))
}
