#![no_main]

use std::sync::Once;

use aws_lc_rs::{
    rsa::KeyPair as RsaKeyPair,
    signature::{
        ED25519, Ed25519KeyPair, KeyPair as _, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey,
    },
};
use base64::prelude::*;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_kalshi::{
    common::{credential::KalshiCredential, enums::KalshiEnvironment, parse::parse_ticker},
    execution::request,
    http::{
        client::{stable_object_key_bytes, validate_create},
        models::{BookSide, TimeInForce as VenueTimeInForce},
    },
};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    orders::{LimitOrder, Order, OrderAny},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};

#[path = "../common/mod.rs"]
mod common;

#[allow(dead_code)]
#[path = "../../tests/integration/keys.rs"]
mod keys;

fuzz_target!(|data: &[u8]| {
    independent_checks();
    let Some(data) = common::capped(data) else {
        return;
    };

    if data.len() < 4 {
        return;
    }
    let instrument = common::instrument(None);
    assert_stable_rejections(&instrument.id);
    let Some(order) = order(&instrument.id, data) else {
        return;
    };
    let first = request(&order, None, 0, 0);
    let second = request(&order, None, 0, 0);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "order construction rejection changed"
    );

    if order.time_in_force() == TimeInForce::Day {
        assert!(first.is_err(), "day order was accepted");
        return;
    }
    let built = first.expect("supported order was rejected");
    assert_eq!(
        built.price.0,
        order.price().unwrap().as_decimal().normalize()
    );
    assert_eq!(built.count.0, order.quantity().as_decimal().normalize());
    assert_eq!(
        built.side,
        if order.order_side() == OrderSide::Buy {
            BookSide::Bid
        } else {
            BookSide::Ask
        }
    );
    assert_eq!(
        built.time_in_force,
        match order.time_in_force() {
            TimeInForce::Ioc => VenueTimeInForce::ImmediateOrCancel,
            TimeInForce::Fok => VenueTimeInForce::FillOrKill,
            TimeInForce::Gtc | TimeInForce::Gtd => VenueTimeInForce::GoodTillCanceled,
            _ => unreachable!("rejected time in force returned a request"),
        }
    );
    assert_eq!(
        built.client_order_id.as_deref(),
        Some(order.client_order_id().as_str())
    );
    assert_eq!(built.ticker, parse_ticker(order.instrument_id()).unwrap());
    if order.time_in_force() == TimeInForce::Gtd {
        assert_eq!(built.expiration_time, Some(4_000_000_000));
    } else {
        assert_eq!(built.expiration_time, None);
    }
    assert!(
        built.price.0 > Decimal::ZERO && built.price.0 < Decimal::ONE,
        "constructed price left the open binary interval"
    );
    assert!(
        built.price.0.scale() <= 4,
        "constructed price scale exceeds 4"
    );
    assert!(
        built.count.0 > Decimal::ZERO,
        "constructed count is not positive"
    );
    assert!(
        built.count.0.scale() <= 2,
        "constructed count scale exceeds 2"
    );
    validate_create(&built).expect("accepted request failed create validation");
    let body = stable_object_key_bytes(serde_json::to_value(&built).unwrap()).unwrap();
    let again = stable_object_key_bytes(serde_json::to_value(&built).unwrap()).unwrap();
    assert_eq!(body, again, "order body encoding changed");
    assert!(body.len() < 4_096, "order body is unbounded");
    assert_sorted_keys(&serde_json::from_slice(&body).unwrap());
});

fn assert_sorted_keys(value: &Value) {
    match value {
        Value::Object(map) => {
            let keys: Vec<_> = map.keys().collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "object keys are not sorted");

            for child in map.values() {
                assert_sorted_keys(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_sorted_keys(item);
            }
        }
        _ => {}
    }
}

fn independent_checks() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        verify_ed25519();
        verify_rsa();
        reject_headers();
        assert_key_order();
    });
}

fn assert_key_order() {
    let forward = stable_object_key_bytes(json!({"z": 1, "a": {"b": 1, "a": 2}, "m": 3})).unwrap();
    let reverse = stable_object_key_bytes(json!({"m": 3, "a": {"a": 2, "b": 1}, "z": 1})).unwrap();
    assert_eq!(forward, reverse, "key order changed the encoding");
    assert_sorted_keys(&serde_json::from_slice(&forward).unwrap());
}

fn assert_stable_rejections(instrument_id: &InstrumentId) {
    for (price, count, message) in [
        ("0.00001", "1.00", "excess price scale was accepted"),
        ("0.5000", "1.001", "excess count scale was accepted"),
        ("0.0000", "1.00", "zero price was accepted"),
        ("1.0000", "1.00", "unit price was accepted"),
    ] {
        let order = limit_order(
            instrument_id,
            OrderSide::Buy,
            TimeInForce::Gtc,
            price,
            count,
        )
        .expect("rejection probe must be constructible");
        assert!(request(&order, None, 0, 0).is_err(), "{message}");
    }
}

fn order(instrument_id: &InstrumentId, data: &[u8]) -> Option<OrderAny> {
    let side = if data[0].is_multiple_of(2) {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let tif = match data[2] % 5 {
        0 => TimeInForce::Gtc,
        1 => TimeInForce::Ioc,
        2 => TimeInForce::Fok,
        3 => TimeInForce::Gtd,
        _ => TimeInForce::Day,
    };

    let price_units = 1 + u16::from_le_bytes([data[3], data.get(4).copied().unwrap_or(0)]) % 9_998;
    let count_units = 1 + u16::from(data.get(5).copied().unwrap_or(1));

    limit_order(
        instrument_id,
        side,
        tif,
        &format!("0.{price_units:04}"),
        &format!("{}.{:02}", count_units / 100, count_units % 100),
    )
}

fn limit_order(
    instrument_id: &InstrumentId,
    side: OrderSide,
    tif: TimeInForce,
    price: &str,
    count: &str,
) -> Option<OrderAny> {
    let expire = (tif == TimeInForce::Gtd).then_some(UnixNanos::from(4_000_000_000_000_000_000));
    let order = LimitOrder::new_checked(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-FUZZ"),
        *instrument_id,
        ClientOrderId::from("O-FUZZ-1"),
        side,
        Quantity::from(count),
        Price::from(price),
        tif,
        expire,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::from(1),
    )
    .ok()?;
    Some(OrderAny::Limit(order))
}

fn verify_ed25519() {
    let credential = KalshiCredential::resolve(
        Some("fuzz-key".into()),
        Some(keys::ed25519_pkcs8().into()),
        KalshiEnvironment::Demo,
    )
    .unwrap();
    let headers = credential
        .headers(
            1_700_000_000_000,
            "post",
            "/trade-api/v2/portfolio/events/orders?token=secret",
        )
        .unwrap();
    assert_eq!(headers.len(), 3);
    let signature = BASE64_STANDARD
        .decode(headers[2].1.expose_secret())
        .unwrap();
    let key = Ed25519KeyPair::from_pkcs8(include_bytes!("../../test_data/auth_ed25519_pkcs8.der"))
        .unwrap();
    UnparsedPublicKey::new(&ED25519, key.public_key().as_ref())
        .verify(
            b"1700000000000POST/trade-api/v2/portfolio/events/orders",
            &signature,
        )
        .expect("Ed25519 request signature did not verify");
    assert!(!format!("{headers:?}").contains(headers[2].1.expose_secret()));
}

fn verify_rsa() {
    let credential = KalshiCredential::resolve(
        Some("fuzz-key".into()),
        Some(keys::rsa_pkcs8().into()),
        KalshiEnvironment::Demo,
    )
    .unwrap();
    let headers = credential
        .headers(
            1_700_000_000_000,
            "POST",
            "/trade-api/v2/portfolio/events/orders",
        )
        .unwrap();
    let signature = BASE64_STANDARD
        .decode(headers[2].1.expose_secret())
        .unwrap();
    let key = RsaKeyPair::from_pkcs8(include_bytes!("../../test_data/auth_rsa_pkcs8.der")).unwrap();
    UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA256, key.public_key().as_ref())
        .verify(
            b"1700000000000POST/trade-api/v2/portfolio/events/orders",
            &signature,
        )
        .expect("RSA-PSS request signature did not verify");
}

fn reject_headers() {
    let credential = KalshiCredential::resolve(
        Some("fuzz-key".into()),
        Some(keys::ed25519_pkcs8().into()),
        KalshiEnvironment::Demo,
    )
    .unwrap();

    for (method, path) in [
        ("GET", "trade-api"),
        ("", "/trade-api/v2"),
        ("POST", "//secret"),
    ] {
        assert!(credential.headers(1, method, path).is_err());
        assert!(credential.headers(1, method, path).is_err());
    }
}
