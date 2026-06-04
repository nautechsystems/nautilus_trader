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

//! Python bindings for loading blockchain cache state.

use nautilus_common::live::get_runtime;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::{PoolIdentifier, pool_analysis::PoolSnapshot};
use pyo3::prelude::*;

use crate::cache::database::BlockchainCacheDatabase;

/// Loads the latest pool snapshot from the Postgres cache for backtest replay.
///
/// Connects to the cache database and reconstructs the most recent snapshot for the pool,
/// including its full position and tick state, so a pool profiler can initialize before
/// historical swaps and liquidity updates are replayed. When `before_block` is set, only
/// snapshots at or before that block are considered, so the snapshot aligns with the replay
/// start. When `require_valid` is `true`, only snapshots validated against on-chain state are
/// returned. Returns `None` when no matching snapshot exists in the cache.
///
/// # Errors
///
/// Returns a `PyErr` if `pool_address` is not a valid pool identifier, or if the database
/// connection or query fails.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.blockchain")]
#[pyo3(name = "load_pool_snapshot")]
#[pyo3(signature = (pg_config, chain_id, pool_address, before_block=None, require_valid=true))]
#[gen_stub(
    override_return_type(
        type_repr = "typing.Optional[nautilus_trader.model.PoolSnapshot]",
        imports = ("typing", "nautilus_trader.model"),
    ),
)]
pub fn py_load_pool_snapshot(
    #[gen_stub(
        override_type(
            type_repr = "nautilus_trader.infrastructure.PostgresConnectOptions",
            imports = ("nautilus_trader.infrastructure",),
        ),
    )]
    pg_config: PostgresConnectOptions,
    chain_id: u32,
    pool_address: &str,
    before_block: Option<u64>,
    require_valid: bool,
) -> PyResult<Option<PoolSnapshot>> {
    let pool_identifier = PoolIdentifier::new_checked(pool_address).map_err(to_pyvalue_err)?;

    get_runtime().block_on(async move {
        let database = BlockchainCacheDatabase::connect(pg_config.into())
            .await
            .map_err(to_pyruntime_err)?;
        database
            .load_latest_pool_snapshot(chain_id, &pool_identifier, before_block, require_valid)
            .await
            .map_err(to_pyruntime_err)
    })
}
