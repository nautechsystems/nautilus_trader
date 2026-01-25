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

//! Data client trait definition.

use async_trait::async_trait;
use nautilus_model::identifiers::{ClientId, Venue};

use super::log_not_implemented;
use crate::messages::data::{
    RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCustomData, RequestInstrument,
    RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
    SubscribeBookDepth10, SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices,
    SubscribeInstrument, SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
    SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas,
    UnsubscribeBookDepth10, UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
    UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
    UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
};
#[cfg(feature = "defi")]
use crate::messages::defi::{
    RequestPoolSnapshot, SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects,
    SubscribePoolFlashEvents, SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
    UnsubscribePool, UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents,
    UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
};

/// Defines the interface for a data client, managing connections, subscriptions, and requests.
///
/// # Thread Safety
///
/// Client instances are not intended to be sent across threads. The `?Send` bound
/// allows implementations to hold non-Send state for any Python interop.
#[async_trait(?Send)]
pub trait DataClient {
    /// Returns the unique identifier for this data client.
    fn client_id(&self) -> ClientId;

    /// Returns the optional venue this client is associated with.
    fn venue(&self) -> Option<Venue>;

    /// Starts the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Resets the data client to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn reset(&mut self) -> anyhow::Result<()>;

    /// Disposes of client resources and cleans up.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    fn dispose(&mut self) -> anyhow::Result<()>;

    /// Returns `true` if the client is currently connected.
    fn is_connected(&self) -> bool;

    /// Returns `true` if the client is currently disconnected.
    fn is_disconnected(&self) -> bool;

    /// Connects the client to the data provider.
    ///
    /// For live clients, this triggers the actual connection to external APIs.
    /// For backtest clients, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    async fn connect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Disconnects the client from the data provider.
    ///
    /// For live clients, this closes connections to external APIs.
    /// For backtest clients, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the disconnection fails.
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Subscribes to custom data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe(&mut self, cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscribe operation fails.
    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Subscribes to instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_instrument_close(&mut self, cmd: &SubscribeInstrumentClose) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to blocks for a specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_blocks(&mut self, cmd: &SubscribeBlocks) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool definition updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool(&mut self, cmd: &SubscribePool) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool swaps for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_swaps(&mut self, cmd: &SubscribePoolSwaps) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool liquidity updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: &SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool fee collects for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_fee_collects(
        &mut self,
        cmd: &SubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Subscribes to pool flash loan events for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn subscribe_pool_flash_events(
        &mut self,
        cmd: &SubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from custom data types according to the command.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from instruments list for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from data for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from order book delta updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from top 10 order book depth updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from bar updates of the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from instrument status updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Unsubscribes from instrument close events for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscribe operation fails.
    fn unsubscribe_instrument_close(
        &mut self,
        cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from blocks for a specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool definition updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from swaps for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool liquidity updates for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool fee collects for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_fee_collects(
        &mut self,
        cmd: &UnsubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Unsubscribes from pool flash loan events for a specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription operation fails.
    fn unsubscribe_pool_flash_events(
        &mut self,
        cmd: &UnsubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        log_not_implemented(&cmd);
        Ok(())
    }

    /// Sends a custom data request to the provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the data request fails.
    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests a list of instruments from the provider for a given venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the instruments request fails.
    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests detailed data for a single instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument request fails.
    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests a snapshot of the order book for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the book snapshot request fails.
    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming quote data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the quotes request fails.
    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming trade data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the trades request fails.
    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical or streaming bar data for a specified instrument and bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bars request fails.
    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    /// Requests historical order book depth data for a specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the order book depths request fails.
    fn request_book_depth(&self, request: RequestBookDepth) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Requests a snapshot of a specific AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool snapshot request fails.
    fn request_pool_snapshot(&self, request: RequestPoolSnapshot) -> anyhow::Result<()> {
        log_not_implemented(&request);
        Ok(())
    }
}
