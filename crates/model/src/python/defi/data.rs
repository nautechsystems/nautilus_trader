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
        data::{
            Block, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolLiquidityUpdateType,
            PoolSwap, Transaction,
        },
    },
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

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
        receiver: String,
        amount0: String,
        amount1: String,
        sqrt_price_x96: String,
        liquidity: u128,
        tick: i32,
        side: Option<OrderSide>,
        size: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<Self> {
        let sender = sender.parse().map_err(to_pyvalue_err)?;
        let receiver = receiver.parse().map_err(to_pyvalue_err)?;
        let amount0 = amount0.parse().map_err(to_pyvalue_err)?;
        let amount1 = amount1.parse().map_err(to_pyvalue_err)?;
        let sqrt_price_x96 = sqrt_price_x96.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            instrument_id,
            Address::from_str(&pool_address).map_err(to_pyvalue_err)?,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            Some(timestamp.into()),
            sender,
            receiver,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
            side,
            size,
            price,
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
    fn py_side(&self) -> Option<OrderSide> {
        self.side
    }

    #[getter]
    #[pyo3(name = "size")]
    fn py_size(&self) -> Option<Quantity> {
        self.size
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Option<Price> {
        self.price
    }

    #[getter]
    #[pyo3(name = "timestamp")]
    fn py_timestamp(&self) -> Option<u64> {
        self.timestamp.map(|x| x.as_u64())
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> Option<u64> {
        self.ts_init.map(|x| x.as_u64())
    }
}

#[pymethods]
impl PoolLiquidityUpdate {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        pool_address: String,
        instrument_id: InstrumentId,
        kind: PoolLiquidityUpdateType,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: Option<String>,
        owner: String,
        position_liquidity: String,
        amount0: String,
        amount1: String,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: u64,
    ) -> PyResult<Self> {
        let sender = sender
            .map(|s| s.parse())
            .transpose()
            .map_err(to_pyvalue_err)?;
        let owner = owner.parse().map_err(to_pyvalue_err)?;
        let position_liquidity = position_liquidity.parse().map_err(to_pyvalue_err)?;
        let amount0 = amount0.parse().map_err(to_pyvalue_err)?;
        let amount1 = amount1.parse().map_err(to_pyvalue_err)?;
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
            Some(timestamp.into()),
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
    fn py_position_liquidity(&self) -> String {
        self.position_liquidity.to_string()
    }

    #[getter]
    #[pyo3(name = "amount0")]
    fn py_amount0(&self) -> String {
        self.amount0.to_string()
    }

    #[getter]
    #[pyo3(name = "amount1")]
    fn py_amount1(&self) -> String {
        self.amount1.to_string()
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
    fn py_timestamp(&self) -> Option<u64> {
        self.timestamp.map(|x| x.as_u64())
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> Option<u64> {
        self.ts_init.map(|x| x.as_u64())
    }
}

#[pymethods]
impl PoolFeeCollect {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        pool_address: String,
        instrument_id: InstrumentId,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        owner: String,
        amount0: String,
        amount1: String,
        tick_lower: i32,
        tick_upper: i32,
        timestamp: u64,
    ) -> PyResult<Self> {
        let owner = owner.parse().map_err(to_pyvalue_err)?;
        let amount0 = amount0.parse().map_err(to_pyvalue_err)?;
        let amount1 = amount1.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            instrument_id,
            Address::from_str(&pool_address).map_err(to_pyvalue_err)?,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            owner,
            amount0,
            amount1,
            tick_lower,
            tick_upper,
            Some(timestamp.into()),
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
        self.transaction_hash.hash(&mut hasher);
        self.log_index.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for PoolFeeCollect"),
        }
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
    #[pyo3(name = "owner")]
    fn py_owner(&self) -> String {
        self.owner.to_string()
    }

    #[getter]
    #[pyo3(name = "amount0")]
    fn py_amount0(&self) -> String {
        self.amount0.to_string()
    }

    #[getter]
    #[pyo3(name = "amount1")]
    fn py_amount1(&self) -> String {
        self.amount1.to_string()
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
    fn py_timestamp(&self) -> Option<u64> {
        self.timestamp.map(|x| x.as_u64())
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> Option<u64> {
        self.ts_init.map(|x| x.as_u64())
    }
}

#[pymethods]
impl PoolFlash {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        pool_address: String,
        instrument_id: InstrumentId,
        block: u64,
        transaction_hash: String,
        transaction_index: u32,
        log_index: u32,
        sender: String,
        recipient: String,
        amount0: String,
        amount1: String,
        paid0: String,
        paid1: String,
        timestamp: u64,
    ) -> PyResult<Self> {
        let sender = sender.parse().map_err(to_pyvalue_err)?;
        let recipient = recipient.parse().map_err(to_pyvalue_err)?;
        let amount0 = amount0.parse().map_err(to_pyvalue_err)?;
        let amount1 = amount1.parse().map_err(to_pyvalue_err)?;
        let paid0 = paid0.parse().map_err(to_pyvalue_err)?;
        let paid1 = paid1.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            Arc::new(dex),
            instrument_id,
            Address::from_str(&pool_address).map_err(to_pyvalue_err)?,
            block,
            transaction_hash,
            transaction_index,
            log_index,
            Some(timestamp.into()),
            sender,
            recipient,
            amount0,
            amount1,
            paid0,
            paid1,
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
        self.transaction_hash.hash(&mut hasher);
        self.log_index.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self == other,
            CompareOp::Ne => self != other,
            _ => panic!("Unsupported comparison for PoolFlash"),
        }
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
    #[pyo3(name = "recipient")]
    fn py_recipient(&self) -> String {
        self.recipient.to_string()
    }

    #[getter]
    #[pyo3(name = "amount0")]
    fn py_amount0(&self) -> String {
        self.amount0.to_string()
    }

    #[getter]
    #[pyo3(name = "amount1")]
    fn py_amount1(&self) -> String {
        self.amount1.to_string()
    }

    #[getter]
    #[pyo3(name = "paid0")]
    fn py_paid0(&self) -> String {
        self.paid0.to_string()
    }

    #[getter]
    #[pyo3(name = "paid1")]
    fn py_paid1(&self) -> String {
        self.paid1.to_string()
    }

    #[getter]
    #[pyo3(name = "timestamp")]
    fn py_timestamp(&self) -> Option<u64> {
        self.ts_event.map(|x| x.as_u64())
    }
}

#[pymethods]
impl Transaction {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        hash: String,
        block_hash: String,
        block_number: u64,
        from: String,
        to: String,
        gas: String,
        gas_price: String,
        transaction_index: u64,
        value: String,
    ) -> PyResult<Self> {
        let from = from.parse().map_err(to_pyvalue_err)?;
        let to = to.parse().map_err(to_pyvalue_err)?;
        let gas = gas.parse().map_err(to_pyvalue_err)?;
        let gas_price = gas_price.parse().map_err(to_pyvalue_err)?;
        let value = value.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            chain,
            hash,
            block_hash,
            block_number,
            from,
            to,
            gas,
            gas_price,
            transaction_index,
            value,
        ))
    }

    fn __str__(&self) -> String {
        format!(
            "Transaction(chain={}, hash={}, block_number={}, from={}, to={}, value={})",
            self.chain.name, self.hash, self.block_number, self.from, self.to, self.value
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash.hash(&mut hasher);
        hasher.finish()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp) -> bool {
        match op {
            CompareOp::Eq => self.hash == other.hash,
            CompareOp::Ne => self.hash != other.hash,
            _ => panic!("Unsupported comparison for Transaction"),
        }
    }

    #[getter]
    #[pyo3(name = "chain")]
    fn py_chain(&self) -> Chain {
        self.chain.clone()
    }

    #[getter]
    #[pyo3(name = "hash")]
    fn py_hash(&self) -> &str {
        &self.hash
    }

    #[getter]
    #[pyo3(name = "block_hash")]
    fn py_block_hash(&self) -> &str {
        &self.block_hash
    }

    #[getter]
    #[pyo3(name = "block_number")]
    fn py_block_number(&self) -> u64 {
        self.block_number
    }

    #[getter]
    #[pyo3(name = "from")]
    fn py_from(&self) -> String {
        self.from.to_string()
    }

    #[getter]
    #[pyo3(name = "to")]
    fn py_to(&self) -> String {
        self.to.to_string()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> String {
        self.value.to_string()
    }

    #[getter]
    #[pyo3(name = "transaction_index")]
    fn py_transaction_index(&self) -> u64 {
        self.transaction_index
    }

    #[getter]
    #[pyo3(name = "gas")]
    fn py_gas(&self) -> String {
        self.gas.to_string()
    }

    #[getter]
    #[pyo3(name = "gas_price")]
    fn py_gas_price(&self) -> String {
        self.gas_price.to_string()
    }
}
