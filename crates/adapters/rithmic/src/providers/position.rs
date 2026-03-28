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

//! Position state provider for Rithmic.
//!
//! The `RithmicPositionProvider` receives position updates from the PnL plant
//! via the `RithmicGateway`. It tracks per-instrument positions including
//! quantity, average price, and P&L calculations.

use dashmap::DashMap;
use std::{fmt::Debug, sync::Arc};
use tokio::task::JoinHandle;

use crate::{
    common::types::{ExchangeId, RithmicAccountId, RithmicSymbol, UnixNanos},
    error::Result,
    gateway::{PnlEvent, RithmicGateway},
};

/// Position information.
#[derive(Debug, Clone)]
pub struct Position {
    /// Whether the source notification was a Rithmic snapshot payload.
    pub is_snapshot: bool,
    /// Account ID.
    pub account_id: RithmicAccountId,
    /// Instrument symbol.
    pub symbol: RithmicSymbol,
    /// Exchange.
    pub exchange: ExchangeId,
    /// Net position quantity (positive=long, negative=short).
    pub quantity: f64,
    /// Average entry price.
    pub avg_price: f64,
    /// Unrealized PnL.
    pub unrealized_pnl: f64,
    /// Realized PnL.
    pub realized_pnl: f64,
    /// Timestamp.
    pub ts_event: UnixNanos,
}

impl Position {
    /// Returns true if this is a long position.
    pub fn is_long(&self) -> bool {
        self.quantity > 0.0
    }

    /// Returns true if this is a short position.
    pub fn is_short(&self) -> bool {
        self.quantity < 0.0
    }

    /// Returns true if position is flat (no position).
    pub fn is_flat(&self) -> bool {
        self.quantity == 0.0
    }

    /// Returns the absolute position size.
    pub fn abs_quantity(&self) -> f64 {
        self.quantity.abs()
    }
}

/// Position state event.
#[derive(Debug, Clone)]
pub enum PositionEvent {
    /// Position opened.
    Opened(Position),
    /// Position updated.
    Updated(Position),
    /// Position closed.
    Closed {
        account_id: String,
        symbol: String,
        exchange: String,
        realized_pnl: f64,
    },
    /// Error.
    Error(String),
}

/// Provides position state from Rithmic.
///
/// The provider receives position updates from the gateway's PnL plant
/// and maintains the current position state for all instruments. It tracks
/// position lifecycle (opened, updated, closed) and emits appropriate events.
pub struct RithmicPositionProvider {
    gateway: Arc<RithmicGateway>,
    account_id: String,
    positions: Arc<DashMap<String, Position>>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<PositionEvent>>,
    task_handle: Option<JoinHandle<()>>,
}

impl RithmicPositionProvider {
    /// Creates a new position provider connected to the given gateway.
    ///
    /// # Arguments
    /// * `gateway` - The connected Rithmic gateway
    /// * `account_id` - The account ID to track (usually from gateway config)
    pub fn new(gateway: Arc<RithmicGateway>, account_id: impl Into<String>) -> Self {
        Self {
            gateway,
            account_id: account_id.into(),
            positions: Arc::new(DashMap::new()),
            event_tx: None,
            task_handle: None,
        }
    }

    /// Returns the account ID.
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Returns a reference to the gateway.
    pub fn gateway(&self) -> &Arc<RithmicGateway> {
        &self.gateway
    }

    /// Starts receiving position updates from the gateway.
    ///
    /// This prepares the provider to process PnL events from the gateway.
    /// Call this after the gateway is connected.
    pub async fn start(&mut self) -> Result<()> {
        if self.task_handle.is_some() {
            tracing::warn!("Position provider already started");
            return Ok(());
        }

        // Note: The gateway already subscribes to PnL updates during connect().
        // Position updates come from the same PnL subscription as account updates.
        tracing::debug!("Position provider started for account {}", self.account_id);
        Ok(())
    }

    /// Stops receiving position updates.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        tracing::debug!("Position provider stopped");
        Ok(())
    }

    /// Returns position for a symbol.
    pub fn position(&self, symbol: &str, exchange: &str) -> Option<Position> {
        let key = format!("{exchange}:{symbol}");
        self.positions.get(&key).map(|r| r.clone())
    }

    /// Returns all open positions (non-flat).
    pub fn positions(&self) -> Vec<Position> {
        self.positions
            .iter()
            .filter(|r| !r.is_flat())
            .map(|r| r.clone())
            .collect()
    }

    /// Returns all positions (including flat).
    pub fn all_positions(&self) -> Vec<Position> {
        self.positions.iter().map(|r| r.clone()).collect()
    }

    /// Returns total unrealized PnL across all positions.
    pub fn total_unrealized_pnl(&self) -> f64 {
        self.positions.iter().map(|r| r.unrealized_pnl).sum()
    }

    /// Returns total realized PnL across all positions.
    pub fn total_realized_pnl(&self) -> f64 {
        self.positions.iter().map(|r| r.realized_pnl).sum()
    }

    /// Returns count of open positions.
    pub fn open_positions_count(&self) -> usize {
        self.positions.iter().filter(|r| !r.is_flat()).count()
    }

    /// Returns count of long positions.
    pub fn long_positions_count(&self) -> usize {
        self.positions.iter().filter(|r| r.is_long()).count()
    }

    /// Returns count of short positions.
    pub fn short_positions_count(&self) -> usize {
        self.positions.iter().filter(|r| r.is_short()).count()
    }

    /// Returns a receiver for position events.
    pub fn event_receiver(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<PositionEvent> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.event_tx = Some(tx);
        rx
    }

    /// Processes a PnL event from the gateway.
    ///
    /// This method should be called by the consumer that owns the gateway's
    /// PnL event receiver. It updates the provider's internal state and
    /// emits events to any registered listeners.
    pub fn process_pnl_event(&self, event: &PnlEvent) {
        if let PnlEvent::Position(position_event) = event {
            match position_event {
                PositionEvent::Updated(position) => {
                    // Filter by account if needed
                    if position.account_id != self.account_id {
                        return;
                    }
                    self.update_position(position.clone());
                }
                PositionEvent::Opened(position) => {
                    if position.account_id != self.account_id {
                        return;
                    }
                    self.update_position(position.clone());
                }
                PositionEvent::Closed {
                    account_id,
                    symbol,
                    exchange,
                    realized_pnl,
                } => {
                    if account_id != &self.account_id {
                        return;
                    }
                    // Remove the position
                    let key = format!("{exchange}:{symbol}");
                    self.positions.remove(&key);

                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(PositionEvent::Closed {
                            account_id: account_id.clone(),
                            symbol: symbol.clone(),
                            exchange: exchange.clone(),
                            realized_pnl: *realized_pnl,
                        });
                    }
                }
                PositionEvent::Error(err) => {
                    tracing::error!("Position error: {}", err);

                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(PositionEvent::Error(err.clone()));
                    }
                }
            }
        }
    }

    /// Updates position state (internal use).
    ///
    /// This method determines whether the position was opened, updated, or closed
    /// based on the previous state and emits the appropriate event.
    pub(crate) fn update_position(&self, position: Position) {
        let key = format!("{}:{}", position.exchange, position.symbol);
        let was_flat = self.positions.get(&key).is_none_or(|p| p.is_flat());
        let is_flat = position.is_flat();

        let event = if was_flat && !is_flat {
            // Position opened
            PositionEvent::Opened(position.clone())
        } else if !was_flat && is_flat {
            // Position closed
            PositionEvent::Closed {
                account_id: position.account_id.clone(),
                symbol: position.symbol.clone(),
                exchange: position.exchange.clone(),
                realized_pnl: position.realized_pnl,
            }
        } else if !is_flat {
            // Position updated (still open)
            PositionEvent::Updated(position.clone())
        } else {
            // Was flat, still flat - no event needed
            return;
        };

        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }

        if is_flat {
            // Remove flat positions from cache
            self.positions.remove(&key);
        } else {
            self.positions.insert(key, position);
        }
    }

    /// Clears all positions (for reset/reconnect).
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.positions.clear();
    }
}

impl Debug for RithmicPositionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RithmicPositionProvider))
            .field("account_id", &self.account_id)
            .field("open_positions", &self.open_positions_count())
            .field("long_positions", &self.long_positions_count())
            .field("short_positions", &self.short_positions_count())
            .field("total_unrealized_pnl", &self.total_unrealized_pnl())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RithmicEnv;
    use crate::gateway::GatewayConfig;

    fn create_test_gateway() -> Arc<RithmicGateway> {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "ACCOUNT123",
        );
        Arc::new(RithmicGateway::new(config))
    }

    #[rstest::rstest]
    fn test_position_provider_creation() {
        let gateway = create_test_gateway();
        let provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");
        assert_eq!(provider.account_id(), "ACCOUNT123");
        assert_eq!(provider.open_positions_count(), 0);
    }

    #[rstest::rstest]
    fn test_update_position() {
        let gateway = create_test_gateway();
        let provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");

        let position = Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: 5.0,
            avg_price: 4500.00,
            unrealized_pnl: 250.0,
            realized_pnl: 0.0,
            ts_event: 0,
        };

        provider.update_position(position);

        let retrieved = provider.position("ESZ4", "CME").unwrap();
        assert_eq!(retrieved.quantity, 5.0);
        assert!(retrieved.is_long());
        assert_eq!(provider.open_positions_count(), 1);
    }

    #[rstest::rstest]
    fn test_position_states() {
        let mut pos = Position {
            is_snapshot: false,
            account_id: "ACC".to_string(),
            symbol: "ES".to_string(),
            exchange: "CME".to_string(),
            quantity: 5.0,
            avg_price: 100.0,
            unrealized_pnl: 0.0,
            realized_pnl: 0.0,
            ts_event: 0,
        };

        assert!(pos.is_long());
        assert!(!pos.is_short());
        assert!(!pos.is_flat());

        pos.quantity = -3.0;
        assert!(!pos.is_long());
        assert!(pos.is_short());
        assert!(!pos.is_flat());

        pos.quantity = 0.0;
        assert!(!pos.is_long());
        assert!(!pos.is_short());
        assert!(pos.is_flat());
    }

    #[rstest::rstest]
    fn test_process_pnl_event_updates_position() {
        let gateway = create_test_gateway();
        let provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");

        let position = Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "NQZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: -2.0,
            avg_price: 18500.0,
            unrealized_pnl: -100.0,
            realized_pnl: 0.0,
            ts_event: 1704067200000000000,
        };

        let event = PnlEvent::Position(PositionEvent::Updated(position));
        provider.process_pnl_event(&event);

        let retrieved = provider.position("NQZ4", "CME").unwrap();
        assert_eq!(retrieved.quantity, -2.0);
        assert!(retrieved.is_short());
        assert_eq!(provider.short_positions_count(), 1);
        assert_eq!(provider.total_unrealized_pnl(), -100.0);
    }

    #[rstest::rstest]
    fn test_position_lifecycle_opened_closed() {
        let gateway = create_test_gateway();
        let mut provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");
        let mut rx = provider.event_receiver();

        // Open position
        let position = Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: 3.0,
            avg_price: 5000.0,
            unrealized_pnl: 150.0,
            realized_pnl: 0.0,
            ts_event: 0,
        };

        provider.update_position(position);
        assert_eq!(provider.open_positions_count(), 1);

        // Check that Opened event was emitted
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, PositionEvent::Opened(_)));

        // Close position (flat)
        let flat_position = Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: 0.0,
            avg_price: 0.0,
            unrealized_pnl: 0.0,
            realized_pnl: 300.0,
            ts_event: 1,
        };

        provider.update_position(flat_position);
        assert_eq!(provider.open_positions_count(), 0);

        // Check that Closed event was emitted
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, PositionEvent::Closed { .. }));
    }

    #[rstest::rstest]
    fn test_ignores_other_account_positions() {
        let gateway = create_test_gateway();
        let provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");

        let position = Position {
            is_snapshot: false,
            account_id: "OTHER_ACCOUNT".to_string(),
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: 10.0,
            avg_price: 5000.0,
            unrealized_pnl: 500.0,
            realized_pnl: 0.0,
            ts_event: 0,
        };

        let event = PnlEvent::Position(PositionEvent::Updated(position));
        provider.process_pnl_event(&event);

        // Should not add position for different account
        assert_eq!(provider.open_positions_count(), 0);
    }

    #[rstest::rstest]
    fn test_multiple_positions() {
        let gateway = create_test_gateway();
        let provider = RithmicPositionProvider::new(gateway, "ACCOUNT123");

        // Long ES position
        provider.update_position(Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: 2.0,
            avg_price: 5000.0,
            unrealized_pnl: 100.0,
            realized_pnl: 0.0,
            ts_event: 0,
        });

        // Short NQ position
        provider.update_position(Position {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            symbol: "NQZ4".to_string(),
            exchange: "CME".to_string(),
            quantity: -1.0,
            avg_price: 18500.0,
            unrealized_pnl: 200.0,
            realized_pnl: 0.0,
            ts_event: 0,
        });

        assert_eq!(provider.open_positions_count(), 2);
        assert_eq!(provider.long_positions_count(), 1);
        assert_eq!(provider.short_positions_count(), 1);
        assert_eq!(provider.total_unrealized_pnl(), 300.0);
    }
}
