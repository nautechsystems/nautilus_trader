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

//! Binary-option settlement from authoritative contract-expiration closes.
//!
//! The [`ExecutionEngine`](super::ExecutionEngine) applies these rules to closes it receives, and
//! event-store cache replay applies the same rules to captured closes.

use nautilus_common::cache::Cache;
use nautilus_model::{
    data::InstrumentClose,
    enums::InstrumentCloseType,
    identifiers::{InstrumentId, PositionId},
    instruments::InstrumentAny,
    position::Position,
    types::Quantity,
};

/// Settles every open position in the binary option named by a contract-expiration `close`.
///
/// The first contract-expiration close cached for the instrument is authoritative. A later close
/// settles any position still open on the cached terms, and a conflicting price is logged. Returns
/// a transient copy of each settled position, without stored history, with the quantity the
/// settlement closed. The list is empty when `close` is not a contract expiration for a cached
/// binary option. A position that cannot be settled is logged and skipped.
///
/// # Errors
///
/// Returns an error if caching the close fails.
pub fn settle_instrument_close(
    cache: &mut Cache,
    close: InstrumentClose,
) -> anyhow::Result<Vec<(Position, Quantity)>> {
    let instrument_id = close.instrument_id;

    if close.close_type != InstrumentCloseType::ContractExpired
        || !matches!(
            cache.instrument(&instrument_id),
            Some(InstrumentAny::BinaryOption(_))
        )
    {
        return Ok(Vec::new());
    }

    let close = if let Some(settled) = settlement_close(cache, &instrument_id) {
        if settled.close_price != close.close_price {
            log::error!(
                "Ignoring conflicting instrument close for {instrument_id}: settled at {}, received {}",
                settled.close_price,
                close.close_price,
            );
        }

        settled
    } else {
        cache.add_instrument_close(close)?;
        close
    };

    let positions: Vec<(PositionId, Quantity)> = cache
        .positions_open(None, Some(&instrument_id), None, None, None)
        .iter()
        .map(|position| (position.id, position.quantity))
        .collect();
    let mut settled = Vec::with_capacity(positions.len());

    for (position_id, last_qty) in positions {
        match cache.update_position_from_instrument_close(position_id, close) {
            Ok(position) => settled.push((position, last_qty)),
            Err(e) => log::error!("Cannot settle position {position_id}: {e}"),
        }
    }

    Ok(settled)
}

/// Returns the contract-expiration close that settled binary option `instrument_id`, if any.
#[must_use]
pub fn settlement_close(cache: &Cache, instrument_id: &InstrumentId) -> Option<InstrumentClose> {
    if !matches!(
        cache.instrument(instrument_id),
        Some(InstrumentAny::BinaryOption(_))
    ) {
        return None;
    }

    cache
        .instrument_close(instrument_id)
        .copied()
        .filter(|close| close.close_type == InstrumentCloseType::ContractExpired)
}
