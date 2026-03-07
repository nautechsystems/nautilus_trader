// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use std::str::Utf8Error;

use aws_lc_rs::hmac;

const WS_LOGIN_METHOD: &str = "GET";
const WS_LOGIN_PATH: &str = "/user/verify";

#[must_use]
pub fn rest_signature_payload(
    timestamp_ms: i64,
    method: &str,
    request_path: &str,
    query_string: Option<&str>,
    body_bytes: Option<&[u8]>,
) -> Result<String, Utf8Error> {
    let body = std::str::from_utf8(body_bytes.unwrap_or(b""))?;

    Ok(rest_signature_payload_no_utf8(timestamp_ms, method, request_path, query_string, body))
}

#[must_use]
fn rest_signature_payload_no_utf8(
    timestamp_ms: i64,
    method: &str,
    request_path: &str,
    query_string: Option<&str>,
    body: &str,
) -> String {
    let ts = timestamp_ms.to_string();
    let method = method.to_ascii_uppercase();

    if let Some(query_string) = query_string.filter(|v| !v.is_empty()) {
        format!("{ts}{method}{request_path}?{query_string}{body}")
    } else {
        format!("{ts}{method}{request_path}{body}")
    }
}

#[must_use]
pub fn rest_sign_base64(
    secret: &str,
    timestamp_ms: i64,
    method: &str,
    request_path: &str,
    query_string: Option<&str>,
    body: Option<&[u8]>,
) -> String {
    let payload = rest_signature_payload_bytes(timestamp_ms, method, request_path, query_string, body);
    sign_base64_bytes(secret, &payload)
}

#[must_use]
fn rest_signature_payload_bytes(
    timestamp_ms: i64,
    method: &str,
    request_path: &str,
    query_string: Option<&str>,
    body: Option<&[u8]>,
) -> Vec<u8> {
    let body = body.unwrap_or(b"");
    let ts = timestamp_ms.to_string();
    let method = method.to_ascii_uppercase();

    let mut bytes = if let Some(query_string) = query_string.filter(|v| !v.is_empty()) {
        format!("{ts}{method}{request_path}?{query_string}").into_bytes()
    } else {
        format!("{ts}{method}{request_path}").into_bytes()
    };

    bytes.extend_from_slice(body);
    bytes
}

#[must_use]
pub fn ws_login_sign_base64(secret: &str, timestamp_ms: i64) -> String {
    let payload = format!("{timestamp_ms}{WS_LOGIN_METHOD}{WS_LOGIN_PATH}");
    sign_base64(secret, &payload)
}

#[must_use]
pub fn sign_base64(secret: &str, payload: &str) -> String {
    sign_base64_bytes(secret, payload.as_bytes())
}

#[must_use]
fn sign_base64_bytes(secret: &str, payload: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, payload);
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, tag.as_ref())
}
