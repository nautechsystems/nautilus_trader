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
    messages::{
        AxMdErrorResponse, AxMdMessage, AxOrdersWsFrame, AxWsOrderEvent, AxWsOrderResponse,
    },
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

    if value
        .get("result")
        .or_else(|| value.get("res"))
        .is_some_and(|v| !v.is_null())
    {
        return serde_json::from_value(value).map(AxMdMessage::SubscriptionResponse);
    }

    if value
        .get("error")
        .or_else(|| value.get("err"))
        .is_some_and(|v| !v.is_null())
    {
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
        return parse_order_event(raw).map(|e| AxOrdersWsFrame::Event(Box::new(e)));
    }

    // Slow path: responses and errors (infrequent, use Value dispatch)
    let value: serde_json::Value = serde_json::from_str(raw)?;

    if value
        .get("err")
        .or_else(|| value.get("error"))
        .is_some_and(|v| !v.is_null())
    {
        return serde_json::from_value(value).map(AxOrdersWsFrame::Error);
    }

    if let Some(res) = value.get("res").or_else(|| value.get("result")) {
        if res.get("orders").is_some() {
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
        return parse_order_event(raw).map(|e| AxOrdersWsFrame::Event(Box::new(e)));
    }

    Err(serde_json::Error::custom(
        "order WS message has no 't', 'err', or 'res' field",
    ))
}

fn parse_order_event(raw: &str) -> Result<AxWsOrderEvent, serde_json::Error> {
    match serde_json::from_str(raw) {
        Ok(event) => Ok(event),
        Err(e) => {
            // Live orders WS sends undocumented `{"t":"pu"}` about every 2s.
            let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
                return Err(e);
            };

            if value.get("t").and_then(|v| v.as_str()) == Some("pu") {
                return Ok(AxWsOrderEvent::Heartbeat);
            }

            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::websocket::messages::{
        AxMdMessage, AxOrdersWsFrame, AxWsOrderEvent, AxWsOrderResponse,
    };

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
    fn test_parse_order_message_pu_keep_alive() {
        let msg = parse_order_message(r#"{"t":"pu"}"#).expect("should parse keep-alive");
        assert!(matches!(
            msg,
            AxOrdersWsFrame::Event(event) if matches!(*event, AxWsOrderEvent::Heartbeat)
        ));
    }

    #[rstest]
    fn test_parse_order_message_unknown_tag_errors() {
        let err = parse_order_message(r#"{"t":"zz"}"#).expect_err("unknown tag should error");
        assert!(err.to_string().contains("unknown variant `zz`"));
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

    #[rstest]
    fn test_parse_order_message_replaced_live_shape() {
        let raw = include_str!("../../test_data/ws_order_replaced_live.json");
        let msg = parse_order_message(raw).expect("should parse live replaced event");

        let AxOrdersWsFrame::Event(event) = msg else {
            panic!("expected Event frame");
        };
        let AxWsOrderEvent::Replaced(replaced) = *event else {
            panic!("expected Replaced event");
        };

        assert_eq!(
            replaced.no.as_ref().unwrap().oid,
            "O-01KWY01WX8JT4DABKC6FRS5NT4"
        );
        assert_eq!(replaced.no.as_ref().unwrap().rq, 100);
    }

    #[rstest]
    #[case::direct_open_orders_array(
        include_str!("../../test_data/ws_order_open_orders_response_invalid_direct_array.json"),
        "unrecognized order response shape",
    )]
    #[case::single_replacement_order(
        include_str!("../../test_data/ws_order_replaced_invalid_single_order.json"),
        "missing field `ro`",
    )]
    fn test_parse_order_message_rejects_obsolete_shapes(
        #[case] raw: &str,
        #[case] expected_error: &str,
    ) {
        let error = parse_order_message(raw).expect_err("obsolete shape should be rejected");
        assert!(
            error.to_string().contains(expected_error),
            "expected {expected_error:?} in {error}",
        );
    }

    #[rstest]
    #[case("rid", "result", "error")]
    #[case("request_id", "res", "err")]
    fn test_md_response_aliases(#[case] id: &str, #[case] result: &str, #[case] error: &str) {
        let mut success: serde_json::Value = serde_json::from_str(include_str!(
            "../../test_data/captured/ws-md-response-1.json"
        ))
        .unwrap();
        let mut failure: serde_json::Value = serde_json::from_str(include_str!(
            "../../test_data/captured/ws-md-response-6.json"
        ))
        .unwrap();

        for value in [&mut success, &mut failure] {
            let obj = value.as_object_mut().unwrap();
            let rid = obj.remove("rid").unwrap();
            obj.insert(id.into(), rid);
        }

        let res = success.as_object_mut().unwrap().remove("result").unwrap();
        success[result] = res;
        let err = failure.as_object_mut().unwrap().remove("error").unwrap();
        failure[error] = err;

        let AxMdMessage::SubscriptionResponse(response) =
            parse_md_message(&success.to_string()).unwrap()
        else {
            panic!("expected subscription response")
        };

        let AxMdMessage::Error(response_error) = parse_md_message(&failure.to_string()).unwrap()
        else {
            panic!("expected error response")
        };

        assert_eq!(response.rid, 1);
        assert_eq!(response_error.request_id, Some(6));
        assert_eq!(
            response_error.message,
            failure[error]["message"].as_str().unwrap()
        );
    }

    #[rstest]
    #[case("rid", "res", "err")]
    #[case("request_id", "result", "error")]
    fn test_orders_response_aliases_and_null_error(
        #[case] id: &str,
        #[case] result: &str,
        #[case] error: &str,
    ) {
        let mut value: serde_json::Value = serde_json::from_str(include_str!(
            "../../test_data/captured/ws-orders-response-0.json"
        ))
        .unwrap();
        let obj = value.as_object_mut().unwrap();
        let rid = obj.remove("rid").unwrap();
        let res = obj.remove("res").unwrap();
        obj.remove("err");
        obj.insert(id.into(), rid);
        obj.insert(result.into(), res);
        obj.insert(error.into(), serde_json::Value::Null);

        let AxOrdersWsFrame::Response(AxWsOrderResponse::List(response)) =
            parse_order_message(&value.to_string()).unwrap()
        else {
            panic!("expected login response")
        };

        assert_eq!(response.rid, 0);
        assert_eq!(response.res.li, value[result]["li"].as_str().unwrap());
        assert_eq!(response.res.cod, value[result]["cod"].as_bool());
        assert_eq!(response.res.chb, value[result]["chb"].as_u64());
    }

    #[rstest]
    fn test_in_place_amendment_has_no_replacement_identity() {
        let raw = include_str!("../../test_data/ws_order_amended.json");

        let AxOrdersWsFrame::Event(event) = parse_order_message(raw).unwrap() else {
            panic!("expected event")
        };

        let AxWsOrderEvent::Replaced(message) = *event else {
            panic!("expected amendment")
        };

        assert_eq!(message.noid, None);
        assert!(message.no.is_none());
        assert_eq!(message.ro.q, 150);
        assert_eq!(message.ro.rq, 150);
        assert_eq!(message.ro.o, crate::common::enums::AxOrderStatus::Accepted);
    }

    #[rstest]
    #[case(include_str!("../../test_data/captured/ws-md-1.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-2.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-3.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-c.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-h.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-response-1.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-response-2.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-response-3.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-response-4.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-response-6.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-s.json"))]
    #[case(include_str!("../../test_data/captured/ws-md-t.json"))]
    fn test_current_market_data_capture(#[case] raw: &str) {
        let value: serde_json::Value = serde_json::from_str(raw).unwrap();
        let parsed = parse_md_message(raw).unwrap();

        let (tag, ts) = match parsed {
            AxMdMessage::BookL1(book) => ("1", book.ts),
            AxMdMessage::BookL2(book) => {
                assert!(book.st);
                ("2", book.ts)
            }
            AxMdMessage::BookL3(book) => {
                assert!(book.st);
                ("3", book.ts)
            }
            AxMdMessage::Trade(trade) => ("t", trade.ts),
            AxMdMessage::Candle(candle) => ("c", candle.ts),
            AxMdMessage::Ticker(ticker) => ("s", ticker.ts),
            AxMdMessage::Heartbeat(heartbeat) => ("h", heartbeat.ts),
            AxMdMessage::SubscriptionResponse(response) => {
                assert_eq!(response.rid, value["rid"].as_i64().unwrap());
                assert_eq!(
                    response
                        .result
                        .subscribed
                        .as_deref()
                        .or(response.result.subscribed_candle.as_deref()),
                    value["result"]["subscribed"]
                        .as_str()
                        .or(value["result"]["subscribed_candle"].as_str())
                );
                return;
            }
            AxMdMessage::Error(error) => {
                assert_eq!(error.request_id, value["rid"].as_i64());
                assert_eq!(error.message, value["error"]["message"].as_str().unwrap());
                return;
            }
        };

        assert_eq!(tag, value["t"].as_str().unwrap());
        assert_eq!(ts, value["ts"].as_i64().unwrap());
    }

    #[rstest]
    #[case(include_str!("../../test_data/captured/ws-orders-response-2.json"))]
    #[case(include_str!("../../test_data/captured/ws-orders-response-3.json"))]
    fn test_orders_error_alias(#[case] raw: &str) {
        let mut value: serde_json::Value = serde_json::from_str(raw).unwrap();
        let obj = value.as_object_mut().unwrap();
        let rid = obj.remove("rid").unwrap();
        let error = obj.remove("err").unwrap();
        obj.insert("request_id".into(), rid);
        obj.insert("error".into(), error);

        let AxOrdersWsFrame::Error(response) = parse_order_message(&value.to_string()).unwrap()
        else {
            panic!("expected error")
        };

        assert_eq!(response.rid, value["request_id"].as_i64().unwrap());
        assert_eq!(response.err.code, value["error"]["code"].as_i64());
        assert_eq!(response.err.msg.as_deref(), value["error"]["msg"].as_str());
    }
    #[rstest]
    #[case(false, false)]
    #[case(true, false)]
    #[case(false, true)]
    #[case(true, true)]
    fn test_sparse_order_error_preserves_request_identity(
        #[case] code: bool,
        #[case] message: bool,
    ) {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/ws_order_error_response.json"))
                .unwrap();
        let expected_id = value["rid"].as_i64();
        let expected_code = code.then(|| value["err"]["code"].as_i64().unwrap().to_string());

        let expected_message = if message {
            value["err"]["msg"].as_str().unwrap().to_owned()
        } else {
            "AX request failed without an error message".to_owned()
        };

        if !code {
            value["err"].as_object_mut().unwrap().remove("code");
        }

        if !message {
            value["err"].as_object_mut().unwrap().remove("msg");
        }

        let AxOrdersWsFrame::Error(response) = parse_order_message(&value.to_string()).unwrap()
        else {
            panic!("expected error")
        };

        let error: crate::websocket::messages::AxWsError = response.into();

        assert_eq!(error.request_id, expected_id);
        assert_eq!(error.code, expected_code);
        assert_eq!(error.message, expected_message);
    }
}
