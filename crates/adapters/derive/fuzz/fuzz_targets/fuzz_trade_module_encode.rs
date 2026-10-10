#![no_main]

use alloy_primitives::{Address, U256};
use nautilus_derive::{
    common::consts::DECIMAL_PRECISION, signing::modules::trade::TradeModuleData,
};
use nautilus_live::fuzz::fuzz_target;
use rust_decimal::Decimal;

const INPUT_LEN: usize = 20 + 8 + (3 * 17) + 8 + 1;
const ABI_WORDS: usize = 7;
const ABI_WORD_BYTES: usize = 32;

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }

    let trade = TradeModuleData {
        asset_address: address(data, 0),
        sub_id: U256::from(read_u64(data, 20)),
        limit_price: decimal(data, 28),
        amount: decimal(data, 45),
        max_fee: decimal(data, 62),
        recipient_id: read_u64(data, 79),
        is_bid: data[87] & 1 == 1,
    };

    let result = trade.encode();
    let rejected = trade.limit_price.scale() > DECIMAL_PRECISION
        || trade.amount.scale() > DECIMAL_PRECISION
        || trade.max_fee.scale() > DECIMAL_PRECISION
        || trade.max_fee.is_sign_negative();
    if rejected {
        assert!(result.is_err(), "invalid financial input must be rejected");
        return;
    }
    let encoded = result.expect("representable decimals with accepted precision must encode");

    for (index, value) in [
        (2, trade.limit_price),
        (3, trade.amount),
        (4, trade.max_fee),
    ] {
        let value = value.normalize();
        let magnitude = U256::from(value.mantissa().unsigned_abs())
            * U256::from(10_u128.pow(18 - value.scale()));
        let expected = if value.is_sign_negative() && magnitude != U256::ZERO {
            U256::MAX - magnitude + U256::from(1)
        } else {
            magnitude
        };
        assert_eq!(
            U256::from_be_slice(&encoded[index * 32..(index + 1) * 32]),
            expected
        );
    }

    assert_eq!(
        encoded.len(),
        ABI_WORDS * ABI_WORD_BYTES,
        "trade module ABI length mismatch",
    );
    assert_eq!(
        encoded,
        trade.encode().expect("second encode must match first"),
        "trade module ABI encoding is non-deterministic",
    );
});

fn address(data: &[u8], offset: usize) -> Address {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&data[offset..offset + 20]);
    Address::from(bytes)
}

fn decimal(data: &[u8], offset: usize) -> Decimal {
    let mut mantissa = [0u8; 16];
    mantissa[..12].copy_from_slice(&data[offset..offset + 12]);
    let mut value = i128::from_le_bytes(mantissa);
    if data[offset + 12] & 1 == 1 {
        value = -value;
    }

    let scale = (data[offset + 16] % 29) as u32;
    Decimal::from_i128_with_scale(value, scale)
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(buf)
}
