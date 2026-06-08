#![no_main]

use alloy_primitives::{Address, U256};
use libfuzzer_sys::fuzz_target;
use nautilus_derive::signing::modules::trade::TradeModuleData;
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

    let Ok(encoded) = trade.encode() else {
        return;
    };

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
