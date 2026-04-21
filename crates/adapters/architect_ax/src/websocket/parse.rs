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
    messages::{AxMdErrorResponse, AxMdMessage, AxOrdersWsFrame, AxWsOrderResponse},
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

/// Parses a raw JSON string into an [`AxOrdersWsFrame`].
///
/// Events (most frequent) get a fast byte-scan to detect the `"t"` field
/// and dispatch directly to `AxWsOrderEvent` (internally tagged).
/// Responses and errors (infrequent) use a single `Value` parse with
/// field inspection, avoiding the sequential-try overhead of `untagged`.
pub(crate) fn parse_order_message(raw: &str) -> Result<AxOrdersWsFrame, serde_json::Error> {
    // Fast path: event messages start with {"t":"
    if has_type_tag_prefix(raw.as_bytes()) {
        return serde_json::from_str(raw).map(|e| AxOrdersWsFrame::Event(Box::new(e)));
    }

    // Slow path: responses and errors (infrequent, use Value dispatch)
    let value: serde_json::Value = serde_json::from_str(raw)?;

    if value.get("err").is_some() {
        return serde_json::from_value(value).map(AxOrdersWsFrame::Error);
    }

    if let Some(res) = value.get("res") {
        if res.is_array() {
            return serde_json::from_value(value)
                .map(|r| AxOrdersWsFrame::Response(AxWsOrderResponse::OpenOrders(r)));
        }

        if res.get("oid").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxOrdersWsFrame::Response(AxWsOrderResponse::PlaceOrder(r)));
        }

        if res.get("cxl_rx").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxOrdersWsFrame::Response(AxWsOrderResponse::CancelOrder(r)));
        }

        if res.get("li").is_some() {
            return serde_json::from_value(value)
                .map(|r| AxOrdersWsFrame::Response(AxWsOrderResponse::List(r)));
        }

        return Err(serde_json::Error::custom(
            "unrecognized order response shape",
        ));
    }

    // Fallback: may be an event with "t" not at position 0
    if value.get("t").is_some() {
        return serde_json::from_value(value).map(|e| AxOrdersWsFrame::Event(Box::new(e)));
    }

    Err(serde_json::Error::custom(
        "order WS message has no 't', 'err', or 'res' field",
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::websocket::messages::{AxMdMessage, AxOrdersWsFrame, AxWsOrderResponse};

    #[rstest]
    fn test_parse_md_message_unknown_tag_errors() {
        let raw = r#"{"t":"X","s":"EURUSD-PERP"}"#;
        let err = parse_md_message(raw).expect_err("unknown tag should error");
        assert!(err.to_string().contains("unknown MD message type tag"));
    }

    #[rstest]
    fn test_parse_md_message_slow_path_subscription_response() {
        let raw = r#"{"rid":1,"result":{"subscribed":"EURUSD-PERP"}}"#;
        let msg = parse_md_message(raw).expect("should parse subscription response");
        assert!(matches!(msg, AxMdMessage::SubscriptionResponse(_)));
    }

    #[rstest]
    fn test_parse_md_message_slow_path_error_response() {
        let raw = r#"{"rid":2,"error":{"code":400,"message":"bad"}}"#;
        let msg = parse_md_message(raw).expect("should parse error response");
        match msg {
            AxMdMessage::Error(err) => {
                assert_eq!(err.message, "bad");
                assert_eq!(err.request_id, Some(2));
            }
            other => panic!("expected Error variant, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_md_message_no_recognized_fields_errors() {
        let raw = r#"{"foo":"bar"}"#;
        let err = parse_md_message(raw).expect_err("should reject unknown shape");
        assert!(
            err.to_string()
                .contains("no 't', 'result', or 'error' field")
        );
    }

    #[rstest]
    fn test_parse_md_message_malformed_json_errors() {
        let raw = "not json";
        assert!(parse_md_message(raw).is_err());
    }

    #[rstest]
    fn test_parse_order_message_unrecognized_res_errors() {
        let raw = r#"{"rid":1,"res":{"foo":"bar"}}"#;
        let err = parse_order_message(raw).expect_err("unrecognized res shape should error");
        assert!(
            err.to_string()
                .contains("unrecognized order response shape")
        );
    }

    #[rstest]
    fn test_parse_order_message_no_recognized_fields_errors() {
        let raw = r#"{"foo":"bar"}"#;
        let err = parse_order_message(raw).expect_err("unknown shape should error");
        assert!(err.to_string().contains("no 't', 'err', or 'res' field"));
    }

    #[rstest]
    fn test_parse_order_message_malformed_json_errors() {
        let raw = "not json";
        assert!(parse_order_message(raw).is_err());
    }

    #[rstest]
    fn test_parse_order_message_list_response_with_orders() {
        let raw = r#"{"rid":0,"res":{"li":"01KCQM-4WP1-0000","o":[]}}"#;
        let msg = parse_order_message(raw).expect("should parse list response");
        assert!(matches!(
            msg,
            AxOrdersWsFrame::Response(AxWsOrderResponse::List(_))
        ));
    }
}
