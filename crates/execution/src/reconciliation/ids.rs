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

//! Deterministic ID generation for reconciliation.
//!
//! These helpers hash the logical fill/order fields so that replayed reconciliation
//! events (e.g. after a process restart) produce the same `TradeId` and
//! `VenueOrderId` as the original run. That stability is what lets the engine's
//! duplicate-fill sanitizer dedupe replays instead of treating them as new events.

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    types::{Price, Quantity},
};
use uuid::Uuid;

use super::types::FillSnapshot;

// FNV-1a 64-bit constants (see http://www.isthe.com/chongo/tech/comp/fnv/).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// Create a synthetic `VenueOrderId` for a derived fill.
///
/// The suffix is hashed from the fill fields and instrument so distinct fills never collide,
/// and the same logical fill always yields the same `VenueOrderId` across restarts.
///
/// Format: `S-{hex_timestamp}-{hash_suffix}`
#[must_use]
pub fn create_synthetic_venue_order_id(
    fill: &FillSnapshot,
    instrument_id: InstrumentId,
) -> VenueOrderId {
    let hash_suffix = synthetic_fill_id_suffix("venue_order", fill, Some(instrument_id));
    let venue_order_id_value = format!("S-{:x}-{hash_suffix:08x}", fill.ts_event);
    VenueOrderId::new(&venue_order_id_value)
}

/// Create a synthetic `TradeId` using stable fill fields.
///
/// Format: `S-{hex_timestamp}-{hash_suffix}`
#[must_use]
pub fn create_synthetic_trade_id(fill: &FillSnapshot) -> TradeId {
    let hash_suffix = synthetic_fill_id_suffix("trade", fill, None);
    let trade_id_value = format!("S-{:x}-{hash_suffix:08x}", fill.ts_event);
    TradeId::new(&trade_id_value)
}

/// Create a deterministic `TradeId` for an inferred reconciliation fill.
///
/// The `account_id` scopes the ID to the venue account, preventing cross-account
/// collisions on venues where `venue_order_id` is only account-unique. The `ts_last`
/// (venue-provided) differentiates successive reconciliation incidents with the same
/// shape while keeping cross-restart replays deterministic.
#[must_use]
#[expect(clippy::too_many_arguments)]
pub fn create_inferred_reconciliation_trade_id(
    account_id: AccountId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    order_side: OrderSide,
    order_type: OrderType,
    filled_qty: Quantity,
    last_qty: Quantity,
    last_px: Price,
    position_id: PositionId,
    ts_last: UnixNanos,
) -> TradeId {
    let mut seed = String::from("reconciliation-fill");
    append_seed_part(&mut seed, account_id.as_str());
    append_seed_part(&mut seed, &instrument_id.to_string());
    append_seed_part(&mut seed, client_order_id.as_str());
    append_seed_part(
        &mut seed,
        venue_order_id.as_ref().map_or("", |value| value.as_ref()),
    );
    append_seed_part(&mut seed, order_side.as_ref());
    append_seed_part(&mut seed, order_type.as_ref());
    append_seed_part(&mut seed, &filled_qty.to_string());
    append_seed_part(&mut seed, &last_qty.to_string());
    append_seed_part(&mut seed, &last_px.to_string());
    append_seed_part(&mut seed, position_id.as_str());
    append_seed_part(&mut seed, &ts_last.as_u64().to_string());

    TradeId::new(deterministic_uuid_from_seed("reconciliation-fill", &seed))
}

/// The `account_id` scopes the ID to the venue account, preventing cross-account
/// collisions where the engine would otherwise fall back to `ClientOrderId::from(venue_order_id)`
/// and conflate orders from different accounts. The `ts_last` (venue-provided) ensures that
/// successive reconciliation incidents with the same shape get distinct IDs, while the same
/// logical event replayed after restart still hashes the same (venue re-reports identical ts).
#[must_use]
#[expect(clippy::too_many_arguments)]
pub fn create_position_reconciliation_venue_order_id(
    account_id: AccountId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    order_type: OrderType,
    quantity: Quantity,
    price: Option<Price>,
    venue_position_id: Option<PositionId>,
    tag: Option<&str>,
    ts_last: UnixNanos,
) -> VenueOrderId {
    let mut seed = String::from("position-reconciliation-order");
    append_seed_part(&mut seed, account_id.as_str());
    append_seed_part(&mut seed, &instrument_id.to_string());
    append_seed_part(&mut seed, order_side.as_ref());
    append_seed_part(&mut seed, order_type.as_ref());
    append_seed_part(&mut seed, &quantity.to_string());
    append_seed_part(
        &mut seed,
        &price.map_or_else(String::new, |value| value.to_string()),
    );
    append_seed_part(
        &mut seed,
        &venue_position_id.map_or_else(String::new, |value| value.to_string()),
    );
    append_seed_part(&mut seed, tag.unwrap_or(""));
    append_seed_part(&mut seed, &ts_last.as_u64().to_string());

    VenueOrderId::new(deterministic_uuid_from_seed(
        "position-reconciliation-order",
        &seed,
    ))
}

fn synthetic_fill_id_suffix(
    namespace: &str,
    fill: &FillSnapshot,
    instrument_id: Option<InstrumentId>,
) -> u32 {
    let mut hash: u64 = FNV_OFFSET_BASIS;

    update_synthetic_fill_hash(&mut hash, namespace.as_bytes());
    if let Some(instrument_id) = instrument_id {
        update_synthetic_fill_hash(&mut hash, instrument_id.to_string().as_bytes());
    }
    update_synthetic_fill_hash(&mut hash, &fill.ts_event.to_le_bytes());
    update_synthetic_fill_hash(&mut hash, fill.venue_order_id.as_str().as_bytes());
    update_synthetic_fill_hash(&mut hash, order_side_tag(fill.side).as_bytes());
    update_synthetic_fill_hash(&mut hash, fill.qty.to_string().as_bytes());
    update_synthetic_fill_hash(&mut hash, fill.px.to_string().as_bytes());

    hash as u32
}

fn deterministic_uuid_from_seed(namespace: &str, seed: &str) -> String {
    let primary = stable_hash64(namespace.as_bytes(), &[seed.as_bytes()]);
    let secondary = stable_hash64(b"uuid-alt", &[namespace.as_bytes(), seed.as_bytes()]);
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&primary.to_be_bytes());
    bytes[8..].copy_from_slice(&secondary.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes).to_string()
}

fn stable_hash64(namespace: &[u8], parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = FNV_OFFSET_BASIS;

    update_synthetic_fill_hash(&mut hash, namespace);

    for part in parts {
        update_synthetic_fill_hash(&mut hash, part);
    }

    hash
}

fn append_seed_part(seed: &mut String, value: &str) {
    seed.push('|');
    seed.push_str(value);
}

fn update_synthetic_fill_hash(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(FNV_PRIME);
    }

    *hash ^= 0xff;
    *hash = hash.wrapping_mul(FNV_PRIME);
}

fn order_side_tag(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
        _ => "UNSPECIFIED",
    }
}
