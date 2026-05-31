#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use nautilus_derive::signing::nonce::NonceManager;

const CHUNK_LEN: usize = 38;
const MAX_DOMAIN_NOW_MS: u64 = 4_102_444_800_000;
const MAX_DOMAIN_NONCE: u64 = (MAX_DOMAIN_NOW_MS * 10) + 9;
const MAX_STEPS: usize = 256;

fuzz_target!(|data: &[u8]| {
    let manager = NonceManager::new();
    let mut expected_last = BTreeMap::<(String, u64), u64>::new();

    for chunk in data.chunks_exact(CHUNK_LEN).take(MAX_STEPS) {
        let wallet = wallet_from_chunk(&chunk[1..22]);
        let subaccount_id = read_u64(chunk, 22);
        let raw_value = read_u64(chunk, 30);
        let key = (wallet.to_ascii_lowercase(), subaccount_id);
        let last = *expected_last.get(&key).unwrap_or(&0);

        if chunk[0] & 1 == 0 {
            let now_ms = raw_value % (MAX_DOMAIN_NOW_MS + 1);
            let nonce = manager.next_nonce_at(&wallet, subaccount_id, now_ms);
            let initial = now_ms.saturating_mul(10);
            let expected = if initial > last {
                initial
            } else {
                last.checked_add(1)
                    .expect("nonce sequence exceeded u64::MAX")
            };

            assert_eq!(nonce, expected, "next nonce diverged from model");
            expected_last.insert(key, nonce);
        } else {
            let last_seen_nonce = raw_value % (MAX_DOMAIN_NONCE + 1);
            manager.refresh(&wallet, subaccount_id, last_seen_nonce);
            let expected = last.max(last_seen_nonce);
            if expected == 0 {
                assert!(
                    manager.last_issued(&wallet, subaccount_id).is_none(),
                    "zero refresh must not surface as issued nonce",
                );
            } else {
                assert_eq!(
                    manager.last_issued(&wallet, subaccount_id),
                    Some(expected),
                    "refresh must advance monotonically",
                );
                expected_last.insert(key, expected);
            }
        }
    }
});

fn wallet_from_chunk(bytes: &[u8]) -> String {
    let mut wallet = String::with_capacity(42);
    wallet.push_str("0x");

    for (idx, byte) in bytes[..20].iter().enumerate() {
        let upper = bytes[20] & (1 << (idx % 8)) != 0;
        push_hex_byte(&mut wallet, *byte, upper);
    }
    wallet
}

fn push_hex_byte(out: &mut String, byte: u8, upper: bool) {
    let alphabet = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    out.push(alphabet[(byte >> 4) as usize] as char);
    out.push(alphabet[(byte & 0x0f) as usize] as char);
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(buf)
}
