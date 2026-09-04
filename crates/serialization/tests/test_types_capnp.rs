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

//! Cap'n Proto serialization integration tests for value types.

#![cfg(feature = "capnp")]

use nautilus_core::UUID4;
use nautilus_model::{
    enums::CurrencyType,
    types::{
        AccountBalance, Currency, MarginBalance, Money, Price, Quantity,
        fixed::check_fixed_precision,
    },
};
use nautilus_serialization::capnp::{
    FromCapnp, ToCapnp, base_capnp,
    conversions::{
        deserialize_account_balance, deserialize_currency, deserialize_instrument_id,
        deserialize_margin_balance, deserialize_money, deserialize_price, deserialize_quantity,
    },
    enums_capnp, types_capnp,
};
use rstest::rstest;

#[rstest]
#[case(Price::from("100.50"))]
#[case(Price::from("0.00001"))]
#[case(Price::from("99999.999"))]
#[case(Price::from("1.0"))]
fn test_price_roundtrip(#[case] price: Price) {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::price::Builder>();
    price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::price::Reader>().unwrap();
    let decoded = Price::from_capnp(root).unwrap();

    assert_eq!(price, decoded);
}

#[rstest]
#[case(Quantity::from("1000.5"))]
#[case(Quantity::from("0.0001"))]
#[case(Quantity::from("999999.999"))]
#[case(Quantity::from("1.0"))]
fn test_quantity_roundtrip(#[case] qty: Quantity) {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::quantity::Builder>();
    qty.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::quantity::Reader>().unwrap();
    let decoded = Quantity::from_capnp(root).unwrap();

    assert_eq!(qty, decoded);
}

#[rstest]
fn test_price_invalid_precision_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::price::Builder>();
    let mut raw = builder.reborrow().init_raw();
    raw.set_lo(0);
    raw.set_hi(0);
    builder.set_precision(u8::MAX);

    let reader = message
        .get_root_as_reader::<types_capnp::price::Reader>()
        .unwrap();
    let error = Price::from_capnp(reader).unwrap_err();
    let expected_error = check_fixed_precision(u8::MAX).unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}

#[rstest]
fn test_quantity_invalid_precision_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::quantity::Builder>();
    let mut raw = builder.reborrow().init_raw();
    raw.set_lo(0);
    raw.set_hi(0);
    builder.set_precision(u8::MAX);

    let reader = message
        .get_root_as_reader::<types_capnp::quantity::Reader>()
        .unwrap();
    let error = Quantity::from_capnp(reader).unwrap_err();
    let expected_error = check_fixed_precision(u8::MAX).unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}

#[rstest]
#[case(u64::from(u32::MAX) + 1, 0, 0, "Decimal lo limb exceeds u32")]
#[case(0, u64::from(u32::MAX) + 1, 0, "Decimal mid limb exceeds u32")]
#[case(0, 0, u64::from(u32::MAX) + 1, "Decimal hi limb exceeds u32")]
fn test_decimal_oversized_limb_returns_error(
    #[case] lo: u64,
    #[case] mid: u64,
    #[case] hi: u64,
    #[case] expected_error: &str,
) {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::decimal::Builder>();
    builder.set_lo(lo);
    builder.set_mid(mid);
    builder.set_hi(hi);

    let reader = message
        .get_root_as_reader::<types_capnp::decimal::Reader>()
        .unwrap();
    let error = rust_decimal::Decimal::from_capnp(reader).unwrap_err();

    assert_eq!(error.to_string(), expected_error);
}

#[rstest]
#[case(1, "Decimal flags contain unsupported bits")]
#[case(1 << 24, "Decimal flags contain unsupported bits")]
#[case(29 << 16, "Decimal scale exceeds maximum 28, was 29")]
#[case(32 << 16, "Decimal scale exceeds maximum 28, was 32")]
fn test_decimal_invalid_flags_returns_error(#[case] flags: u32, #[case] expected_error: &str) {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::decimal::Builder>();
    builder.set_lo(1);
    builder.set_flags(flags);

    let reader = message
        .get_root_as_reader::<types_capnp::decimal::Reader>()
        .unwrap();
    let error = rust_decimal::Decimal::from_capnp(reader).unwrap_err();

    assert_eq!(error.to_string(), expected_error);
}

#[rstest]
fn test_uuid4_invalid_length_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    message
        .init_root::<base_capnp::u_u_i_d4::Builder>()
        .set_value(&[0; 15]);

    let reader = message
        .get_root_as_reader::<base_capnp::u_u_i_d4::Reader>()
        .unwrap();
    let error = UUID4::from_capnp(reader).unwrap_err();

    assert_eq!(error.to_string(), "Invalid UUID4 bytes length");
}

#[rstest]
fn test_deserializers_truncated_message_return_error() {
    let bytes = [0];
    let errors = [
        deserialize_instrument_id(&bytes).unwrap_err().to_string(),
        deserialize_price(&bytes).unwrap_err().to_string(),
        deserialize_quantity(&bytes).unwrap_err().to_string(),
        deserialize_currency(&bytes).unwrap_err().to_string(),
        deserialize_money(&bytes).unwrap_err().to_string(),
        deserialize_account_balance(&bytes).unwrap_err().to_string(),
        deserialize_margin_balance(&bytes).unwrap_err().to_string(),
    ];

    assert_eq!(errors, ["failed to fill the whole buffer"; 7]);
}

#[rstest]
fn test_currency_invalid_precision_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::currency::Builder>();
    builder.set_code("USD");
    builder.set_precision(u8::MAX);
    builder.set_iso4217(840);
    builder.set_name("United States dollar");
    builder.set_currency_type(enums_capnp::CurrencyType::Fiat);

    let reader = message
        .get_root_as_reader::<types_capnp::currency::Reader>()
        .unwrap();
    let error = Currency::from_capnp(reader).unwrap_err();
    let expected_error = Currency::new_checked(
        "USD",
        u8::MAX,
        840,
        "United States dollar",
        CurrencyType::Fiat,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}

#[rstest]
fn test_account_balance_invalid_total_returns_error() {
    let total = Money::from("100 USD");
    let locked = Money::from("10 USD");
    let free = Money::from("80 USD");
    let expected_error = AccountBalance::new_checked(total, locked, free).unwrap_err();
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::account_balance::Builder>();
    total.to_capnp(builder.reborrow().init_total());
    locked.to_capnp(builder.reborrow().init_locked());
    free.to_capnp(builder.init_free());

    let reader = message
        .get_root_as_reader::<types_capnp::account_balance::Reader>()
        .unwrap();
    let error = AccountBalance::from_capnp(reader).unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}

#[rstest]
fn test_margin_balance_currency_mismatch_returns_error() {
    let initial = Money::from("100 USD");
    let maintenance = Money::from("10 BTC");
    let expected_error = MarginBalance::new_checked(initial, maintenance, None).unwrap_err();
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::margin_balance::Builder>();
    initial.to_capnp(builder.reborrow().init_initial());
    maintenance.to_capnp(builder.init_maintenance());

    let reader = message
        .get_root_as_reader::<types_capnp::margin_balance::Reader>()
        .unwrap();
    let error = MarginBalance::from_capnp(reader).unwrap_err();

    assert_eq!(error.to_string(), expected_error.to_string());
}

#[cfg(not(feature = "high-precision"))]
#[rstest]
fn test_price_raw_overflow_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::price::Builder>();
    let mut raw = builder.reborrow().init_raw();
    raw.set_hi(1);

    let reader = message
        .get_root_as_reader::<types_capnp::price::Reader>()
        .unwrap();
    let error = Price::from_capnp(reader).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Price value overflows i64 in standard precision mode"
    );
}

#[cfg(not(feature = "high-precision"))]
#[rstest]
fn test_quantity_raw_overflow_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::quantity::Builder>();
    let mut raw = builder.reborrow().init_raw();
    raw.set_hi(1);

    let reader = message
        .get_root_as_reader::<types_capnp::quantity::Reader>()
        .unwrap();
    let error = Quantity::from_capnp(reader).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Quantity value overflows u64 in standard precision mode"
    );
}

#[cfg(not(feature = "high-precision"))]
#[rstest]
fn test_money_raw_overflow_returns_error() {
    let mut message = capnp::message::Builder::new_default();
    let mut builder = message.init_root::<types_capnp::money::Builder>();
    {
        let mut raw = builder.reborrow().init_raw();
        raw.set_hi(1);
    }
    Currency::USD().to_capnp(builder.init_currency());

    let reader = message
        .get_root_as_reader::<types_capnp::money::Reader>()
        .unwrap();
    let error = Money::from_capnp(reader).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Money value overflows i64 in standard precision mode"
    );
}

#[rstest]
fn test_price_capnp_conversion_roundtrip() {
    let price = Price::from("123.45");
    let bytes = nautilus_serialization::capnp::conversions::serialize_price(&price).unwrap();
    let decoded = nautilus_serialization::capnp::conversions::deserialize_price(&bytes).unwrap();
    assert_eq!(price, decoded);
}

#[rstest]
fn test_quantity_capnp_conversion_roundtrip() {
    let qty = Quantity::from("100.5");
    let bytes = nautilus_serialization::capnp::conversions::serialize_quantity(&qty).unwrap();
    let decoded = nautilus_serialization::capnp::conversions::deserialize_quantity(&bytes).unwrap();
    assert_eq!(qty, decoded);
}

#[rstest]
fn test_price_zero() {
    let price = Price::from("0.0");
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::price::Builder>();
    price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::price::Reader>().unwrap();
    let decoded = Price::from_capnp(root).unwrap();

    assert_eq!(price, decoded);
}

#[rstest]
fn test_quantity_zero() {
    let qty = Quantity::from("0.0");
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::quantity::Builder>();
    qty.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::quantity::Reader>().unwrap();
    let decoded = Quantity::from_capnp(root).unwrap();

    assert_eq!(qty, decoded);
}

#[rstest]
fn test_price_negative() {
    let price = Price::from("-50.25");
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::price::Builder>();
    price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::price::Reader>().unwrap();
    let decoded = Price::from_capnp(root).unwrap();

    assert_eq!(price, decoded);
}

#[rstest]
fn test_price_max_precision() {
    let price = Price::from("123.123456789");
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::price::Builder>();
    price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::price::Reader>().unwrap();
    let decoded = Price::from_capnp(root).unwrap();

    assert_eq!(price, decoded);
}

#[rstest]
fn test_quantity_max_precision() {
    let qty = Quantity::from("100.123456789");
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<types_capnp::quantity::Builder>();
    qty.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<types_capnp::quantity::Reader>().unwrap();
    let decoded = Quantity::from_capnp(root).unwrap();

    assert_eq!(qty, decoded);
}
