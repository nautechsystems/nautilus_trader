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

//! True bidirectional client order ID encoder for dYdX.
//!
//! dYdX chain requires u32 client IDs, but Nautilus uses string-based `ClientOrderId`.
//! This module provides deterministic encoding that:
//! - Encodes the full ClientOrderId into (client_id, client_metadata) u32 pair
//! - Decodes back to the exact original ClientOrderId string
//! - Works across restarts without persisted state
//! - Enables reconciliation of orders from previous sessions
//!
//! # Encoding Scheme
//!
//! For O-format ClientOrderIds (`O-YYYYMMDD-HHMMSS-TTT-SSS-CCC`):
//! - `client_id` (32 bits): Seconds since base epoch (2020-01-01 00:00:00 UTC)
//! - `client_metadata` (32 bits): `[trader:10][strategy:10][count:12]`
//!
//! For numeric ClientOrderIds (e.g., "12345"):
//! - `client_id`: The parsed u32 value
//! - `client_metadata`: `DEFAULT_RUST_CLIENT_METADATA` (4) - legacy marker
//!
//! For non-standard formats:
//! - Falls back to sequential allocation with in-memory reverse mapping

use std::sync::atomic::{AtomicU32, Ordering};

use dashmap::{mapref::entry::Entry, DashMap};
use nautilus_model::identifiers::ClientOrderId;
use thiserror::Error;

/// Base epoch for timestamp encoding: 2020-01-01 00:00:00 UTC.
/// This gives us ~136 years of range with 32-bit seconds.
pub const DYDX_BASE_EPOCH: i64 = 1577836800;

/// Value used to identify legacy/numeric client IDs.
/// When `client_metadata == 4`, the client_id is treated as a literal numeric ID.
pub const DEFAULT_RUST_CLIENT_METADATA: u32 = 4;

/// Maximum safe client order ID value before warning about overflow.
/// Leave room for ~1000 additional orders after reaching this threshold.
pub const MAX_SAFE_CLIENT_ID: u32 = u32::MAX - 1000;

/// Bit positions for client_metadata packing.
const TRADER_SHIFT: u32 = 22; // Bits [31:22]
const STRATEGY_SHIFT: u32 = 12; // Bits [21:12]
const COUNT_MASK: u32 = 0xFFF; // Bits [11:0] = 12 bits
const TRADER_MASK: u32 = 0x3FF; // 10 bits
const STRATEGY_MASK: u32 = 0x3FF; // 10 bits

/// Sentinel value for client_id to indicate sequential allocation.
/// This timestamp (~year 2156) is effectively impossible for real orders.
const SEQUENTIAL_CLIENT_ID_SENTINEL: u32 = u32::MAX;

/// Encoded client order ID pair for dYdX.
///
/// dYdX provides two u32 fields that survive the full order lifecycle:
/// - `client_id`: Primary identifier (timestamp-based for O-format)
/// - `client_metadata`: Secondary identifier (identity bits for O-format)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedClientOrderId {
    /// Primary client ID for dYdX protocol.
    pub client_id: u32,
    /// Metadata field for encoding additional identity information.
    pub client_metadata: u32,
}

/// Error type for client order ID encoding operations.
#[derive(Debug, Clone, Error)]
pub enum EncoderError {
    /// The encoder has reached the maximum safe client ID value.
    #[error("Client order ID counter overflow: current value {0} exceeds safe limit {MAX_SAFE_CLIENT_ID}")]
    CounterOverflow(u32),

    /// Failed to parse the O-format ClientOrderId.
    #[error("Failed to parse O-format ClientOrderId: {0}")]
    ParseError(String),

    /// Value overflow in encoding (e.g., trader tag > 1023).
    #[error("Value overflow in encoding: {0}")]
    ValueOverflow(String),
}

/// Manages bidirectional mapping of ClientOrderId ↔ (client_id, client_metadata) for dYdX.
///
/// # Encoding Strategy
///
/// 1. **Numeric IDs** (e.g., "12345"): Encoded as `(12345, 4)` for backward compatibility
/// 2. **O-format IDs** (e.g., "O-20260131-174827-001-001-1"): Deterministically encoded
/// 3. **Other formats**: Sequential allocation with in-memory mapping
///
/// # Thread Safety
///
/// All operations are thread-safe using `DashMap` and `AtomicU32`.
#[derive(Debug)]
pub struct ClientOrderIdEncoder {
    /// Forward mapping for non-deterministic IDs: ClientOrderId → EncodedClientOrderId
    forward: DashMap<ClientOrderId, EncodedClientOrderId>,
    /// Reverse mapping for non-deterministic IDs: (client_id, client_metadata) → ClientOrderId
    reverse: DashMap<(u32, u32), ClientOrderId>,
    /// Next ID to allocate for sequential fallback (starts at 1, never 0)
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

    /// Encodes a ClientOrderId to (client_id, client_metadata) pair.
    ///
    /// # Encoding Rules
    ///
    /// 1. If already mapped in cache, returns existing encoded pair
    /// 2. If numeric (e.g., "12345"), returns `(12345, DEFAULT_RUST_CLIENT_METADATA)`
    /// 3. If O-format, deterministically encodes timestamp + identity bits
    /// 4. Otherwise, allocates sequential ID for fallback
    ///
    /// # Errors
    ///
    /// Returns `EncoderError::CounterOverflow` if sequential counter exceeds safe limit.
    /// Returns `EncoderError::ValueOverflow` if O-format values exceed bit limits.
    pub fn encode(&self, id: ClientOrderId) -> Result<EncodedClientOrderId, EncoderError> {
        // Fast path: already mapped (for non-deterministic IDs)
        if let Some(existing) = self.forward.get(&id) {
            let encoded = *existing.value();
            log::debug!(
                "[ENCODER] encode() CACHE HIT: '{}' -> ({}, {})",
                id,
                encoded.client_id,
                encoded.client_metadata
            );
            return Ok(encoded);
        }

        let id_str = id.as_str();

        // Try parsing as direct integer (backward compatible)
        if let Ok(numeric_id) = id_str.parse::<u32>() {
            let encoded = EncodedClientOrderId {
                client_id: numeric_id,
                client_metadata: DEFAULT_RUST_CLIENT_METADATA,
            };
            log::info!(
                "[ENCODER] encode() NUMERIC: '{}' -> ({}, {}) [legacy format]",
                id,
                encoded.client_id,
                encoded.client_metadata
            );
            // Cache for reverse lookup
            self.forward.insert(id, encoded);
            self.reverse.insert((encoded.client_id, encoded.client_metadata), id);
            return Ok(encoded);
        }

        // Try O-format deterministic encoding
        if id_str.starts_with("O-") {
            match self.encode_o_format(id_str) {
                Ok(encoded) => {
                    log::info!(
                        "[ENCODER] encode() O-FORMAT: '{}' -> ({}, {:#010x})",
                        id,
                        encoded.client_id,
                        encoded.client_metadata
                    );
                    // No need to cache - deterministic
                    return Ok(encoded);
                }
                Err(e) => {
                    log::warn!(
                        "[ENCODER] O-format parse failed for '{}': {}, falling back to sequential",
                        id,
                        e
                    );
                    // Fall through to sequential allocation
                }
            }
        }

        // Fallback: sequential allocation for non-standard formats
        self.allocate_sequential(id)
    }

    /// Encodes an O-format ClientOrderId deterministically.
    ///
    /// Format: `O-YYYYMMDD-HHMMSS-TTT-SSS-CCC`
    /// - YYYYMMDD: Date
    /// - HHMMSS: Time
    /// - TTT: Trader tag (001-999)
    /// - SSS: Strategy tag (001-999)
    /// - CCC: Count (1-4095)
    fn encode_o_format(&self, id_str: &str) -> Result<EncodedClientOrderId, EncoderError> {
        // Parse: O-YYYYMMDD-HHMMSS-TTT-SSS-CCC
        let parts: Vec<&str> = id_str.split('-').collect();
        if parts.len() != 6 || parts[0] != "O" {
            return Err(EncoderError::ParseError(format!(
                "Expected O-YYYYMMDD-HHMMSS-TTT-SSS-CCC, got: {}",
                id_str
            )));
        }

        let date_str = parts[1]; // YYYYMMDD
        let time_str = parts[2]; // HHMMSS
        let trader_str = parts[3]; // TTT
        let strategy_str = parts[4]; // SSS
        let count_str = parts[5]; // CCC

        // Validate lengths
        if date_str.len() != 8 || time_str.len() != 6 {
            return Err(EncoderError::ParseError(format!(
                "Invalid date/time format in: {}",
                id_str
            )));
        }

        // Parse datetime components
        let year: i32 = date_str[0..4]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid year in: {}", id_str)))?;
        let month: u32 = date_str[4..6]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid month in: {}", id_str)))?;
        let day: u32 = date_str[6..8]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid day in: {}", id_str)))?;
        let hour: u32 = time_str[0..2]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid hour in: {}", id_str)))?;
        let minute: u32 = time_str[2..4]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid minute in: {}", id_str)))?;
        let second: u32 = time_str[4..6]
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid second in: {}", id_str)))?;

        // Parse identity components
        let trader: u32 = trader_str
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid trader in: {}", id_str)))?;
        let strategy: u32 = strategy_str
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid strategy in: {}", id_str)))?;
        let count: u32 = count_str
            .parse()
            .map_err(|_| EncoderError::ParseError(format!("Invalid count in: {}", id_str)))?;

        // Validate ranges
        if trader > TRADER_MASK {
            return Err(EncoderError::ValueOverflow(format!(
                "Trader tag {} exceeds max {}",
                trader, TRADER_MASK
            )));
        }
        if strategy > STRATEGY_MASK {
            return Err(EncoderError::ValueOverflow(format!(
                "Strategy tag {} exceeds max {}",
                strategy, STRATEGY_MASK
            )));
        }
        if count > COUNT_MASK {
            return Err(EncoderError::ValueOverflow(format!(
                "Count {} exceeds max {}",
                count, COUNT_MASK
            )));
        }

        // Convert to Unix timestamp
        let dt = chrono::NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, minute, second))
            .ok_or_else(|| {
                EncoderError::ParseError(format!("Invalid datetime in: {}", id_str))
            })?;

        let timestamp = dt.and_utc().timestamp();

        // Encode client_id as seconds since base epoch
        let seconds_since_epoch = timestamp - DYDX_BASE_EPOCH;
        if seconds_since_epoch < 0 {
            return Err(EncoderError::ValueOverflow(format!(
                "Timestamp {} is before base epoch {}",
                timestamp, DYDX_BASE_EPOCH
            )));
        }
        let client_id = seconds_since_epoch as u32;

        // Encode client_metadata: [trader:10][strategy:10][count:12]
        let client_metadata =
            (trader << TRADER_SHIFT) | (strategy << STRATEGY_SHIFT) | (count & COUNT_MASK);

        Ok(EncodedClientOrderId {
            client_id,
            client_metadata,
        })
    }

    /// Allocates a sequential ID for non-standard formats.
    fn allocate_sequential(&self, id: ClientOrderId) -> Result<EncodedClientOrderId, EncoderError> {
        // Check for overflow before allocating
        let current = self.next_id.load(Ordering::Relaxed);
        if current >= MAX_SAFE_CLIENT_ID {
            log::error!(
                "[ENCODER] allocate_sequential() OVERFLOW: counter {} >= MAX_SAFE {}",
                current,
                MAX_SAFE_CLIENT_ID
            );
            return Err(EncoderError::CounterOverflow(current));
        }

        // Use entry API to handle race conditions
        match self.forward.entry(id) {
            Entry::Occupied(entry) => {
                let encoded = *entry.get();
                log::debug!(
                    "[ENCODER] allocate_sequential() RACE HIT: '{}' -> ({}, {})",
                    id,
                    encoded.client_id,
                    encoded.client_metadata
                );
                Ok(encoded)
            }
            Entry::Vacant(vacant) => {
                let counter = self.next_id.fetch_add(1, Ordering::Relaxed);
                // Use SEQUENTIAL_CLIENT_ID_SENTINEL in client_id to mark sequential allocation
                // The counter goes in client_metadata for uniqueness
                let encoded = EncodedClientOrderId {
                    client_id: SEQUENTIAL_CLIENT_ID_SENTINEL,
                    client_metadata: counter,
                };
                log::info!(
                    "[ENCODER] allocate_sequential() NEW: '{}' -> ({:#x}, {}) [sequential]",
                    id,
                    encoded.client_id,
                    encoded.client_metadata
                );
                vacant.insert(encoded);
                self.reverse
                    .insert((encoded.client_id, encoded.client_metadata), id);
                Ok(encoded)
            }
        }
    }

    /// Decodes (client_id, client_metadata) back to the original ClientOrderId.
    ///
    /// # Decoding Rules
    ///
    /// 1. If `client_metadata == DEFAULT_RUST_CLIENT_METADATA (4)`: Return numeric string
    /// 2. If `client_metadata >= 1_000_000`: Look up in sequential reverse mapping
    /// 3. Otherwise: Decode as O-format using timestamp + identity bits
    ///
    /// Returns `None` if decoding fails (e.g., sequential ID not in cache).
    #[must_use]
    pub fn decode(&self, client_id: u32, client_metadata: u32) -> Option<ClientOrderId> {
        // Legacy numeric IDs
        if client_metadata == DEFAULT_RUST_CLIENT_METADATA {
            let id = ClientOrderId::from(client_id.to_string().as_str());
            log::debug!(
                "[ENCODER] decode() NUMERIC: ({}, {}) -> '{}' [legacy]",
                client_id,
                client_metadata,
                id
            );
            return Some(id);
        }

        // Sequential allocation marker
        if client_metadata >= 1_000_000 {
            let result = self
                .reverse
                .get(&(client_id, client_metadata))
                .map(|r| *r.value());
            match &result {
                Some(id) => {
                    log::debug!(
                        "[ENCODER] decode() SEQUENTIAL: ({}, {}) -> '{}'",
                        client_id,
                        client_metadata,
                        id
                    );
                }
                None => {
                    log::warn!(
                        "[ENCODER] decode() SEQUENTIAL NOT FOUND: ({}, {}) [mapping lost after restart]",
                        client_id,
                        client_metadata
                    );
                }
            }
            return result;
        }

        // O-format decoding
        self.decode_o_format(client_id, client_metadata)
    }

    /// Decodes O-format encoded values back to ClientOrderId string.
    fn decode_o_format(&self, client_id: u32, client_metadata: u32) -> Option<ClientOrderId> {
        // Extract identity components from client_metadata
        let trader = (client_metadata >> TRADER_SHIFT) & TRADER_MASK;
        let strategy = (client_metadata >> STRATEGY_SHIFT) & STRATEGY_MASK;
        let count = client_metadata & COUNT_MASK;

        // Convert client_id back to timestamp
        let timestamp = (client_id as i64) + DYDX_BASE_EPOCH;

        // Convert to datetime
        let dt = chrono::DateTime::from_timestamp(timestamp, 0)?;

        // Format: O-YYYYMMDD-HHMMSS-TTT-SSS-CCC
        let id_str = format!(
            "O-{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}-{:03}-{}",
            dt.year(),
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
            trader,
            strategy,
            count
        );

        let id = ClientOrderId::from(id_str.as_str());
        log::debug!(
            "[ENCODER] decode() O-FORMAT: ({}, {:#010x}) -> '{}'",
            client_id,
            client_metadata,
            id
        );
        Some(id)
    }

    /// Gets the existing encoded pair without allocating a new one.
    ///
    /// For deterministic formats (numeric, O-format), computes the encoding.
    /// For non-standard formats, looks up in the cache.
    #[must_use]
    pub fn get(&self, id: &ClientOrderId) -> Option<EncodedClientOrderId> {
        let id_str = id.as_str();

        // Try parsing as numeric
        if let Ok(numeric_id) = id_str.parse::<u32>() {
            return Some(EncodedClientOrderId {
                client_id: numeric_id,
                client_metadata: DEFAULT_RUST_CLIENT_METADATA,
            });
        }

        // Try O-format encoding
        if id_str.starts_with("O-") {
            if let Ok(encoded) = self.encode_o_format(id_str) {
                return Some(encoded);
            }
        }

        // Look up in forward mapping
        self.forward.get(id).map(|r| *r.value())
    }

    /// Removes the mapping for a given encoded pair.
    ///
    /// Returns the original ClientOrderId if it was mapped.
    /// For deterministic formats, this is a no-op.
    pub fn remove(&self, client_id: u32, client_metadata: u32) -> Option<ClientOrderId> {
        // Sequential allocations need cleanup
        if client_metadata >= 1_000_000 {
            if let Some((_, client_order_id)) = self.reverse.remove(&(client_id, client_metadata)) {
                self.forward.remove(&client_order_id);
                log::info!(
                    "[ENCODER] remove() CLEANED: ({}, {}) <-> '{}' (sequential mapping removed)",
                    client_id,
                    client_metadata,
                    client_order_id
                );
                return Some(client_order_id);
            }
        }

        // Numeric IDs cached for reverse lookup
        if client_metadata == DEFAULT_RUST_CLIENT_METADATA {
            if let Some((_, client_order_id)) = self.reverse.remove(&(client_id, client_metadata)) {
                self.forward.remove(&client_order_id);
                log::debug!(
                    "[ENCODER] remove() CLEANED: ({}, {}) <-> '{}' (numeric mapping removed)",
                    client_id,
                    client_metadata,
                    client_order_id
                );
                return Some(client_order_id);
            }
        }

        // O-format IDs are deterministic - just decode
        if client_metadata < 1_000_000 && client_metadata != DEFAULT_RUST_CLIENT_METADATA {
            return self.decode_o_format(client_id, client_metadata);
        }

        None
    }

    /// Legacy remove method for backward compatibility.
    /// Removes by client_id only, assumes DEFAULT_RUST_CLIENT_METADATA.
    pub fn remove_by_client_id(&self, client_id: u32) -> Option<ClientOrderId> {
        // Try with default metadata first
        if let result @ Some(_) = self.remove(client_id, DEFAULT_RUST_CLIENT_METADATA) {
            return result;
        }

        // Try to find in reverse map with any metadata
        let key_to_remove = self
            .reverse
            .iter()
            .find(|r| r.key().0 == client_id)
            .map(|r| *r.key());

        if let Some((cid, meta)) = key_to_remove {
            return self.remove(cid, meta);
        }

        None
    }

    /// Updates the forward mapping to point to a new encoded pair.
    ///
    /// Used during modify_order when a new client ID is assigned.
    /// Returns the new EncodedClientOrderId.
    ///
    /// # Errors
    ///
    /// Returns `EncoderError::CounterOverflow` if sequential counter exceeds safe limit.
    pub fn update_mapping(
        &self,
        id: ClientOrderId,
        old_client_id: u32,
        old_client_metadata: u32,
    ) -> Result<EncodedClientOrderId, EncoderError> {
        // Remove old mappings
        self.reverse.remove(&(old_client_id, old_client_metadata));
        self.forward.remove(&id);

        // Allocate new sequential ID (modify always uses sequential to avoid timestamp collision)
        let current = self.next_id.load(Ordering::Relaxed);
        if current >= MAX_SAFE_CLIENT_ID {
            return Err(EncoderError::CounterOverflow(current));
        }

        let new_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let encoded = EncodedClientOrderId {
            client_id: new_id,
            client_metadata: 1_000_000 + new_id,
        };

        self.forward.insert(id, encoded);
        self.reverse.insert((encoded.client_id, encoded.client_metadata), id);

        log::info!(
            "[ENCODER] update_mapping() MODIFY: '{}' ({}, {}) -> ({}, {}) [for modify_order]",
            id,
            old_client_id,
            old_client_metadata,
            encoded.client_id,
            encoded.client_metadata
        );

        Ok(encoded)
    }

    /// Returns the current counter value (for debugging/monitoring).
    #[must_use]
    pub fn current_counter(&self) -> u32 {
        self.next_id.load(Ordering::Relaxed)
    }

    /// Returns the number of non-deterministic mappings currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    /// Returns true if no non-deterministic mappings are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

// Add chrono traits for datetime handling
use chrono::{Datelike, Timelike};

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
        let encoded = result.unwrap();
        assert_eq!(encoded.client_id, 12345);
        assert_eq!(encoded.client_metadata, DEFAULT_RUST_CLIENT_METADATA);
    }

    #[rstest]
    fn test_encode_o_format() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20260131-174827-001-001-1");

        let result = encoder.encode(id);
        assert!(result.is_ok());
        let encoded = result.unwrap();

        // Verify timestamp encoding (seconds since 2020-01-01)
        // 2026-01-31 17:48:27 UTC
        let expected_timestamp = chrono::NaiveDate::from_ymd_opt(2026, 1, 31)
            .unwrap()
            .and_hms_opt(17, 48, 27)
            .unwrap()
            .and_utc()
            .timestamp();
        let expected_client_id = (expected_timestamp - DYDX_BASE_EPOCH) as u32;
        assert_eq!(encoded.client_id, expected_client_id);

        // Verify metadata encoding: [trader:10][strategy:10][count:12]
        // trader=1, strategy=1, count=1
        let expected_metadata = (1 << TRADER_SHIFT) | (1 << STRATEGY_SHIFT) | 1;
        assert_eq!(encoded.client_metadata, expected_metadata);
    }

    #[rstest]
    fn test_roundtrip_o_format() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20260131-174827-001-001-1");

        let encoded = encoder.encode(id).unwrap();
        let decoded = encoder.decode(encoded.client_id, encoded.client_metadata);

        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_roundtrip_o_format_various() {
        let encoder = ClientOrderIdEncoder::new();
        let test_cases = vec![
            "O-20260131-000000-001-001-1",
            "O-20260131-235959-999-999-4095",
            "O-20200101-000000-000-000-0",
            "O-20251215-123456-123-456-789",
        ];

        for id_str in test_cases {
            let id = ClientOrderId::from(id_str);
            let encoded = encoder.encode(id).unwrap();
            let decoded = encoder.decode(encoded.client_id, encoded.client_metadata);
            assert_eq!(
                decoded,
                Some(id),
                "Roundtrip failed for {}",
                id_str
            );
        }
    }

    #[rstest]
    fn test_roundtrip_numeric() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("12345");

        let encoded = encoder.encode(id).unwrap();
        let decoded = encoder.decode(encoded.client_id, encoded.client_metadata);

        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_encode_non_standard_uses_sequential() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("custom-order-id");

        let result = encoder.encode(id);
        assert!(result.is_ok());
        let encoded = result.unwrap();

        // Sequential allocation uses client_metadata >= 1_000_000
        assert!(encoded.client_metadata >= 1_000_000);
    }

    #[rstest]
    fn test_roundtrip_sequential() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("custom-order-id");

        let encoded = encoder.encode(id).unwrap();
        let decoded = encoder.decode(encoded.client_id, encoded.client_metadata);

        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_sequential_lost_after_restart() {
        // Simulate restart: new encoder without previous mappings
        let encoder1 = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("custom-order-id");

        let encoded = encoder1.encode(id).unwrap();

        // New encoder (simulating restart)
        let encoder2 = ClientOrderIdEncoder::new();
        let decoded = encoder2.decode(encoded.client_id, encoded.client_metadata);

        // Sequential mappings are lost after restart
        assert!(decoded.is_none());
    }

    #[rstest]
    fn test_o_format_survives_restart() {
        let encoder1 = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20260131-174827-001-001-1");

        let encoded = encoder1.encode(id).unwrap();

        // New encoder (simulating restart)
        let encoder2 = ClientOrderIdEncoder::new();
        let decoded = encoder2.decode(encoded.client_id, encoded.client_metadata);

        // O-format is deterministic - survives restart!
        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_get_without_encode() {
        let encoder = ClientOrderIdEncoder::new();

        // Numeric - should work without encode
        let numeric_id = ClientOrderId::from("12345");
        let got = encoder.get(&numeric_id);
        assert_eq!(
            got,
            Some(EncodedClientOrderId {
                client_id: 12345,
                client_metadata: DEFAULT_RUST_CLIENT_METADATA
            })
        );

        // O-format - should work without encode
        let o_id = ClientOrderId::from("O-20260131-174827-001-001-1");
        let got = encoder.get(&o_id);
        assert!(got.is_some());

        // Non-standard - requires encode first
        let custom_id = ClientOrderId::from("custom");
        let got = encoder.get(&custom_id);
        assert!(got.is_none());
    }

    #[rstest]
    fn test_remove_sequential() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("custom-order-id");

        let encoded = encoder.encode(id).unwrap();
        assert_eq!(encoder.len(), 1);

        let removed = encoder.remove(encoded.client_id, encoded.client_metadata);
        assert_eq!(removed, Some(id));
        assert_eq!(encoder.len(), 0);
    }

    #[rstest]
    fn test_update_mapping() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20260131-174827-001-001-1");

        let old_encoded = encoder.encode(id).unwrap();
        let new_encoded = encoder
            .update_mapping(id, old_encoded.client_id, old_encoded.client_metadata)
            .unwrap();

        // New encoding should be different
        assert_ne!(new_encoded.client_id, old_encoded.client_id);

        // Forward lookup should return new encoding
        let got = encoder.get(&id);
        assert_eq!(got, Some(new_encoded));

        // Old encoding should not decode to this ID anymore
        let decoded_old = encoder.decode(old_encoded.client_id, old_encoded.client_metadata);
        // O-format can still be decoded deterministically
        assert!(decoded_old.is_some());

        // New encoding should decode correctly
        let decoded_new = encoder.decode(new_encoded.client_id, new_encoded.client_metadata);
        assert_eq!(decoded_new, Some(id));
    }

    #[rstest]
    fn test_max_values_o_format() {
        let encoder = ClientOrderIdEncoder::new();
        // Max trader (1023), max strategy (1023), max count (4095)
        let id = ClientOrderId::from("O-20260131-235959-999-999-4095");

        let result = encoder.encode(id);
        assert!(result.is_ok());

        let encoded = result.unwrap();
        let decoded = encoder.decode(encoded.client_id, encoded.client_metadata);
        assert_eq!(decoded, Some(id));
    }

    #[rstest]
    fn test_overflow_trader_tag() {
        let encoder = ClientOrderIdEncoder::new();
        // Trader tag 1024 exceeds 10-bit limit (1023)
        let id = ClientOrderId::from("O-20260131-174827-1024-001-1");

        let result = encoder.encode(id);
        // Should fall back to sequential, not error
        assert!(result.is_ok());
        assert!(result.unwrap().client_metadata >= 1_000_000);
    }

    #[rstest]
    fn test_encode_same_id_returns_same_value() {
        let encoder = ClientOrderIdEncoder::new();
        let id = ClientOrderId::from("O-20260131-174827-001-001-1");

        let first = encoder.encode(id).unwrap();
        let second = encoder.encode(id).unwrap();

        assert_eq!(first, second);
    }

    #[rstest]
    fn test_encode_different_ids_returns_different_values() {
        let encoder = ClientOrderIdEncoder::new();
        let id1 = ClientOrderId::from("O-20260131-174827-001-001-1");
        let id2 = ClientOrderId::from("O-20260131-174828-001-001-2");

        let result1 = encoder.encode(id1).unwrap();
        let result2 = encoder.encode(id2).unwrap();

        assert_ne!(result1, result2);
    }

    #[rstest]
    fn test_current_counter() {
        let encoder = ClientOrderIdEncoder::new();
        assert_eq!(encoder.current_counter(), 1);

        encoder.encode(ClientOrderId::from("custom-1")).unwrap();
        assert_eq!(encoder.current_counter(), 2);

        encoder.encode(ClientOrderId::from("custom-2")).unwrap();
        assert_eq!(encoder.current_counter(), 3);

        // O-format doesn't increment counter
        encoder
            .encode(ClientOrderId::from("O-20260131-174827-001-001-1"))
            .unwrap();
        assert_eq!(encoder.current_counter(), 3);
    }

    #[rstest]
    fn test_is_empty() {
        let encoder = ClientOrderIdEncoder::new();
        assert!(encoder.is_empty());

        encoder.encode(ClientOrderId::from("custom")).unwrap();
        assert!(!encoder.is_empty());
    }
}
