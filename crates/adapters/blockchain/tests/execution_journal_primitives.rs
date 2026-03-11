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

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use alloy::primitives::address;
use nautilus_blockchain::execution::journal::{
    DuplicateSubmitDisposition, JournalEvent, JournalEventStatus, JournalIntentKind,
    OrderIdempotencyKey, append_event_jsonl, classify_duplicate_submit, load_events_jsonl,
    replay_events, stable_sort_events,
};

fn make_event(
    sequence: u64,
    ts_event_ns: u64,
    status: JournalEventStatus,
    intent_kind: JournalIntentKind,
    intent_hash: &str,
    tx_hash: Option<&str>,
) -> JournalEvent {
    JournalEvent {
        sequence,
        ts_event_ns,
        idempotency_key: OrderIdempotencyKey::new(
            "Bsc:PancakeSwapV2",
            address!("0x3333333333333333333333333333333333333333"),
            "COID-001",
        ),
        intent_kind,
        intent_hash: intent_hash.to_string(),
        tx_hash: tx_hash.map(ToString::to_string),
        raw_tx_hash: tx_hash.map(|hash| format!("raw:{hash}")),
        reserved_nonce: Some(11),
        status,
    }
}

fn unique_temp_path(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    path.push(format!("nautilus-journal-{tag}-{nanos}.jsonl"));
    path
}

#[test]
fn test_idempotency_key_is_deterministic_and_wallet_is_normalized() {
    let key_a = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );
    let key_b = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );

    assert_eq!(key_a, key_b);
    assert_eq!(
        key_a.stable_key(),
        "Bsc:PancakeSwapV2|0x3333333333333333333333333333333333333333|COID-001"
    );
}

#[test]
fn test_stable_sort_events_orders_by_key_and_sequence() {
    let mut event_a = make_event(
        2,
        200,
        JournalEventStatus::Accepted,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xabc"),
    );
    let mut event_b = make_event(
        1,
        100,
        JournalEventStatus::Submitted,
        JournalIntentKind::Approve,
        "intent-approve",
        None,
    );

    event_a.idempotency_key.client_order_id = "COID-002".to_string();
    event_b.idempotency_key.client_order_id = "COID-001".to_string();

    let sorted = stable_sort_events(vec![event_a, event_b]);
    assert_eq!(sorted[0].idempotency_key.client_order_id, "COID-001");
    assert_eq!(sorted[0].sequence, 1);
    assert_eq!(sorted[1].idempotency_key.client_order_id, "COID-002");
}

#[test]
fn test_replay_events_applies_monotonic_status_and_dedupes() {
    let submitted = make_event(
        1,
        100,
        JournalEventStatus::Submitted,
        JournalIntentKind::Swap,
        "intent-swap",
        None,
    );
    let accepted = make_event(
        2,
        200,
        JournalEventStatus::Accepted,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );
    let filled = make_event(
        3,
        300,
        JournalEventStatus::Filled,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );
    let duplicate_filled = filled.clone();
    let late_regression = make_event(
        4,
        400,
        JournalEventStatus::Accepted,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );

    let states = replay_events(&[
        late_regression,
        accepted,
        duplicate_filled,
        submitted,
        filled,
    ]);
    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );
    let state = states.get(&key).expect("replayed order state should exist");

    assert_eq!(state.status, JournalEventStatus::Filled);
    assert_eq!(state.sequence, 3);
    assert_eq!(state.terminal_tx_hash.as_deref(), Some("0xaaaa"));
    assert_eq!(state.intents.len(), 1);
    let intent = state
        .intents
        .get("intent-swap")
        .expect("swap intent state should exist");
    assert_eq!(intent.status, JournalEventStatus::Filled);
    assert_eq!(intent.sequence, 3);

    assert_eq!(
        classify_duplicate_submit(Some(state)),
        DuplicateSubmitDisposition::RejectTerminal
    );
}

#[test]
fn test_replay_events_conflicting_terminal_statuses_are_deterministic() {
    let filled = make_event(
        5,
        500,
        JournalEventStatus::Filled,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );
    let rejected = make_event(
        5,
        500,
        JournalEventStatus::Rejected,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );

    let states_a = replay_events(&[filled.clone(), rejected.clone()]);
    let states_b = replay_events(&[rejected, filled]);

    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );
    let state_a = states_a.get(&key).expect("state should exist");
    let state_b = states_b.get(&key).expect("state should exist");

    assert_eq!(state_a.status, JournalEventStatus::Rejected);
    assert_eq!(state_a, state_b);
}

#[test]
fn test_replay_events_multi_intent_keeps_order_inflight_until_latest_intent_terminal() {
    let approve_filled = make_event(
        2,
        200,
        JournalEventStatus::Filled,
        JournalIntentKind::Approve,
        "intent-approve",
        Some("0xaaaa"),
    );
    let swap_submitted = make_event(
        3,
        300,
        JournalEventStatus::Submitted,
        JournalIntentKind::Swap,
        "intent-swap",
        None,
    );

    let states = replay_events(&[approve_filled, swap_submitted]);
    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );
    let state = states.get(&key).expect("replayed order state should exist");

    assert_eq!(state.status, JournalEventStatus::Submitted);
    assert_eq!(state.sequence, 3);
    assert_eq!(state.terminal_tx_hash, None);
    assert_eq!(state.intents.len(), 2);
    assert_eq!(
        classify_duplicate_submit(Some(state)),
        DuplicateSubmitDisposition::NoOpInFlight
    );
}

#[test]
fn test_classify_duplicate_submit_non_terminal_is_noop() {
    let accepted = make_event(
        2,
        200,
        JournalEventStatus::Accepted,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );

    let states = replay_events(&[accepted]);
    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        address!("0x3333333333333333333333333333333333333333"),
        "COID-001",
    );
    let state = states.get(&key).expect("state should exist");

    assert_eq!(
        classify_duplicate_submit(Some(state)),
        DuplicateSubmitDisposition::NoOpInFlight
    );
    assert_eq!(
        classify_duplicate_submit(None),
        DuplicateSubmitDisposition::New
    );
}

#[test]
fn test_load_events_jsonl_ignores_partial_final_line() {
    let path = unique_temp_path("partial");
    let event = make_event(
        1,
        100,
        JournalEventStatus::Submitted,
        JournalIntentKind::Swap,
        "intent-swap",
        None,
    );

    let valid_line = serde_json::to_string(&event).expect("event should serialize");
    let payload = format!("{valid_line}\n{{\"sequence\":");
    fs::write(&path, payload).expect("should write journal fixture");

    let loaded = load_events_jsonl(&path).expect("load should tolerate partial final line");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], event);

    let _ = fs::remove_file(path);
}

#[test]
fn test_append_and_load_jsonl_roundtrip() {
    let path = unique_temp_path("append");
    let event_a = make_event(
        1,
        100,
        JournalEventStatus::Submitted,
        JournalIntentKind::Swap,
        "intent-swap",
        None,
    );
    let event_b = make_event(
        2,
        200,
        JournalEventStatus::Accepted,
        JournalIntentKind::Swap,
        "intent-swap",
        Some("0xaaaa"),
    );

    append_event_jsonl(&path, &event_a).expect("append should succeed");
    append_event_jsonl(&path, &event_b).expect("append should succeed");

    let loaded = load_events_jsonl(&path).expect("load should succeed");
    assert_eq!(loaded, vec![event_a, event_b]);

    let _ = fs::remove_file(path);
}
