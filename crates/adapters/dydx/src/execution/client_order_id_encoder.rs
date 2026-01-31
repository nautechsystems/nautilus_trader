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

//! Bidirectional client order ID encoder for dYdX.
//!
//! dYdX chain requires u32 client IDs, but Nautilus uses string-based `ClientOrderId`.
//! This module provides a robust encoder that:
//! - Maps Nautilus ClientOrderId → dYdX u32 (forward)
//! - Maps dYdX u32 → Nautilus ClientOrderId (reverse)
//! - Uses sequential allocation to guarantee no collisions
//! - Includes overflow protection

use std::sync::atomic::{AtomicU32, Ordering};

use dashmap::{mapref::entry::Entry, DashMap};
use nautilus_model::identifiers::ClientOrderId;
use thiserror::Error;

/// Maximum safe client order ID value before warning about overflow.
/// Leave room for ~1000 additional orders after reaching this threshold.
pub const MAX_SAFE_CLIENT_ID: u32 = u32::MAX - 1000;

/// Error type for client order ID encoding operations.
#[derive(Debug, Clone, Error)]
pub enum EncoderError {
    /// The encoder has reached the maximum safe client ID value.
    #[error("Client order ID counter overflow: current value {0} exceeds safe limit {MAX_SAFE_CLIENT_ID}")]
    CounterOverflow(u32),
}

/// Manages bidirectional mapping of ClientOrderId ↔ u32 for dYdX.
///
/// dYdX chain requires u32 client IDs, but Nautilus uses string-based `ClientOrderId`.
/// This encoder provides:
/// - **Forward mapping**: Nautilus ClientOrderId → dYdX u32
/// - **Reverse mapping**: dYdX u32 → Nautilus ClientOrderId
/// - **Sequential allocation**: Guarantees no collisions within session
///
/// # Session Scope
///
/// Mappings are kept in-memory only and not persisted across restarts.
/// After restart, the encoder starts fresh with counter at 1.
///
/// # Thread Safety
///
/// All operations are thread-safe using `DashMap` and `AtomicU32`.
#[derive(Debug)]
pub struct ClientOrderIdEncoder {
    /// Forward mapping: Nautilus ClientOrderId → dYdX u32
    forward: DashMap<ClientOrderId, u32>,
    /// Reverse mapping: dYdX u32 → Nautilus ClientOrderId
    reverse: DashMap<u32, ClientOrderId>,
    /// Next ID to allocate (starts at 1, never 0)
    next_id: AtomicU32,
}

impl Default for ClientOrderIdEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientOrderIdEncoder {
    /// Creates a new encoder with counter starting at 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            forward: DashMap::new(),
            reverse: DashMap::new(),
            next_id: AtomicU32::new(1),
        }
    }

    /// Encodes a ClientOrderId to u32, allocating a new ID if needed.
    ///
    /// # Encoding Rules
    ///
    /// 1. If already mapped, returns existing u32
    /// 2. If ClientOrderId is numeric (e.g., "12345"), parses and uses that value
    /// 3. Otherwise, allocates a new sequential u32
    ///
    /// # Errors
    ///
    /// Returns `EncoderError::CounterOverflow` if the counter exceeds `MAX_SAFE_CLIENT_ID`.
    pub fn encode(&self, id: ClientOrderId) -> Result<u32, EncoderError> {
        // Fast path: already mapped
        if let Some(existing) = self.forward.get(&id) {
            let u32_id = *existing.value();
            log::info!(
                "[ENCODER] encode() CACHE HIT: '{}' -> {} (already mapped)",
                id,
                u32_id
            );
            return Ok(u32_id);
        }

        // Try parsing as direct integer
        if let Ok(numeric_id) = id.as_str().parse::<u32>() {
            log::info!(
                "[ENCODER] encode() NUMERIC: '{}' -> {} (parsed directly as u32)",
                id,
                numeric_id
            );
            self.forward.insert(id, numeric_id);
            self.reverse.insert(numeric_id, id);
            return Ok(numeric_id);
        }

        // Check for overflow before allocating
        let current = self.next_id.load(Ordering::Relaxed);
        if current >= MAX_SAFE_CLIENT_ID {
            log::error!(
                "[ENCODER] encode() OVERFLOW: counter {} >= MAX_SAFE {}",
                current,
                MAX_SAFE_CLIENT_ID
            );
            return Err(EncoderError::CounterOverflow(current));
        }

        // Allocate new sequential ID
        // Use entry API to handle race conditions
        match self.forward.entry(id) {
            Entry::Occupied(entry) => {
                let u32_id = *entry.get();
                log::info!(
                    "[ENCODER] encode() RACE HIT: '{}' -> {} (concurrent insert)",
                    id,
                    u32_id
                );
                Ok(u32_id)
            }
            Entry::Vacant(vacant) => {
                let new_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                log::info!(
                    "[ENCODER] encode() NEW ALLOC: '{}' -> {} (sequential, counter now={})",
                    id,
                    new_id,
                    self.next_id.load(Ordering::Relaxed)
                );
                vacant.insert(new_id);
                self.reverse.insert(new_id, id);
                Ok(new_id)
            }
        }
    }

    /// Decodes a u32 back to its original ClientOrderId.
    ///
    /// Returns `None` if no mapping exists for this u32.
    #[must_use]
    pub fn decode(&self, dydx_id: u32) -> Option<ClientOrderId> {
        let result = self.reverse.get(&dydx_id).map(|r| *r.value());
        match &result {
            Some(client_id) => {
                log::info!(
                    "[ENCODER] decode() FOUND: {} -> '{}'",
                    dydx_id,
                    client_id
                );
            }
            None => {
                log::warn!(
                    "[ENCODER] decode() NOT FOUND: {} (no mapping exists)",
                    dydx_id
                );
            }
        }
        result
    }

    /// Gets the existing u32 mapping without allocating a new one.
    ///
    /// Returns `None` if no mapping exists for this ClientOrderId.
    #[must_use]
    pub fn get(&self, id: &ClientOrderId) -> Option<u32> {
        // Try parsing first (numeric IDs may not be in the map)
        if let Ok(numeric_id) = id.as_str().parse::<u32>() {
            log::debug!(
                "[ENCODER] get() NUMERIC: '{}' -> {} (parsed directly)",
                id,
                numeric_id
            );
            return Some(numeric_id);
        }
        let result = self.forward.get(id).map(|r| *r.value());
        match result {
            Some(u32_id) => {
                log::debug!(
                    "[ENCODER] get() FOUND: '{}' -> {}",
                    id,
                    u32_id
                );
            }
            None => {
                log::debug!(
                    "[ENCODER] get() NOT FOUND: '{}' (no mapping)",
                    id
                );
            }
        }
        result
    }

    /// Removes the mapping for a dYdX u32 client ID.
    ///
    /// Returns the original ClientOrderId if it was mapped.
    /// Use this to clean up after orders reach terminal states.
    pub fn remove(&self, dydx_id: u32) -> Option<ClientOrderId> {
        if let Some((_, client_id)) = self.reverse.remove(&dydx_id) {
            self.forward.remove(&client_id);
            log::info!(
                "[ENCODER] remove() CLEANED: {} <-> '{}' (mappings removed, len={})",
                dydx_id,
                client_id,
                self.forward.len()
            );
            Some(client_id)
        } else {
            log::debug!(
                "[ENCODER] remove() NOOP: {} (no mapping to remove)",
                dydx_id
            );
            None
        }
    }

    /// Updates the forward mapping to point to a new u32.
    ///
    /// Used during modify_order when a new client ID is assigned.
    /// Returns the new u32 ID allocated.
    ///
    /// # Errors
    ///
    /// Returns `EncoderError::CounterOverflow` if the counter exceeds `MAX_SAFE_CLIENT_ID`.
    pub fn update_mapping(&self, id: ClientOrderId, old_dydx_id: u32) -> Result<u32, EncoderError> {
        // Check for overflow before allocating
        let current = self.next_id.load(Ordering::Relaxed);
        if current >= MAX_SAFE_CLIENT_ID {
            log::error!(
                "[ENCODER] update_mapping() OVERFLOW: counter {} >= MAX_SAFE {}",
                current,
                MAX_SAFE_CLIENT_ID
            );
            return Err(EncoderError::CounterOverflow(current));
        }

        // Remove old reverse mapping
        self.reverse.remove(&old_dydx_id);

        // Allocate new ID
        let new_id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Update forward mapping
        self.forward.insert(id, new_id);
        // Add new reverse mapping
        self.reverse.insert(new_id, id);

        log::info!(
            "[ENCODER] update_mapping() MODIFY: '{}' old={} -> new={} (for modify_order)",
            id,
            old_dydx_id,
            new_id
        );

        Ok(new_id)
    }

    /// Returns the current counter value (for debugging/monitoring).
    #[must_use]
    pub fn current_counter(&self) -> u32 {
        self.next_id.load(Ordering::Relaxed)
    }

    /// Returns the number of mappings currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    /// Returns true if no mappings are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_encode_numeric_id() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("12345");

        let result = encoder.encode(id);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 12345);
    }

    #[rstest]
    fn test_encode_string_id_allocates_sequential() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let result = encoder.encode(id);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1); // First allocation
    }

    #[rstest]
    fn test_encode_same_id_returns_same_value() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let first = encoder.encode(id).unwrap();
        let second = encoder.encode(id).unwrap();

        assert_eq!(first, second);
        assert_eq!(encoder.len(), 1);
    }

    #[rstest]
    fn test_encode_different_ids_returns_different_values() {
        let encoder = ClientOrderIdEncoder::new();
        let id1 = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");
        let id2 = ClientOrderId::from("O-20240115-120001-TRADER-STRAT-002");

        let result1 = encoder.encode(id1).unwrap();
        let result2 = encoder.encode(id2).unwrap();

        assert_ne!(result1, result2);
        assert_eq!(encoder.len(), 2);
    }

    #[rstest]
    fn test_decode_returns_original_id() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let encoded = encoder.encode(id).unwrap();
        let decoded = encoder.decode(encoded);

        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_decode_numeric_id() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("12345");

        let encoded = encoder.encode(id).unwrap();
        let decoded = encoder.decode(encoded);

        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_decode_unknown_returns_none() {
        let encoder = ClientOrderIdEncoder::new();
        let decoded = encoder.decode(99999);
        assert!(decoded.is_none());
    }

    #[rstest]
    fn test_get_existing_mapping() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let encoded = encoder.encode(id).unwrap();
        let got = encoder.get(&id);

        assert_eq!(got, Some(encoded));
    }

    #[rstest]
    fn test_get_numeric_without_encoding() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("12345");

        // get() should parse numeric IDs even without prior encode()
        let got = encoder.get(&id);
        assert_eq!(got, Some(12345));
    }

    #[rstest]
    fn test_get_unknown_returns_none() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let got = encoder.get(&id);
        assert!(got.is_none());
    }

    #[rstest]
    fn test_remove_mapping() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let encoded = encoder.encode(id).unwrap();
        assert_eq!(encoder.len(), 1);

        let removed = encoder.remove(encoded);
        assert_eq!(removed, Some(id));
        assert_eq!(encoder.len(), 0);

        // Both forward and reverse should be gone
        assert!(encoder.decode(encoded).is_none());
        assert!(encoder.get(&id).is_none());
    }

    #[rstest]
    fn test_remove_unknown_returns_none() {
        let encoder = ClientOrderIdEncoder::new();
        let removed = encoder.remove(99999);
        assert!(removed.is_none());
    }

    #[rstest]
    fn test_update_mapping() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20240115-120000-TRADER-STRAT-001");

        let old_encoded = encoder.encode(id).unwrap();
        assert_eq!(old_encoded, 1);

        let new_encoded = encoder.update_mapping(id, old_encoded).unwrap();
        assert_eq!(new_encoded, 2);

        // Forward should point to new value
        assert_eq!(encoder.get(&id), Some(new_encoded));

        // Old reverse mapping should be gone
        assert!(encoder.decode(old_encoded).is_none());

        // New reverse mapping should work
        assert_eq!(encoder.decode(new_encoded), Some(id));
    }

    #[rstest]
    fn test_sequential_allocation() {
        let encoder = ClientOrderIdEncoder::new();

        let ids: Vec<ClientOrderId> = (0..10)
            .map(|i| ClientOrderId::from(format!("ORDER-{i}").as_str()))
            .collect();

        let encoded: Vec<u32> = ids
            .iter()
            .map(|id| encoder.encode(*id).unwrap())
            .collect();

        // Should be sequential 1, 2, 3, ...
        for (i, val) in encoded.iter().enumerate() {
            assert_eq!(*val, (i + 1) as u32);
        }
    }

    #[rstest]
    fn test_uuid_encoding() {
        let encoder = ClientOrderIdEncoder::new();
        let uuid_id = ClientOrderId::from("550e8400-e29b-41d4-a716-446655440000");

        let encoded = encoder.encode(uuid_id).unwrap();
        let decoded = encoder.decode(encoded);

        assert_eq!(decoded, Some(uuid_id));
    }

    #[rstest]
    fn test_current_counter() {
        let encoder = ClientOrderIdEncoder::new();
        assert_eq!(encoder.current_counter(), 1);

        encoder
            .encode(ClientOrderId::from("ORDER-1"))
            .unwrap();
        assert_eq!(encoder.current_counter(), 2);

        encoder
            .encode(ClientOrderId::from("ORDER-2"))
            .unwrap();
        assert_eq!(encoder.current_counter(), 3);
    }

    #[rstest]
    fn test_is_empty() {
        let encoder = ClientOrderIdEncoder::new();
        assert!(encoder.is_empty());

        encoder
            .encode(ClientOrderId::from("ORDER-1"))
            .unwrap();
        assert!(!encoder.is_empty());
    }
}
