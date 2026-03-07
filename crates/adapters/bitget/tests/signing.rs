// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use nautilus_bitget::common::signing::{
    rest_sign_base64, rest_signature_payload, ws_login_sign_base64,
};

#[test]
fn rest_payload_with_querystring_matches_docs_shape() {
    let query = "limit=20&symbol=BTCUSDT";
    let payload = rest_signature_payload(
        1_659_927_630_000,
        "GET",
        "/api/mix/v2/market/depth",
        Some(query),
        None,
    )
    .expect("payload should be UTF-8");

    assert_eq!(
        payload,
        "1659927630000GET/api/mix/v2/market/depth?limit=20&symbol=BTCUSDT"
    );
}

#[test]
fn rest_payload_with_body_matches_docs_shape() {
    let body = b"{\"symbol\":\"BTCUSDT\",\"size\":\"0.1\"}";

    let payload = rest_signature_payload(
        1_659_927_630_000,
        "post",
        "/api/v2/spot/trade/place-order",
        None,
        Some(body),
    )
    .expect("payload should be UTF-8");

    assert_eq!(
        payload,
        "1659927630000POST/api/v2/spot/trade/place-order{\"symbol\":\"BTCUSDT\",\"size\":\"0.1\"}"
    );
}

#[test]
fn rest_signature_payload_with_invalid_utf8_body_returns_error() {
    let body = [0xff, 0xfe, 0xfd];

    assert!(rest_signature_payload(
        1_659_927_630_000,
        "POST",
        "/api/v2/spot/trade/place-order",
        None,
        Some(&body),
    )
    .is_err());
}

#[test]
fn rest_signature_matches_expected_vector() {
    let query = "limit=20&symbol=BTCUSDT";
    let sig = rest_sign_base64(
        "testsecret",
        1_659_927_630_000,
        "GET",
        "/api/mix/v2/market/depth",
        Some(query),
        None,
    );

    assert_eq!(sig, "BUbassOUwHdAjeOFu8D5FZfp4i2JGYCsS9yBvRLaC0U=");
}

#[test]
fn rest_sign_with_invalid_utf8_body_is_deterministic() {
    let body = [0xff, 0xfe, 0xfd];
    let sig1 = rest_sign_base64(
        "testsecret",
        1_659_927_630_000,
        "POST",
        "/api/v2/spot/trade/place-order",
        None,
        Some(&body),
    );
    let sig2 = rest_sign_base64(
        "testsecret",
        1_659_927_630_000,
        "POST",
        "/api/v2/spot/trade/place-order",
        None,
        Some(&body),
    );

    assert_eq!(sig1, sig2);
}

#[test]
fn ws_login_signature_is_deterministic() {
    let sig = ws_login_sign_base64("testsecret", 1_659_927_630_000);
    assert_eq!(sig, "ReLoBF0dP2l8UDnVe5VABcut6c7GGb+QFkEZ/KeNw8k=");
}
