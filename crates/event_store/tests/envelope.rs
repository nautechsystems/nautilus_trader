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

//! On-disk envelope contract: bincode round-trips for `EventStoreEntry` and `RunManifest`
//! must preserve every field, and the recomputed hash on a decoded entry must still match
//! the stored hash.

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_event_store::{
    EventStoreEntry, Headers, RegisteredComponents, RunManifest, RunStatus, Topic,
    compute_entry_hash,
};
use rstest::rstest;
use ustr::Ustr;

fn entry_with(headers: Headers) -> EventStoreEntry {
    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let payload = Bytes::from_static(b"\x01\x02\x03\x04");
    let ts_init = UnixNanos::from(1_700_000_000_000_000_000);
    let ts_publish = UnixNanos::from(1_700_000_000_000_000_001);
    let hash = compute_entry_hash(
        7,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload,
        &headers,
    );

    EventStoreEntry::new(
        hash,
        7,
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        ts_publish,
    )
}

fn manifest_with(status: RunStatus, end_ts_init: Option<UnixNanos>) -> RunManifest {
    RunManifest {
        run_id: "1700000000-abcd1234".to_string(),
        parent_run_id: None,
        instance_id: "trader-001".to_string(),
        binary_hash: "deadbeef".to_string(),
        schema_version: 1,
        crate_versions: "feedface".to_string(),
        feature_flags: vec!["live".to_string()],
        adapter_versions: IndexMap::new(),
        config_hash: "cafebabe".to_string(),
        registered_components: RegisteredComponents::default(),
        seed: None,
        start_ts_init: UnixNanos::from(0),
        end_ts_init,
        high_watermark: 0,
        status,
    }
}

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let config = bincode::config::standard();
    let encoded = bincode::serde::encode_to_vec(value, config).expect("serialize");
    let (decoded, _) =
        bincode::serde::decode_from_slice::<T, _>(&encoded, config).expect("deserialize");
    decoded
}

#[rstest]
fn entry_round_trip_empty_headers() {
    let entry = entry_with(Headers::empty());
    let decoded = round_trip(&entry);

    assert_eq!(decoded, entry);
    assert_eq!(decoded.recompute_hash(), decoded.entry_hash);
}

#[rstest]
fn entry_round_trip_populated_headers() {
    // All three header fields must round-trip through bincode and the hash must
    // still validate; catches regressions in `wire::nanos_as_u64` and in
    // `Option<UUID4>` serde coupling under non-self-describing formats.
    let headers = Headers {
        correlation_id: Some(UUID4::new()),
        causation_id: Some(UUID4::new()),
    };
    let entry = entry_with(headers);
    let decoded = round_trip(&entry);

    assert_eq!(decoded, entry);
    assert_eq!(decoded.recompute_hash(), decoded.entry_hash);
}

#[rstest]
fn manifest_round_trip_running_with_none_end_ts() {
    // The Running case never sets end_ts_init; the None path through
    // `wire::opt_nanos_as_u64` must round-trip without becoming Some(0).
    let manifest = manifest_with(RunStatus::Running, None);
    let decoded = round_trip(&manifest);

    assert_eq!(decoded, manifest);
    assert!(decoded.end_ts_init.is_none());
}

#[rstest]
fn manifest_round_trip_sealed_with_some_end_ts() {
    let mut manifest = manifest_with(RunStatus::Ended, Some(UnixNanos::from(123_456_789)));
    manifest.high_watermark = 99;
    manifest
        .adapter_versions
        .insert("binance".to_string(), "v1.2.3".to_string());
    let decoded = round_trip(&manifest);

    assert_eq!(decoded, manifest);
    assert_eq!(decoded.end_ts_init, Some(UnixNanos::from(123_456_789)));
    assert_eq!(decoded.high_watermark, 99);
}

#[rstest]
fn manifest_round_trip_populated_components() {
    let mut components = RegisteredComponents::default();
    components
        .actors
        .insert("actor-1".to_string(), "abc123".to_string());
    components
        .strategies
        .insert("strat-1".to_string(), "def456".to_string());
    components
        .algorithms
        .insert("algo-1".to_string(), "789ghi".to_string());
    components.subscriptions.push("data.quotes.*".to_string());
    components.endpoints.push("Cache.Client".to_string());

    let mut manifest = manifest_with(RunStatus::Running, None);
    manifest.registered_components = components;
    let decoded = round_trip(&manifest);

    assert_eq!(decoded, manifest);
}
