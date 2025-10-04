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

//! Order book management for Coinbase Advanced Trade API.
//!
//! This module provides an order book manager that maintains a local order book
//! from Level2 WebSocket updates.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::websocket::types::{Level2Event, Level2Update};

/// Price level in the order book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub size: Decimal,
}

impl PriceLevel {
    #[must_use]
    pub fn new(price: Decimal, size: Decimal) -> Self {
        Self { price, size }
    }
}

/// Order book side (bids or asks)
#[derive(Debug, Clone)]
pub struct OrderBookSide {
    /// Price levels stored in a BTreeMap for automatic sorting
    /// For bids: highest price first (reverse order)
    /// For asks: lowest price first (normal order)
    levels: BTreeMap<Decimal, Decimal>,
    is_bid: bool,
}

impl OrderBookSide {
    #[must_use]
    pub fn new(is_bid: bool) -> Self {
        Self {
            levels: BTreeMap::new(),
            is_bid,
        }
    }

    /// Update a price level
    pub fn update(&mut self, price: Decimal, size: Decimal) {
        if size.is_zero() {
            // Remove the level if size is zero
            self.levels.remove(&price);
        } else {
            // Update or insert the level
            self.levels.insert(price, size);
        }
    }

    /// Get the best price (highest for bids, lowest for asks)
    #[must_use]
    pub fn best_price(&self) -> Option<Decimal> {
        if self.is_bid {
            // For bids, get the highest price (last in BTreeMap)
            self.levels.keys().next_back().copied()
        } else {
            // For asks, get the lowest price (first in BTreeMap)
            self.levels.keys().next().copied()
        }
    }

    /// Get the best price level
    #[must_use]
    pub fn best_level(&self) -> Option<PriceLevel> {
        if self.is_bid {
            self.levels
                .iter()
                .next_back()
                .map(|(price, size)| PriceLevel::new(*price, *size))
        } else {
            self.levels
                .iter()
                .next()
                .map(|(price, size)| PriceLevel::new(*price, *size))
        }
    }

    /// Get top N levels
    #[must_use]
    pub fn top_levels(&self, n: usize) -> Vec<PriceLevel> {
        if self.is_bid {
            // For bids, take from the end (highest prices)
            self.levels
                .iter()
                .rev()
                .take(n)
                .map(|(price, size)| PriceLevel::new(*price, *size))
                .collect()
        } else {
            // For asks, take from the start (lowest prices)
            self.levels
                .iter()
                .take(n)
                .map(|(price, size)| PriceLevel::new(*price, *size))
                .collect()
        }
    }

    /// Get all levels
    #[must_use]
    pub fn all_levels(&self) -> Vec<PriceLevel> {
        if self.is_bid {
            self.levels
                .iter()
                .rev()
                .map(|(price, size)| PriceLevel::new(*price, *size))
                .collect()
        } else {
            self.levels
                .iter()
                .map(|(price, size)| PriceLevel::new(*price, *size))
                .collect()
        }
    }

    /// Get the number of levels
    #[must_use]
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// Clear all levels
    pub fn clear(&mut self) {
        self.levels.clear();
    }
}

/// Order book manager
#[derive(Debug, Clone)]
pub struct OrderBook {
    pub product_id: String,
    pub bids: OrderBookSide,
    pub asks: OrderBookSide,
    pub sequence: u64,
}

impl OrderBook {
    /// Create a new order book
    #[must_use]
    pub fn new(product_id: String) -> Self {
        Self {
            product_id,
            bids: OrderBookSide::new(true),
            asks: OrderBookSide::new(false),
            sequence: 0,
        }
    }

    /// Process a Level2 event (snapshot or update)
    ///
    /// # Errors
    ///
    /// Returns an error if the event cannot be processed
    pub fn process_event(&mut self, event: &Level2Event) -> Result<()> {
        debug!(
            "Processing Level2 event for {}: type={:?}, {} updates",
            event.product_id,
            event.event_type,
            event.updates.len()
        );

        match event.event_type.as_str() {
            "snapshot" => {
                self.process_snapshot(event)?;
            }
            "update" => {
                self.process_update(event)?;
            }
            _ => {
                warn!("Unknown Level2 event type: {}", event.event_type);
            }
        }

        Ok(())
    }

    /// Process a snapshot event (full order book)
    fn process_snapshot(&mut self, event: &Level2Event) -> Result<()> {
        info!(
            "Processing snapshot for {}: {} updates",
            event.product_id,
            event.updates.len()
        );

        // Clear existing data
        self.bids.clear();
        self.asks.clear();

        // Apply all updates
        for update in &event.updates {
            self.apply_update(update)?;
        }

        info!(
            "Snapshot processed: {} bids, {} asks",
            self.bids.depth(),
            self.asks.depth()
        );

        Ok(())
    }

    /// Process an update event (incremental changes)
    fn process_update(&mut self, event: &Level2Event) -> Result<()> {
        debug!(
            "Processing update for {}: {} changes",
            event.product_id,
            event.updates.len()
        );

        // Apply all updates
        for update in &event.updates {
            self.apply_update(update)?;
        }

        Ok(())
    }

    /// Apply a single update to the order book
    fn apply_update(&mut self, update: &Level2Update) -> Result<()> {
        let price = update
            .price_level
            .parse::<Decimal>()
            .context("Failed to parse price")?;
        let size = update
            .new_quantity
            .parse::<Decimal>()
            .context("Failed to parse size")?;

        match update.side.as_str() {
            "bid" => {
                self.bids.update(price, size);
            }
            "offer" => {
                self.asks.update(price, size);
            }
            _ => {
                warn!("Unknown order book side: {}", update.side);
            }
        }

        Ok(())
    }

    /// Get the best bid price
    #[must_use]
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.best_price()
    }

    /// Get the best ask price
    #[must_use]
    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.best_price()
    }

    /// Get the mid price (average of best bid and ask)
    #[must_use]
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::from(2)),
            _ => None,
        }
    }

    /// Get the spread (difference between best ask and best bid)
    #[must_use]
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Get the spread as a percentage of mid price
    #[must_use]
    pub fn spread_bps(&self) -> Option<Decimal> {
        match (self.spread(), self.mid_price()) {
            (Some(spread), Some(mid)) if !mid.is_zero() => {
                Some(spread / mid * Decimal::from(10000))
            }
            _ => None,
        }
    }
}

