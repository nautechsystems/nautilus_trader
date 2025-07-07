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

    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
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

    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> crate::identifiers::InstrumentId {
        self.instrument_id
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
