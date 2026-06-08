#![no_main]

use alloy_primitives::{Address, B256};
use libfuzzer_sys::fuzz_target;
use nautilus_derive::signing::eip712::{
    ActionContext, compute_action_hash, compute_typed_data_hash,
};

const INPUT_LEN: usize = 180;

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }

    let ctx = ActionContext {
        subaccount_id: read_u64(data, 0),
        nonce: read_u64(data, 8),
        module_address: address(data, 16),
        signature_expiry_sec: read_non_negative_i64(data, 36),
        owner: address(data, 44),
        signer: address(data, 64),
    };
    let module_data_hash = b256(data, 84);
    let action_typehash = b256(data, 116);
    let domain_separator = b256(data, 148);

    let action_hash = compute_action_hash(&ctx, module_data_hash, action_typehash);
    let typed_hash = compute_typed_data_hash(domain_separator, action_hash);

    assert_eq!(
        action_hash,
        compute_action_hash(&ctx, module_data_hash, action_typehash),
        "action hash is non-deterministic",
    );
    assert_eq!(
        typed_hash,
        compute_typed_data_hash(domain_separator, action_hash),
        "typed-data hash is non-deterministic",
    );
});

fn address(data: &[u8], offset: usize) -> Address {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&data[offset..offset + 20]);
    Address::from(bytes)
}

fn b256(data: &[u8], offset: usize) -> B256 {
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&data[offset..offset + 32]);
    B256::from(bytes)
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(buf)
}

fn read_non_negative_i64(data: &[u8], offset: usize) -> i64 {
    (read_u64(data, offset) & i64::MAX as u64) as i64
}
