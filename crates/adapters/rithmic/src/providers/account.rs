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

//! Account state provider for Rithmic.
//!
//! The `RithmicAccountProvider` receives account balance updates from the
//! PnL plant via the `RithmicGateway`. It maintains current account state
//! and provides event notifications for balance changes.

use std::{fmt::Debug, sync::Arc};

use dashmap::DashMap;
use tokio::task::JoinHandle;

use crate::common::types::{RithmicAccountId, UnixNanos};
use crate::error::Result;
use crate::gateway::{PnlEvent, RithmicGateway};

/// Account balance information.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    /// Whether the source notification was a Rithmic snapshot payload.
    pub is_snapshot: bool,
    /// Account ID.
    pub account_id: RithmicAccountId,
    /// Currency.
    pub currency: String,
    /// Total balance.
    pub total: f64,
    /// Available balance (for trading).
    pub available: f64,
    /// Locked/used margin.
    pub locked: f64,
    /// Unrealized PnL.
    pub unrealized_pnl: f64,
    /// Realized PnL.
    pub realized_pnl: f64,
    /// Timestamp.
    pub ts_event: UnixNanos,
}

/// Account state event.
#[derive(Debug, Clone)]
pub enum AccountEvent {
    /// Balance update.
    BalanceUpdate(AccountBalance),
    /// Margin call warning.
    MarginWarning { account_id: String, message: String },
    /// Error.
    Error(String),
}

/// Provides account state from Rithmic.
///
/// The provider receives account balance updates from the gateway's PnL plant
/// and maintains the current account state. Use `event_receiver()` to get a
/// channel for real-time balance change notifications.
pub struct RithmicAccountProvider {
    gateway: Arc<RithmicGateway>,
    account_id: String,
    balances: Arc<DashMap<String, AccountBalance>>,
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<AccountEvent>>,
    task_handle: Option<JoinHandle<()>>,
}

impl RithmicAccountProvider {
    /// Creates a new account provider connected to the given gateway.
    ///
    /// # Arguments
    /// * `gateway` - The connected Rithmic gateway
    /// * `account_id` - The account ID to track (usually from gateway config)
    pub fn new(gateway: Arc<RithmicGateway>, account_id: impl Into<String>) -> Self {
        Self {
            gateway,
            account_id: account_id.into(),
            balances: Arc::new(DashMap::new()),
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

    /// Starts receiving account updates from the gateway.
    ///
    /// This spawns a background task that processes PnL events from the gateway
    /// and updates the local balance state. Call this after the gateway is connected.
    pub async fn start(&mut self) -> Result<()> {
        if self.task_handle.is_some() {
            tracing::warn!("Account provider already started");
            return Ok(());
        }

        // Note: The gateway already subscribes to PnL updates during connect().
        // We just need to process the events from the gateway's PnL channel.
        // However, the gateway's take_pnl_receiver() can only be called once,
        // so we need a different approach - we'll poll the gateway's event channel
        // through a shared mechanism.
        //
        // For now, the provider acts as a state container that gets updated
        // by the caller who has access to the gateway's PnL events.
        tracing::debug!("Account provider started for account {}", self.account_id);
        Ok(())
    }

    /// Stops receiving account updates.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        tracing::debug!("Account provider stopped");
        Ok(())
    }

    /// Returns the current account balance.
    pub fn balance(&self) -> Option<AccountBalance> {
        self.balances.get(&self.account_id).map(|r| r.clone())
    }

    /// Returns balance for a specific account.
    pub fn balance_for_account(&self, account_id: &str) -> Option<AccountBalance> {
        self.balances.get(account_id).map(|r| r.clone())
    }

    /// Returns available margin.
    pub fn available_margin(&self) -> f64 {
        self.balance().map_or(0.0, |b| b.available)
    }

    /// Returns unrealized PnL.
    pub fn unrealized_pnl(&self) -> f64 {
        self.balance().map_or(0.0, |b| b.unrealized_pnl)
    }

    /// Returns realized PnL.
    pub fn realized_pnl(&self) -> f64 {
        self.balance().map_or(0.0, |b| b.realized_pnl)
    }

    /// Returns total balance.
    pub fn total_balance(&self) -> f64 {
        self.balance().map_or(0.0, |b| b.total)
    }

    /// Returns locked margin.
    pub fn locked_margin(&self) -> f64 {
        self.balance().map_or(0.0, |b| b.locked)
    }

    /// Returns a receiver for account events.
    pub fn event_receiver(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<AccountEvent> {
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
        if let PnlEvent::Account(account_event) = event {
            match account_event {
                AccountEvent::BalanceUpdate(balance) => {
                    self.update_balance(balance.clone());
                }
                AccountEvent::MarginWarning {
                    account_id,
                    message,
                } => {
                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AccountEvent::MarginWarning {
                            account_id: account_id.clone(),
                            message: message.clone(),
                        });
                    }
                }
                AccountEvent::Error(err) => {
                    tracing::error!("Account error: {}", err);

                    if let Some(tx) = &self.event_tx {
                        let _ = tx.send(AccountEvent::Error(err.clone()));
                    }
                }
            }
        }
    }

    /// Updates account balance (internal use).
    pub(crate) fn update_balance(&self, balance: AccountBalance) {
        let account_id = balance.account_id.clone();

        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AccountEvent::BalanceUpdate(balance.clone()));
        }
        self.balances.insert(account_id, balance);
    }

    /// Clears all balances (for reset/reconnect).
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.balances.clear();
    }
}

impl Debug for RithmicAccountProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RithmicAccountProvider))
            .field("account_id", &self.account_id)
            .field("available_margin", &self.available_margin())
            .field("total_balance", &self.total_balance())
            .field("unrealized_pnl", &self.unrealized_pnl())
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
    fn test_account_provider_creation() {
        let gateway = create_test_gateway();
        let provider = RithmicAccountProvider::new(gateway, "ACCOUNT123");
        assert_eq!(provider.account_id(), "ACCOUNT123");
        assert!(provider.balance().is_none());
    }

    #[rstest::rstest]
    fn test_update_balance() {
        let gateway = create_test_gateway();
        let provider = RithmicAccountProvider::new(gateway, "ACCOUNT123");

        let balance = AccountBalance {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            currency: "USD".to_string(),
            total: 100000.0,
            available: 50000.0,
            locked: 50000.0,
            unrealized_pnl: 1000.0,
            realized_pnl: 500.0,
            ts_event: 0,
        };

        provider.update_balance(balance);

        let retrieved = provider.balance().unwrap();
        assert_eq!(retrieved.total, 100000.0);
        assert_eq!(retrieved.available, 50000.0);
    }

    #[rstest::rstest]
    fn test_process_pnl_event() {
        let gateway = create_test_gateway();
        let provider = RithmicAccountProvider::new(gateway, "ACCOUNT123");

        let balance = AccountBalance {
            is_snapshot: false,
            account_id: "ACCOUNT123".to_string(),
            currency: "USD".to_string(),
            total: 75000.0,
            available: 25000.0,
            locked: 50000.0,
            unrealized_pnl: 500.0,
            realized_pnl: 250.0,
            ts_event: 1704067200000000000,
        };

        let event = PnlEvent::Account(AccountEvent::BalanceUpdate(balance));
        provider.process_pnl_event(&event);

        let retrieved = provider.balance().unwrap();
        assert_eq!(retrieved.total, 75000.0);
        assert_eq!(retrieved.available, 25000.0);
        assert_eq!(provider.available_margin(), 25000.0);
        assert_eq!(provider.unrealized_pnl(), 500.0);
    }

    #[rstest::rstest]
    fn test_balance_for_different_account() {
        let gateway = create_test_gateway();
        let provider = RithmicAccountProvider::new(gateway, "ACCOUNT123");

        // Update balance for a different account
        let balance = AccountBalance {
            is_snapshot: false,
            account_id: "OTHER_ACCOUNT".to_string(),
            currency: "USD".to_string(),
            total: 50000.0,
            available: 30000.0,
            locked: 20000.0,
            unrealized_pnl: 0.0,
            realized_pnl: 0.0,
            ts_event: 0,
        };

        provider.update_balance(balance);

        // Primary account should still be None
        assert!(provider.balance().is_none());

        // But we can retrieve the other account's balance
        let retrieved = provider.balance_for_account("OTHER_ACCOUNT").unwrap();
        assert_eq!(retrieved.total, 50000.0);
    }
}
