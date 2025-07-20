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

//! Python bindings for DeFi data types.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
};

use alloy_primitives::Address;
use nautilus_core::python::to_pyvalue_err;
use pyo3::{basic::CompareOp, prelude::*};

use crate::{
    defi::{
        Chain, Dex,
        chain::Blockchain,
        data::{Block, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap},
    },
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

#[pymethods]
impl PoolSwap {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        instrument_id: InstrumentId,
        pool_address: String,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        timestamp: u64,
        sender: String,
        side: OrderSide,
        size: Quantity,
        price: Price,
    ) -> PyResult<Self> {
        let sender = sender.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            instrument_id,
            Address::from_str(&pool_address).map_err(to_pyvalue_err)?,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            timestamp.into(),
            sender,
            side,
            size,
            price,
        ))
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> PyResult<Chain> {
        Ok(self.chain.as_ref().clone())
    }

    #[getter]
    #[pyo3(name = "dex")]
    fn py_dex(&self) -> PyResult<Dex> {
        Ok(self.dex.as_ref().clone())
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "pool_address")]
    fn py_pool_address(&self) -> String {
        self.pool_address.to_string()
    }

    #[getter]
    #[pyo3(name = "block")]
    fn py_block(&self) -> u64 {
        self.block
    }

    #[getter]
    #[pyo3(name = "transaction_hash")]
    fn py_transaction_hash(&self) -> &str {
        &self.transaction_hash
    }

    #[getter]
    #[pyo3(name = "transaction_index")]
    fn py_transaction_index(&self) -> u32 {
        self.transaction_index
    }

    #[getter]
    #[pyo3(name = "log_index")]
    fn py_log_index(&self) -> u32 {
        self.log_index
    }

    #[getter]
    #[pyo3(name = "sender")]
    fn py_sender(&self) -> String {
        self.sender.to_string()
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "size")]
    fn py_size(&self) -> Quantity {
        self.size
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price
    }

    #[getter]
    #[pyo3(name = "timestamp")]
    fn py_timestamp(&self) -> u64 {
        self.timestamp.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
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
        self.transaction_hash.hash(&mut hasher);
        self.log_index.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for PoolSwap"),
        }
    }
}

#[pymethods]
impl PoolLiquidityUpdate {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        instrument_id: InstrumentId,
        pool_address: String,
        kind: PoolLiquidityUpdateType,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Option<String>,
        owner: String,
        position_liquidity: Quantity,
        amount0: Quantity,
        amount1: Quantity,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: u64,
    ) -> PyResult<Self> {
        let sender = sender
            .map(|s| s.parse())
            .transpose()
            .map_err(to_pyvalue_err)?;
        let owner = owner.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            instrument_id,
            Address::from_str(&pool_address).map_err(to_pyvalue_err)?,
            kind,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            sender,
            owner,
            position_liquidity,
            amount0,
            amount1,
            tick_lower,
            tick_upper,
            timestamp.into(),
        ))
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> PyResult<Chain> {
        Ok(self.chain.as_ref().clone())
    }

    #[getter]
    #[pyo3(name = "dex")]
    fn py_dex(&self) -> PyResult<Dex> {
        Ok(self.dex.as_ref().clone())
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> crate::identifiers::InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "pool_address")]
    fn py_pool_address(&self) -> String {
        self.pool_address.to_string()
    }

    #[getter]
    #[pyo3(name = "kind")]
    fn py_kind(&self) -> PoolLiquidityUpdateType {
        self.kind
    }

    #[getter]
    #[pyo3(name = "block")]
    fn py_block(&self) -> u64 {
        self.block
    }

    #[getter]
    #[pyo3(name = "transaction_hash")]
    fn py_transaction_hash(&self) -> &str {
        &self.transaction_hash
    }

    #[getter]
    #[pyo3(name = "transaction_index")]
    fn py_transaction_index(&self) -> u32 {
        self.transaction_index
    }

    #[getter]
    #[pyo3(name = "log_index")]
    fn py_log_index(&self) -> u32 {
        self.log_index
    }

    #[getter]
    #[pyo3(name = "sender")]
    fn py_sender(&self) -> Option<String> {
        self.sender.map(|s| s.to_string())
    }

    #[getter]
    #[pyo3(name = "owner")]
    fn py_owner(&self) -> String {
        self.owner.to_string()
    }

    #[getter]
    #[pyo3(name = "position_liquidity")]
    fn py_position_liquidity(&self) -> Quantity {
        self.position_liquidity
    }

    #[getter]
    #[pyo3(name = "amount0")]
    fn py_amount0(&self) -> Quantity {
        self.amount0
    }

    #[getter]
    #[pyo3(name = "amount1")]
    fn py_amount1(&self) -> Quantity {
        self.amount1
    }

    #[getter]
    #[pyo3(name = "tick_lower")]
    fn py_tick_lower(&self) -> i32 {
        self.tick_lower
    }

    #[getter]
    #[pyo3(name = "tick_upper")]
    fn py_tick_upper(&self) -> i32 {
        self.tick_upper
    }

    #[getter]
    #[pyo3(name = "timestamp")]
    fn py_timestamp(&self) -> u64 {
        self.timestamp.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
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
        self.transaction_hash.hash(&mut hasher);
        self.log_index.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for PoolLiquidityUpdate"),
        }
    }
}

#[pymethods]
impl Block {
    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> Option<Blockchain> {
        self.chain
    }

    #[getter]
    #[pyo3(name = "hash")]
    fn py_hash(&self) -> &str {
        &self.hash
    }

    #[getter]
    #[pyo3(name = "number")]
    fn py_number(&self) -> u64 {
        self.number
    }

    #[getter]
    #[pyo3(name = "parent_hash")]
    fn py_parent_hash(&self) -> &str {
        &self.parent_hash
    }

    #[getter]
    #[pyo3(name = "miner")]
    fn py_miner(&self) -> &str {
        &self.miner
    }

    #[getter]
    #[pyo3(name = "gas_limit")]
    fn py_gas_limit(&self) -> u64 {
        self.gas_limit
    }

    #[getter]
    #[pyo3(name = "gas_used")]
    fn py_gas_used(&self) -> u64 {
        self.gas_used
    }

    #[getter]
    #[pyo3(name = "base_fee_per_gas")]
    fn py_base_fee_per_gas(&self) -> Option<String> {
        self.base_fee_per_gas.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "blob_gas_used")]
    fn py_blob_gas_used(&self) -> Option<String> {
        self.blob_gas_used.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "excess_blob_gas")]
    fn py_excess_blob_gas(&self) -> Option<String> {
        self.excess_blob_gas.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "l1_gas_price")]
    fn py_l1_gas_price(&self) -> Option<String> {
        self.l1_gas_price.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "l1_gas_used")]
    fn py_l1_gas_used(&self) -> Option<u64> {
        self.l1_gas_used
    }

    #[getter]
    #[pyo3(name = "l1_fee_scalar")]
    fn py_l1_fee_scalar(&self) -> Option<u64> {
        self.l1_fee_scalar
    }

    #[getter]
    #[pyo3(name = "timestamp")]
    fn py_timestamp(&self) -> u64 {
        self.timestamp.as_u64()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash.hash(&mut hasher);
        hasher.finish()
    }
}
