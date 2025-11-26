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

use std::{ops::Deref, sync::Arc};

use hypersync_client::simple_types::Log;
use nautilus_model::defi::dex::{Dex, SharedDex};

use crate::events::{
    burn::BurnEvent, collect::CollectEvent, flash::FlashEvent, initialize::InitializeEvent,
    mint::MintEvent, pool_created::PoolCreatedEvent, swap::SwapEvent,
};

/// Extended DEX wrapper that adds provider-specific event parsing capabilities to the domain `Dex` model.
#[derive(Debug, Clone)]
pub struct DexExtended {
    /// The core domain Dex object being extended.
    pub dex: SharedDex,
    /// Function to parse pool creation events.
    pub parse_pool_created_event_fn: Option<fn(Log) -> anyhow::Result<PoolCreatedEvent>>,
    /// Function to parse initialize events.
    pub parse_initialize_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<InitializeEvent>>,
    /// Function to parse swap events.
    pub parse_swap_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<SwapEvent>>,
    /// Function to parse mint events.
    pub parse_mint_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<MintEvent>>,
    /// Function to parse burn events.
    pub parse_burn_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<BurnEvent>>,
    /// Function to parse collect events.
    pub parse_collect_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<CollectEvent>>,
    /// Function to parse flash events.
    pub parse_flash_event_fn: Option<fn(SharedDex, Log) -> anyhow::Result<FlashEvent>>,
}

impl DexExtended {
    /// Creates a new [`DexExtended`] wrapper around a domain `Dex` object.
    #[must_use]
    pub fn new(dex: Dex) -> Self {
        Self {
            dex: Arc::new(dex),
            parse_pool_created_event_fn: None,
            parse_initialize_event_fn: None,
            parse_swap_event_fn: None,
            parse_mint_event_fn: None,
            parse_burn_event_fn: None,
            parse_collect_event_fn: None,
            parse_flash_event_fn: None,
        }
    }

    /// Sets the function used to parse pool creation events for this Dex.
    pub fn set_pool_created_event_parsing(
        &mut self,
        parse_pool_created_event: fn(Log) -> anyhow::Result<PoolCreatedEvent>,
    ) {
        self.parse_pool_created_event_fn = Some(parse_pool_created_event);
    }

    /// Sets the function used to parse initialize events for this Dex.
    pub fn set_initialize_event_parsing(
        &mut self,
        parse_initialize_event: fn(SharedDex, Log) -> anyhow::Result<InitializeEvent>,
    ) {
        self.parse_initialize_event_fn = Some(parse_initialize_event);
    }

    /// Sets the function used to parse swap events for this Dex.
    pub fn set_swap_event_parsing(
        &mut self,
        parse_swap_event: fn(SharedDex, Log) -> anyhow::Result<SwapEvent>,
    ) {
        self.parse_swap_event_fn = Some(parse_swap_event);
    }

    /// Sets the function used to parse mint events for this Dex.
    pub fn set_mint_event_parsing(
        &mut self,
        parse_mint_event: fn(SharedDex, Log) -> anyhow::Result<MintEvent>,
    ) {
        self.parse_mint_event_fn = Some(parse_mint_event);
    }

    /// Sets the function used to parse burn events for this Dex.
    pub fn set_burn_event_parsing(
        &mut self,
        parse_burn_event: fn(SharedDex, Log) -> anyhow::Result<BurnEvent>,
    ) {
        self.parse_burn_event_fn = Some(parse_burn_event);
    }

    /// Sets the function used to parse collect events for this Dex.
    pub fn set_collect_event_parsing(
        &mut self,
        parse_collect_event: fn(SharedDex, Log) -> anyhow::Result<CollectEvent>,
    ) {
        self.parse_collect_event_fn = Some(parse_collect_event);
    }

    /// Sets the function used to parse flash events for this Dex.
    pub fn set_flash_event_parsing(
        &mut self,
        parse_flash_event: fn(SharedDex, Log) -> anyhow::Result<FlashEvent>,
    ) {
        self.parse_flash_event_fn = Some(parse_flash_event);
    }

    /// Parses a pool creation event log using this DEX's specific parsing function.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a pool creation event parser defined or if parsing fails.
    pub fn parse_pool_created_event(&self, log: Log) -> anyhow::Result<PoolCreatedEvent> {
        if let Some(parse_pool_created_event_fn) = &self.parse_pool_created_event_fn {
            parse_pool_created_event_fn(log)
        } else {
            anyhow::bail!(
                "Parsing of pool created event in not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name,
            )
        }
    }

    /// Parses a swap event log using this DEX's specific parsing function.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a swap event parser defined or if parsing fails.
    pub fn parse_swap_event(&self, log: Log) -> anyhow::Result<SwapEvent> {
        if let Some(parse_swap_event_fn) = &self.parse_swap_event_fn {
            parse_swap_event_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "Parsing of swap event in not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a mint event log using this DEX's specific parsing function.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a mint event parser defined or if parsing fails.
    pub fn parse_mint_event(&self, log: Log) -> anyhow::Result<MintEvent> {
        if let Some(parse_mint_event_fn) = &self.parse_mint_event_fn {
            parse_mint_event_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "Parsing of mint event in not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a burn event log using this DEX's specific parsing function.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a burn event parser defined or if parsing fails.
    pub fn parse_burn_event(&self, log: Log) -> anyhow::Result<BurnEvent> {
        if let Some(parse_burn_event_fn) = &self.parse_burn_event_fn {
            parse_burn_event_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "Parsing of burn event in not defined in this dex: {}",
                self.dex.name
            )
        }
    }

    /// Checks if this DEX requires pool initialization events.
    pub fn needs_initialization(&self) -> bool {
        self.dex.initialize_event.is_some()
    }

    /// Parses an event log into an `InitializeEvent` struct.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not support initialize events or parsing fails.
    pub fn parse_initialize_event(&self, log: Log) -> anyhow::Result<InitializeEvent> {
        if let Some(parse_initialize_event_fn) = &self.parse_initialize_event_fn {
            parse_initialize_event_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "Parsing of initialize event in not defined in this dex: {}",
                self.dex.name
            )
        }
    }

    /// Parses a collect event log using this DEX's specific parsing function.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a collect event parser defined or if parsing fails.
    pub fn parse_collect_event(&self, log: Log) -> anyhow::Result<CollectEvent> {
        if let Some(parse_collect_event_fn) = &self.parse_collect_event_fn {
            parse_collect_event_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "Parsing of collect event in not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }
}

impl Deref for DexExtended {
    type Target = Dex;

    fn deref(&self) -> &Self::Target {
        &self.dex
    }
}
