// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Portfolio tracking and analytics for Coinbase Advanced Trade API.

use std::collections::HashMap;

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{http::CoinbaseHttpClient, types::Account};

/// Portfolio holding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holding {
    pub currency: String,
    pub balance: Decimal,
    pub available: Decimal,
    pub hold: Decimal,
    pub usd_value: Option<Decimal>,
}

/// Portfolio snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub holdings: Vec<Holding>,
    pub total_usd_value: Decimal,
}

/// Portfolio tracker
#[derive(Debug, Clone)]
pub struct PortfolioTracker {
    client: CoinbaseHttpClient,
    snapshots: Vec<PortfolioSnapshot>,
}

impl PortfolioTracker {
    /// Create a new portfolio tracker
    #[must_use]
    pub fn new(client: CoinbaseHttpClient) -> Self {
        Self {
            client,
            snapshots: Vec::new(),
        }
    }

    /// Take a snapshot of the current portfolio
    ///
    /// # Errors
    ///
    /// Returns an error if fetching accounts or prices fails
    pub async fn take_snapshot(&mut self) -> Result<PortfolioSnapshot> {
        info!("Taking portfolio snapshot...");

        // Get all accounts
        let accounts_response = self.client.list_accounts().await?;
        let accounts = accounts_response.accounts;

        debug!("Found {} accounts", accounts.len());

        // Build holdings
        let mut holdings = Vec::new();
        let mut total_usd_value = Decimal::ZERO;

        for account in accounts {
            let balance = account
                .available_balance
                .value
                .parse::<Decimal>()
                .context("Failed to parse balance")?;

            if balance.is_zero() {
                continue;
            }

            let available = account
                .available_balance
                .value
                .parse::<Decimal>()
                .context("Failed to parse available balance")?;

            let hold = balance - available;

            // Get USD value
            let usd_value = if account.currency == "USD" || account.currency == "USDC" {
                Some(balance)
            } else {
                // Try to get price from product
                let product_id = format!("{}-USD", account.currency);
                match self.client.get_product(&product_id).await {
                    Ok(product) => {
                        if let Some(price_str) = product.price {
                            if let Ok(price) = price_str.parse::<Decimal>() {
                                Some(balance * price)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            };

            if let Some(value) = usd_value {
                total_usd_value += value;
            }

            holdings.push(Holding {
                currency: account.currency.clone(),
                balance,
                available,
                hold,
                usd_value,
            });
        }

        let snapshot = PortfolioSnapshot {
            timestamp: chrono::Utc::now(),
            holdings,
            total_usd_value,
        };

        self.snapshots.push(snapshot.clone());
        info!("Snapshot taken: ${} total value", total_usd_value);

        Ok(snapshot)
    }

    /// Get all snapshots
    #[must_use]
    pub fn snapshots(&self) -> &[PortfolioSnapshot] {
        &self.snapshots
    }

    /// Get the latest snapshot
    #[must_use]
    pub fn latest_snapshot(&self) -> Option<&PortfolioSnapshot> {
        self.snapshots.last()
    }

    /// Calculate portfolio performance metrics
    #[must_use]
    pub fn performance_metrics(&self) -> Option<PerformanceMetrics> {
        if self.snapshots.len() < 2 {
            return None;
        }

        let first = &self.snapshots[0];
        let latest = self.snapshots.last()?;

        let initial_value = first.total_usd_value;
        let current_value = latest.total_usd_value;

        let absolute_return = current_value - initial_value;
        let percentage_return = if !initial_value.is_zero() {
            (absolute_return / initial_value) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        let duration = latest.timestamp - first.timestamp;
        let days = duration.num_days() as f64;

        let annualized_return = if days > 0.0 {
            let daily_return = percentage_return / Decimal::try_from(days).unwrap_or(Decimal::ONE);
            daily_return * Decimal::from(365)
        } else {
            Decimal::ZERO
        };

        Some(PerformanceMetrics {
            initial_value,
            current_value,
            absolute_return,
            percentage_return,
            annualized_return,
            duration_days: days,
            num_snapshots: self.snapshots.len(),
        })
    }

    /// Get holdings breakdown by currency
    #[must_use]
    pub fn holdings_breakdown(&self) -> Option<HashMap<String, HoldingStats>> {
        let latest = self.latest_snapshot()?;

        let mut breakdown = HashMap::new();

        for holding in &latest.holdings {
            let percentage = if !latest.total_usd_value.is_zero() {
                holding.usd_value.unwrap_or(Decimal::ZERO) / latest.total_usd_value
                    * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            breakdown.insert(
                holding.currency.clone(),
                HoldingStats {
                    balance: holding.balance,
                    usd_value: holding.usd_value.unwrap_or(Decimal::ZERO),
                    percentage,
                },
            );
        }

        Some(breakdown)
    }
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub initial_value: Decimal,
    pub current_value: Decimal,
    pub absolute_return: Decimal,
    pub percentage_return: Decimal,
    pub annualized_return: Decimal,
    pub duration_days: f64,
    pub num_snapshots: usize,
}

/// Holding statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldingStats {
    pub balance: Decimal,
    pub usd_value: Decimal,
    pub percentage: Decimal,
}

