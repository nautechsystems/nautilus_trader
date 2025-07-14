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
    defi::{AmmType, Blockchain, Chain, Dex, Pool, Token},
    identifiers::InstrumentId,
};

#[pymethods]
impl Chain {
    #[new]
    fn py_new(name: Blockchain, chain_id: u32) -> Self {
        Self::new(name, chain_id)
    }

    #[pyo3(name = "set_rpc_url")]
    fn py_set_rpc_url(&mut self, rpc_url: String) {
        self.set_rpc_url(rpc_url);
    }

    #[staticmethod]
    #[pyo3(name = "from_chain_id")]
    fn py_from_chain_id(chain_id: u32) -> Option<Chain> {
        Self::from_chain_id(chain_id).cloned()
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
}

#[pymethods]
impl Token {
    #[new]
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
}

#[pymethods]
impl Dex {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        name: String,
        factory: String,
        factory_creation_block: u64,
        amm_type: String,
        pool_created_event: String,
        swap_event: String,
        mint_event: String,
        burn_event: String,
    ) -> PyResult<Self> {
        let amm_type = AmmType::from_str(&amm_type).map_err(to_pyvalue_err)?;
        Ok(Self::new(
            chain,
            name,
            factory,
            factory_creation_block,
            amm_type,
            pool_created_event,
            swap_event,
            mint_event,
            burn_event,
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
}

#[pymethods]
impl Pool {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        chain: Chain,
        dex: Dex,
        address: String,
        creation_block: u64,
        token0: Token,
        token1: Token,
        fee: u32,
        tick_spacing: u32,
        ts_init: u64,
    ) -> PyResult<Self> {
        let address = address.parse().map_err(to_pyvalue_err)?;
        Ok(Self::new(
            Arc::new(chain),
            dex,
            address,
            creation_block,
            token0,
            token1,
            fee,
            tick_spacing,
            ts_init.into(),
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
}
