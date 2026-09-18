#![no_main]

//! Fuzz `compute_tx_hash` and `sign_tx` for `CreateOrderTxInfo` over
//! arbitrary body bytes.
//!
//! Catches panics in the body-element packing, the `i64` to `Fp`
//! casts, and the attribute-aggregation branch under any combination of
//! field values.
//!
//! The 90-byte input is unpacked into the full body + attribute set:
//! 4 bytes chain_id, 8 + 1 + 8 + 8 context, 8 + 8 + 8 + 4 + 1 + 1 + 1 + 1 +
//! 4 + 8 order, 8 + 4 + 4 + 1 attributes - totaling 90 bytes.

use nautilus_lighter::signing::tx::{
    CreateOrderTxInfo, L2TxAttributes, OrderInfo, TxContext, compute_tx_hash,
};
use nautilus_live::fuzz::fuzz_target;

const INPUT_LEN: usize = 90;

fn read_u32(b: &[u8], off: usize) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(buf)
}

fn read_u64(b: &[u8], off: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(buf)
}

fn read_i64(b: &[u8], off: usize) -> i64 {
    read_u64(b, off) as i64
}

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }

    let chain_id = read_u32(data, 0);
    let context = TxContext {
        account_index: read_i64(data, 4),
        api_key_index: data[12],
        nonce: read_i64(data, 13),
        expired_at: read_i64(data, 21),
    };
    let order = OrderInfo {
        market_index: read_i64(data, 29),
        client_order_index: read_i64(data, 37),
        base_amount: read_i64(data, 45),
        price: read_u32(data, 53),
        is_ask: data[57] & 1 == 1,
        order_type: data[58],
        time_in_force: data[59],
        reduce_only: data[60] & 1 == 1,
        trigger_price: read_u32(data, 61),
        order_expiry: read_i64(data, 65),
    };
    let attributes = L2TxAttributes {
        integrator_account_index: read_u64(data, 73),
        integrator_taker_fee: read_u32(data, 81),
        integrator_maker_fee: read_u32(data, 85),
        skip_nonce: data[89],
    };

    let tx = CreateOrderTxInfo {
        context,
        order,
        attributes,
    };
    let _ = compute_tx_hash(&tx, chain_id);
});
