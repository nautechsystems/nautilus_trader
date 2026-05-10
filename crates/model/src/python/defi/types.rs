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

//! Python bindings for DeFi types.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
};

use nautilus_core::python::to_pyvalue_err;
use pyo3::{basic::CompareOp, prelude::*};

use crate::{
    defi::{AmmType, Blockchain, Chain, Dex, DexType, Pool, Token, chain::chains},
    identifiers::InstrumentId,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Chain {
    /// Defines a blockchain with its unique identifiers and connection details for network interaction.
    #[new]
    fn py_new(name: Blockchain, chain_id: u32) -> Self {
        Self::new(name, chain_id)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.chain_id.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for Chain"),
        }
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> Blockchain {
        self.name
    }

    #[getter]
    #[pyo3(name = "chain_id")]
    fn py_chain_id(&self) -> u32 {
        self.chain_id
    }

    #[getter]
    #[pyo3(name = "hypersync_url")]
    fn py_hypersync_url(&self) -> &str {
        &self.hypersync_url
    }

    #[getter]
    #[pyo3(name = "rpc_url")]
    fn py_rpc_url(&self) -> Option<&str> {
        self.rpc_url.as_deref()
    }

    #[getter]
    #[pyo3(name = "native_currency_decimals")]
    fn py_native_currency_decimals(&self) -> u8 {
        self.native_currency_decimals
    }

    /// Sets the RPC URL endpoint.
    #[pyo3(name = "set_rpc_url")]
    fn py_set_rpc_url(&mut self, rpc_url: String) {
        self.set_rpc_url(rpc_url);
    }

    /// Returns a reference to the `Chain` corresponding to the given chain name, or `None` if it is not found.
    ///
    /// String matching is case-insensitive.
    #[staticmethod]
    #[pyo3(name = "from_chain_name")]
    fn py_from_chain_name(chain_name: &str) -> PyResult<Self> {
        Self::from_chain_name(chain_name)
            .cloned()
            .ok_or_else(|| to_pyvalue_err(format!("`chain_name` '{chain_name}' is not recognized")))
    }

    /// Returns a reference to the `Chain` corresponding to the given `chain_id`, or `None` if it is not found.
    #[staticmethod]
    #[pyo3(name = "from_chain_id")]
    fn py_from_chain_id(chain_id: u32) -> Option<Self> {
        Self::from_chain_id(chain_id).cloned()
    }

    #[staticmethod]
    #[pyo3(name = "ARBITRUM")]
    fn py_arbitrum_chain() -> Self {
        chains::ARBITRUM.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Token {
    /// Represents a cryptocurrency token on a blockchain network.
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(
        chain: Chain,
        address: String,
        name: String,
        symbol: String,
        decimals: u8,
    ) -> PyResult<Self> {
        let address = address.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(Arc::new(chain), address, name, symbol, decimals))
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.chain.chain_id.hash(&mut hasher);
        self.address.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for Token"),
        }
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> Chain {
        self.chain.as_ref().clone()
    }

    #[getter]
    #[pyo3(name = "address")]
    fn py_address(&self) -> String {
        self.address.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        &self.name
    }

    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> &str {
        &self.symbol
    }

    #[getter]
    #[pyo3(name = "decimals")]
    fn py_decimals(&self) -> u8 {
        self.decimals
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Dex {
    /// Represents a decentralized exchange (DEX) in a blockchain ecosystem.
    #[new]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn py_new(
        chain: Chain,
        name: String,
        factory: String,
        factory_creation_block: u64,
        amm_type: String,
        pool_created_event: &str,
        swap_event: &str,
        mint_event: &str,
        burn_event: &str,
        collect_event: &str,
    ) -> PyResult<Self> {
        let amm_type = AmmType::from_str(&amm_type).map_err(to_pyvalue_err)?;
        let dex_type = DexType::from_dex_name(&name)
            .ok_or_else(|| to_pyvalue_err(format!("Invalid DEX name: {name}")))?;
        Ok(Self::new(
            chain,
            dex_type,
            &factory,
            factory_creation_block,
            amm_type,
            pool_created_event,
            swap_event,
            mint_event,
            burn_event,
            collect_event,
        ))
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.chain.chain_id.hash(&mut hasher);
        self.name.hash(&mut hasher);
        self.factory.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for Dex"),
        }
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> Chain {
        self.chain.clone()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> DexType {
        self.name
    }

    #[getter]
    #[pyo3(name = "factory")]
    fn py_factory(&self) -> String {
        self.factory.to_string()
    }

    #[getter]
    #[pyo3(name = "factory_creation_block")]
    fn py_factory_creation_block(&self) -> u64 {
        self.factory_creation_block
    }

    #[getter]
    #[pyo3(name = "pool_created_event")]
    fn py_pool_created_event(&self) -> &str {
        &self.pool_created_event
    }

    #[getter]
    #[pyo3(name = "swap_created_event")]
    fn py_swap_created_event(&self) -> &str {
        &self.swap_created_event
    }

    #[getter]
    #[pyo3(name = "mint_created_event")]
    fn py_mint_created_event(&self) -> &str {
        &self.mint_created_event
    }

    #[getter]
    #[pyo3(name = "burn_created_event")]
    fn py_burn_created_event(&self) -> &str {
        &self.burn_created_event
    }

    #[getter]
    #[pyo3(name = "amm_type")]
    fn py_amm_type(&self) -> AmmType {
        self.amm_type
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Pool {
    /// Represents a liquidity pool in a decentralized exchange.
    ///
    /// ## Pool Identification Architecture
    ///
    /// Pools are identified differently depending on the DEX protocol version:
    ///
    /// **UniswapV2/V3**: Each pool has its own smart contract deployed at a unique address.
    /// - `address` = pool contract address
    /// - `pool_identifier` = same as address (hex string)
    ///
    /// **`UniswapV4`**: All pools share a singleton `PoolManager` contract. Pools are distinguished
    /// by a unique Pool ID (keccak256 hash of currencies, fee, tick spacing, and hooks).
    /// - `address` = `PoolManager` contract address (shared by all pools)
    /// - `pool_identifier` = Pool ID (bytes32 as hex string)
    ///
    /// ## Instrument ID Format
    ///
    /// The instrument ID encodes with the following components:
    /// - `symbol` – The pool identifier (address for V2/V3, Pool ID for V4)
    /// - `venue`  – The chain name plus DEX ID
    ///
    /// String representation: `<POOL_IDENTIFIER>.<CHAIN_NAME>:<DEX_ID>`
    ///
    /// Example: `0x11b815efB8f581194ae79006d24E0d814B7697F6.Ethereum:UniswapV3`
    #[new]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        address: String,
        pool_identifier: String,
        creation_block: u64,
        token0: Token,
        token1: Token,
        fee: Option<u32>,
        tick_spacing: Option<u32>,
        ts_init: u64,
    ) -> PyResult<Self> {
        let address = address.parse().map_err(to_pyvalue_err)?;
        let pool_identifier = pool_identifier.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            address,
            pool_identifier,
            creation_block,
            token0,
            token1,
            fee,
            tick_spacing,
            ts_init.into(),
        ))
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.chain.chain_id.hash(&mut hasher);
        self.address.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for Pool"),
        }
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> Chain {
        self.chain.as_ref().clone()
    }

    #[getter]
    #[pyo3(name = "dex")]
    fn py_dex(&self) -> Dex {
        self.dex.as_ref().clone()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "address")]
    fn py_address(&self) -> String {
        self.address.to_string()
    }

    #[getter]
    #[pyo3(name = "creation_block")]
    fn py_creation_block(&self) -> u64 {
        self.creation_block
    }

    #[getter]
    #[pyo3(name = "token0")]
    fn py_token0(&self) -> Token {
        self.token0.clone()
    }

    #[getter]
    #[pyo3(name = "token1")]
    fn py_token1(&self) -> Token {
        self.token1.clone()
    }

    #[getter]
    #[pyo3(name = "fee")]
    fn py_fee(&self) -> Option<u32> {
        self.fee
    }

    #[getter]
    #[pyo3(name = "tick_spacing")]
    fn py_tick_spacing(&self) -> Option<u32> {
        self.tick_spacing
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}
