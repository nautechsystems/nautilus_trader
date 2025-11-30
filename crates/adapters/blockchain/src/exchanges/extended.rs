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

use nautilus_model::defi::{
    dex::{Dex, SharedDex},
    rpc::RpcLog,
};

use crate::{
    events::{
        burn::BurnEvent, collect::CollectEvent, flash::FlashEvent, initialize::InitializeEvent,
        mint::MintEvent, pool_created::PoolCreatedEvent, swap::SwapEvent,
    },
    hypersync::HypersyncLog,
};

/// Extended DEX wrapper that adds provider-specific event parsing capabilities to the domain `Dex` model.
#[derive(Debug, Clone)]
pub struct DexExtended {
    /// The core domain Dex object being extended.
    pub dex: SharedDex,
    // === HyperSync parsers ===
    /// Function to parse pool creation events from HyperSync logs.
    pub parse_pool_created_event_hypersync_fn:
        Option<fn(HypersyncLog) -> anyhow::Result<PoolCreatedEvent>>,
    /// Function to parse initialize events from HyperSync logs.
    pub parse_initialize_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<InitializeEvent>>,
    /// Function to parse swap events from HyperSync logs.
    pub parse_swap_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<SwapEvent>>,
    /// Function to parse mint events from HyperSync logs.
    pub parse_mint_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<MintEvent>>,
    /// Function to parse burn events from HyperSync logs.
    pub parse_burn_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<BurnEvent>>,
    /// Function to parse collect events from HyperSync logs.
    pub parse_collect_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<CollectEvent>>,
    /// Function to parse flash events from HyperSync logs.
    pub parse_flash_event_hypersync_fn:
        Option<fn(SharedDex, HypersyncLog) -> anyhow::Result<FlashEvent>>,
    // === RPC parsers (hex-decode, standard Ethereum format) ===
    /// Function to parse pool creation events from RPC logs.
    pub parse_pool_created_event_rpc_fn: Option<fn(&RpcLog) -> anyhow::Result<PoolCreatedEvent>>,
    /// Function to parse initialize events from RPC logs.
    pub parse_initialize_event_rpc_fn:
        Option<fn(SharedDex, &RpcLog) -> anyhow::Result<InitializeEvent>>,
    /// Function to parse swap events from RPC logs.
    pub parse_swap_event_rpc_fn: Option<fn(SharedDex, &RpcLog) -> anyhow::Result<SwapEvent>>,
    /// Function to parse mint events from RPC logs.
    pub parse_mint_event_rpc_fn: Option<fn(SharedDex, &RpcLog) -> anyhow::Result<MintEvent>>,
    /// Function to parse burn events from RPC logs.
    pub parse_burn_event_rpc_fn: Option<fn(SharedDex, &RpcLog) -> anyhow::Result<BurnEvent>>,
    /// Function to parse collect events from RPC logs.
    pub parse_collect_event_rpc_fn: Option<fn(SharedDex, &RpcLog) -> anyhow::Result<CollectEvent>>,
    /// Function to parse flash events from RPC logs.
    pub parse_flash_event_rpc_fn: Option<fn(SharedDex, &RpcLog) -> anyhow::Result<FlashEvent>>,
}

impl DexExtended {
    /// Creates a new [`DexExtended`] wrapper around a domain `Dex` object.
    #[must_use]
    pub fn new(dex: Dex) -> Self {
        Self {
            dex: Arc::new(dex),
            // HyperSync parsers
            parse_pool_created_event_hypersync_fn: None,
            parse_initialize_event_hypersync_fn: None,
            parse_swap_event_hypersync_fn: None,
            parse_mint_event_hypersync_fn: None,
            parse_burn_event_hypersync_fn: None,
            parse_collect_event_hypersync_fn: None,
            parse_flash_event_hypersync_fn: None,
            // RPC parsers
            parse_pool_created_event_rpc_fn: None,
            parse_initialize_event_rpc_fn: None,
            parse_swap_event_rpc_fn: None,
            parse_mint_event_rpc_fn: None,
            parse_burn_event_rpc_fn: None,
            parse_collect_event_rpc_fn: None,
            parse_flash_event_rpc_fn: None,
        }
    }

    // ==================== HyperSync Parser Setters ====================

    /// Sets the function used to parse pool creation events from HyperSync logs.
    pub fn set_pool_created_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(HypersyncLog) -> anyhow::Result<PoolCreatedEvent>,
    ) {
        self.parse_pool_created_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse initialize events from HyperSync logs.
    pub fn set_initialize_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<InitializeEvent>,
    ) {
        self.parse_initialize_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse swap events from HyperSync logs.
    pub fn set_swap_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<SwapEvent>,
    ) {
        self.parse_swap_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse mint events from HyperSync logs.
    pub fn set_mint_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<MintEvent>,
    ) {
        self.parse_mint_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse burn events from HyperSync logs.
    pub fn set_burn_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<BurnEvent>,
    ) {
        self.parse_burn_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse collect events from HyperSync logs.
    pub fn set_collect_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<CollectEvent>,
    ) {
        self.parse_collect_event_hypersync_fn = Some(parse_fn);
    }

    /// Sets the function used to parse flash events from HyperSync logs.
    pub fn set_flash_event_hypersync_parsing(
        &mut self,
        parse_fn: fn(SharedDex, HypersyncLog) -> anyhow::Result<FlashEvent>,
    ) {
        self.parse_flash_event_hypersync_fn = Some(parse_fn);
    }

    // ==================== RPC Parser Setters ====================

    /// Sets the function used to parse pool creation events from RPC logs.
    pub fn set_pool_created_event_rpc_parsing(
        &mut self,
        parse_fn: fn(&RpcLog) -> anyhow::Result<PoolCreatedEvent>,
    ) {
        self.parse_pool_created_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse initialize events from RPC logs.
    pub fn set_initialize_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<InitializeEvent>,
    ) {
        self.parse_initialize_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse swap events from RPC logs.
    pub fn set_swap_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<SwapEvent>,
    ) {
        self.parse_swap_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse mint events from RPC logs.
    pub fn set_mint_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<MintEvent>,
    ) {
        self.parse_mint_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse burn events from RPC logs.
    pub fn set_burn_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<BurnEvent>,
    ) {
        self.parse_burn_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse collect events from RPC logs.
    pub fn set_collect_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<CollectEvent>,
    ) {
        self.parse_collect_event_rpc_fn = Some(parse_fn);
    }

    /// Sets the function used to parse flash events from RPC logs.
    pub fn set_flash_event_rpc_parsing(
        &mut self,
        parse_fn: fn(SharedDex, &RpcLog) -> anyhow::Result<FlashEvent>,
    ) {
        self.parse_flash_event_rpc_fn = Some(parse_fn);
    }

    // ==================== HyperSync Parser Dispatch Methods ====================

    /// Parses a pool creation event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync pool creation event parser defined or if parsing fails.
    pub fn parse_pool_created_event_hypersync(
        &self,
        log: HypersyncLog,
    ) -> anyhow::Result<PoolCreatedEvent> {
        if let Some(parse_fn) = &self.parse_pool_created_event_hypersync_fn {
            parse_fn(log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of pool created event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name,
            )
        }
    }

    /// Parses a swap event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync swap event parser defined or if parsing fails.
    pub fn parse_swap_event_hypersync(&self, log: HypersyncLog) -> anyhow::Result<SwapEvent> {
        if let Some(parse_fn) = &self.parse_swap_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of swap event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a mint event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync mint event parser defined or if parsing fails.
    pub fn parse_mint_event_hypersync(&self, log: HypersyncLog) -> anyhow::Result<MintEvent> {
        if let Some(parse_fn) = &self.parse_mint_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of mint event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a burn event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync burn event parser defined or if parsing fails.
    pub fn parse_burn_event_hypersync(&self, log: HypersyncLog) -> anyhow::Result<BurnEvent> {
        if let Some(parse_fn) = &self.parse_burn_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of burn event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses an initialize event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync initialize event parser defined or if parsing fails.
    pub fn parse_initialize_event_hypersync(
        &self,
        log: HypersyncLog,
    ) -> anyhow::Result<InitializeEvent> {
        if let Some(parse_fn) = &self.parse_initialize_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of initialize event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a collect event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync collect event parser defined or if parsing fails.
    pub fn parse_collect_event_hypersync(&self, log: HypersyncLog) -> anyhow::Result<CollectEvent> {
        if let Some(parse_fn) = &self.parse_collect_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of collect event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a flash event from a HyperSync log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have a HyperSync flash event parser defined or if parsing fails.
    pub fn parse_flash_event_hypersync(&self, log: HypersyncLog) -> anyhow::Result<FlashEvent> {
        if let Some(parse_fn) = &self.parse_flash_event_hypersync_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "HyperSync parsing of flash event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    // ==================== RPC Parser Dispatch Methods ====================

    /// Parses a pool creation event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC pool creation event parser defined or if parsing fails.
    pub fn parse_pool_created_event_rpc(&self, log: &RpcLog) -> anyhow::Result<PoolCreatedEvent> {
        if let Some(parse_fn) = &self.parse_pool_created_event_rpc_fn {
            parse_fn(log)
        } else {
            anyhow::bail!(
                "RPC parsing of pool created event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name,
            )
        }
    }

    /// Parses a swap event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC swap event parser defined or if parsing fails.
    pub fn parse_swap_event_rpc(&self, log: &RpcLog) -> anyhow::Result<SwapEvent> {
        if let Some(parse_fn) = &self.parse_swap_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of swap event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a mint event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC mint event parser defined or if parsing fails.
    pub fn parse_mint_event_rpc(&self, log: &RpcLog) -> anyhow::Result<MintEvent> {
        if let Some(parse_fn) = &self.parse_mint_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of mint event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a burn event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC burn event parser defined or if parsing fails.
    pub fn parse_burn_event_rpc(&self, log: &RpcLog) -> anyhow::Result<BurnEvent> {
        if let Some(parse_fn) = &self.parse_burn_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of burn event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses an initialize event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC initialize event parser defined or if parsing fails.
    pub fn parse_initialize_event_rpc(&self, log: &RpcLog) -> anyhow::Result<InitializeEvent> {
        if let Some(parse_fn) = &self.parse_initialize_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of initialize event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a collect event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC collect event parser defined or if parsing fails.
    pub fn parse_collect_event_rpc(&self, log: &RpcLog) -> anyhow::Result<CollectEvent> {
        if let Some(parse_fn) = &self.parse_collect_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of collect event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    /// Parses a flash event from an RPC log.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX does not have an RPC flash event parser defined or if parsing fails.
    pub fn parse_flash_event_rpc(&self, log: &RpcLog) -> anyhow::Result<FlashEvent> {
        if let Some(parse_fn) = &self.parse_flash_event_rpc_fn {
            parse_fn(self.dex.clone(), log)
        } else {
            anyhow::bail!(
                "RPC parsing of flash event is not defined in this dex: {}:{}",
                self.dex.chain,
                self.dex.name
            )
        }
    }

    // ==================== Utility Methods ====================

    /// Checks if this DEX requires pool initialization events.
    #[must_use]
    pub fn needs_initialization(&self) -> bool {
        self.dex.initialize_event.is_some()
    }
}

impl Deref for DexExtended {
    type Target = Dex;

    fn deref(&self) -> &Self::Target {
        &self.dex
    }
}
