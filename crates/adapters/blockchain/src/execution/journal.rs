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
    collections::{BTreeMap, HashSet},
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OrderIdempotencyKey {
    pub venue: String,
    pub wallet_address: String,
    pub client_order_id: String,
}

impl OrderIdempotencyKey {
    #[must_use]
    pub fn new(
        venue: impl Into<String>,
        wallet_address: Address,
        client_order_id: impl Into<String>,
    ) -> Self {
        Self {
            venue: venue.into(),
            wallet_address: wallet_address.to_string().to_ascii_lowercase(),
            client_order_id: client_order_id.into(),
        }
    }

    #[must_use]
    pub fn stable_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.venue, self.wallet_address, self.client_order_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalIntentKind {
    Approve,
    Swap,
}

impl JournalIntentKind {
    const fn discriminant(self) -> u8 {
        match self {
            Self::Approve => 0,
            Self::Swap => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalEventStatus {
    Submitted,
    Accepted,
    Filled,
    Rejected,
}

impl JournalEventStatus {
    const fn rank(self) -> u8 {
        match self {
            Self::Submitted => 0,
            Self::Accepted => 1,
            Self::Filled | Self::Rejected => 2,
        }
    }

    const fn discriminant(self) -> u8 {
        match self {
            Self::Submitted => 0,
            Self::Accepted => 1,
            Self::Filled => 2,
            Self::Rejected => 3,
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Rejected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEvent {
    pub sequence: u64,
    pub ts_event_ns: u64,
    pub idempotency_key: OrderIdempotencyKey,
    pub intent_kind: JournalIntentKind,
    pub intent_hash: String,
    pub tx_hash: Option<String>,
    pub raw_tx_hash: Option<String>,
    pub reserved_nonce: Option<u64>,
    pub status: JournalEventStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalIntentState {
    pub intent_kind: JournalIntentKind,
    pub intent_hash: String,
    pub tx_hash: Option<String>,
    pub raw_tx_hash: Option<String>,
    pub reserved_nonce: Option<u64>,
    pub status: JournalEventStatus,
    pub sequence: u64,
    pub ts_event_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalOrderState {
    pub idempotency_key: OrderIdempotencyKey,
    pub status: JournalEventStatus,
    pub sequence: u64,
    pub terminal_tx_hash: Option<String>,
    pub intents: BTreeMap<String, JournalIntentState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateSubmitDisposition {
    New,
    NoOpInFlight,
    RejectTerminal,
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("I/O error for journal {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize journal event: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to parse journal line {line}: {source}")]
    Parse {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

pub fn append_event_jsonl(path: &Path, event: &JournalEvent) -> Result<(), JournalError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| JournalError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| JournalError::Io {
            path: path.display().to_string(),
            source,
        })?;

    let mut encoded = serde_json::to_string(event)?;
    encoded.push('\n');
    file.write_all(encoded.as_bytes())
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|source| JournalError::Io {
            path: path.display().to_string(),
            source,
        })?;

    Ok(())
}

pub fn load_events_jsonl(path: &Path) -> Result<Vec<JournalEvent>, JournalError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path).map_err(|source| JournalError::Io {
        path: path.display().to_string(),
        source,
    })?;

    let mut events = Vec::new();
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let has_trailing_newline = bytes.ends_with(b"\n");
    if has_trailing_newline && lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let line_str = String::from_utf8_lossy(line);
        match serde_json::from_str::<JournalEvent>(line_str.as_ref()) {
            Ok(event) => events.push(event),
            Err(source) => {
                let is_last = index + 1 == lines.len();
                if is_last && !has_trailing_newline {
                    break;
                }
                return Err(JournalError::Parse {
                    line: index + 1,
                    source,
                });
            }
        }
    }

    Ok(events)
}

#[must_use]
pub fn stable_sort_events(mut events: Vec<JournalEvent>) -> Vec<JournalEvent> {
    events.sort_by_key(journal_event_sort_key);
    events
}

#[must_use]
pub fn replay_events(events: &[JournalEvent]) -> BTreeMap<OrderIdempotencyKey, JournalOrderState> {
    let sorted = stable_sort_events(events.to_vec());
    let mut states = BTreeMap::new();
    let mut seen = HashSet::new();

    for event in sorted {
        let dedupe_key = journal_event_dedupe_key(&event);
        if !seen.insert(dedupe_key) {
            continue;
        }

        let state = states
            .entry(event.idempotency_key.clone())
            .or_insert_with(|| JournalOrderState {
                idempotency_key: event.idempotency_key.clone(),
                status: JournalEventStatus::Submitted,
                sequence: 0,
                terminal_tx_hash: None,
                intents: BTreeMap::new(),
            });

        apply_event(state, event);
    }

    states
}

#[must_use]
pub fn classify_duplicate_submit(state: Option<&JournalOrderState>) -> DuplicateSubmitDisposition {
    match state {
        None => DuplicateSubmitDisposition::New,
        Some(order_state) if order_state.status.is_terminal() => {
            DuplicateSubmitDisposition::RejectTerminal
        }
        Some(_) => DuplicateSubmitDisposition::NoOpInFlight,
    }
}

fn apply_event(state: &mut JournalOrderState, event: JournalEvent) {
    let intent_state = state
        .intents
        .entry(event.intent_hash.clone())
        .or_insert_with(|| JournalIntentState {
            intent_kind: event.intent_kind,
            intent_hash: event.intent_hash.clone(),
            tx_hash: None,
            raw_tx_hash: None,
            reserved_nonce: None,
            status: JournalEventStatus::Submitted,
            sequence: 0,
            ts_event_ns: 0,
        });

    let incoming_rank = event.status.rank();
    let current_intent_rank = intent_state.status.rank();
    let incoming_discriminant = event.status.discriminant();
    let current_discriminant = intent_state.status.discriminant();
    if incoming_rank > current_intent_rank
        || (incoming_rank == current_intent_rank
            && (event.sequence > intent_state.sequence
                || (event.sequence == intent_state.sequence
                    && incoming_discriminant > current_discriminant)))
    {
        intent_state.intent_kind = event.intent_kind;
        intent_state.tx_hash = event.tx_hash.clone();
        intent_state.raw_tx_hash = event.raw_tx_hash.clone();
        intent_state.reserved_nonce = event.reserved_nonce;
        intent_state.status = event.status;
        intent_state.sequence = event.sequence;
        intent_state.ts_event_ns = event.ts_event_ns;
    }

    refresh_order_summary(state);
}

fn refresh_order_summary(state: &mut JournalOrderState) {
    let Some(latest_intent) = state.intents.values().max_by_key(|intent| {
        (
            intent.sequence,
            intent.ts_event_ns,
            intent.status.rank(),
            intent.status.discriminant(),
            intent.intent_kind.discriminant(),
            intent.intent_hash.clone(),
        )
    }) else {
        return;
    };

    state.status = latest_intent.status;
    state.sequence = latest_intent.sequence;
    state.terminal_tx_hash = if latest_intent.status.is_terminal() {
        latest_intent.tx_hash.clone()
    } else {
        None
    };
}

fn journal_event_sort_key(event: &JournalEvent) -> (String, u64, u64, u8, String, String, u8) {
    (
        event.idempotency_key.stable_key(),
        event.sequence,
        event.ts_event_ns,
        event.intent_kind.discriminant(),
        event.intent_hash.clone(),
        event.tx_hash.clone().unwrap_or_default(),
        event.status.discriminant(),
    )
}

fn journal_event_dedupe_key(event: &JournalEvent) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        event.idempotency_key.stable_key(),
        event.sequence,
        event.ts_event_ns,
        event.intent_kind.discriminant(),
        event.intent_hash,
        event.tx_hash.clone().unwrap_or_default(),
        event.raw_tx_hash.clone().unwrap_or_default(),
        event
            .reserved_nonce
            .map_or_else(String::new, |nonce| nonce.to_string()),
        event.status.discriminant(),
    )
}

impl fmt::Display for JournalIntentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approve => write!(f, "approve"),
            Self::Swap => write!(f, "swap"),
        }
    }
}
