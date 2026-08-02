#![no_main]

use std::{
    collections::BTreeMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use nautilus_derive::signing::nonce::{NonceError, NonceManager};
use nautilus_live::fuzz::fuzz_target;

const CHUNK_LEN: usize = 17;
const MAX_DOMAIN_NOW_MS: u64 = 4_102_444_800_000;
const MAX_STEPS: usize = 256;
const NONCE_SUFFIX_BASE: u64 = 1_000;
const NONCE_SUFFIX_MAX: u64 = NONCE_SUFFIX_BASE - 1;
const WALLET_A_LOWER: &str = "0x000000000000000000000000000000000000aaaa";
const WALLET_A_UPPER: &str = "0x000000000000000000000000000000000000AAAA";
const WALLET_B_LOWER: &str = "0x000000000000000000000000000000000000bbbb";
const WALLET_B_UPPER: &str = "0x000000000000000000000000000000000000BBBB";

static EXPECTED_LAST: LazyLock<Mutex<BTreeMap<(String, u64), u64>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

fuzz_target!(|data: &[u8]| {
    let managers = [NonceManager::new(), NonceManager::new()];
    let mut expected_last = EXPECTED_LAST.lock().unwrap_or_else(PoisonError::into_inner);

    let (chunks, _remainder) = data.as_chunks::<CHUNK_LEN>();

    for chunk in chunks.iter().take(MAX_STEPS) {
        let manager = &managers[usize::from(chunk[0] & 1)];
        let wallet = wallet_from_selector(chunk[0] >> 1);
        let subaccount_id = read_u64(chunk, 1) % 4;
        let now_ms = read_u64(chunk, 9) % (MAX_DOMAIN_NOW_MS + 1);
        let key = (wallet.to_ascii_lowercase(), subaccount_id);
        let last = expected_last.get(&key).copied();
        let initial = now_ms * NONCE_SUFFIX_BASE;
        let expected = match last {
            None => Ok(initial),
            Some(last) if initial > last => Ok(initial),
            Some(last) if last % NONCE_SUFFIX_BASE == NONCE_SUFFIX_MAX => {
                Err(NonceError::SuffixExhausted {
                    millisecond: last / NONCE_SUFFIX_BASE,
                })
            }
            Some(last) => Ok(last + 1),
        };
        let actual = manager.next_nonce_at(wallet, subaccount_id, now_ms);

        assert_eq!(actual, expected, "next nonce diverged from model");
        if let Ok(nonce) = actual {
            expected_last.insert(key, nonce);
            assert_eq!(
                manager.last_issued(wallet, subaccount_id),
                Some(nonce),
                "successful allocation must update shared state",
            );
        } else {
            assert_eq!(
                manager.last_issued(wallet, subaccount_id),
                last,
                "failed allocation must preserve shared state",
            );
        }
    }
});

fn wallet_from_selector(selector: u8) -> &'static str {
    match selector % 4 {
        0 => WALLET_A_LOWER,
        1 => WALLET_A_UPPER,
        2 => WALLET_B_LOWER,
        _ => WALLET_B_UPPER,
    }
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(buf)
}
