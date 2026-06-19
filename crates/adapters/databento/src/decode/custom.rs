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

use std::ffi::c_char;

use databento::dbn;
use nautilus_core::UnixNanos;
use nautilus_model::{enums::FromU8, identifiers::InstrumentId, types::Quantity};

use super::primitives::{
    decode_optional_price, decode_optional_quantity, decode_price_or_undef, parse_order_side,
};
use crate::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::{DatabentoImbalance, DatabentoStatistics},
};

/// Decodes a Databento imbalance message into a `DatabentoImbalance` event.
///
/// # Errors
///
/// Returns an error if constructing `DatabentoImbalance` fails.
pub fn decode_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<DatabentoImbalance> {
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(DatabentoImbalance::new(
        instrument_id,
        decode_price_or_undef(msg.ref_price, price_precision),
        decode_price_or_undef(msg.cont_book_clr_price, price_precision),
        decode_price_or_undef(msg.auct_interest_clr_price, price_precision),
        Quantity::new(f64::from(msg.paired_qty), 0),
        Quantity::new(f64::from(msg.total_imbalance_qty), 0),
        parse_order_side(msg.side),
        msg.significant_imbalance as c_char,
        msg.hd.ts_event.into(),
        ts_event,
        ts_init,
    ))
}

/// Decodes a Databento statistics message into a `DatabentoStatistics` event.
///
/// # Errors
///
/// Returns an error if constructing `DatabentoStatistics` fails or if `msg.update_action`
/// is not a valid enum variant.
///
/// Returns `Ok(None)` when `msg.stat_type` does not map to a Nautilus variant: covers
/// `VenueSpecificVolume1` (10001) and `VenueSpecificPrice1` (10002), which exceed the
/// `u8` Arrow column width, plus any future dbn value.
pub fn decode_statistics_msg(
    msg: &dbn::StatMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Option<DatabentoStatistics>> {
    let Some(stat_type) = u8::try_from(msg.stat_type)
        .ok()
        .and_then(DatabentoStatisticType::from_u8)
    else {
        log::warn!(
            "Skipping unsupported `stat_type` {} for {instrument_id}",
            msg.stat_type,
        );
        return Ok(None);
    };
    let update_action =
        DatabentoStatisticUpdateAction::from_u8(msg.update_action).ok_or_else(|| {
            anyhow::anyhow!("Invalid value for `update_action`: {}", msg.update_action)
        })?;
    let ts_event = msg.ts_recv.into();
    let ts_init = ts_init.unwrap_or(ts_event);

    Ok(Some(DatabentoStatistics::new(
        instrument_id,
        stat_type,
        update_action,
        decode_optional_price(msg.price, price_precision),
        decode_optional_quantity(msg.quantity),
        msg.channel_id,
        msg.stat_flags,
        msg.sequence,
        msg.ts_ref.into(),
        msg.ts_in_delta,
        msg.hd.ts_event.into(),
        ts_event,
        ts_init,
    )))
}

/// Returns `true` if `stat_type` maps to a modeled [`DatabentoStatisticType`] variant.
///
/// Callers should precheck with this function before resolving price precision or other
/// per-record setup, so unmodeled records can be skipped without surfacing unrelated
/// errors.
#[must_use]
pub fn is_supported_stat_type(stat_type: u16) -> bool {
    u8::try_from(stat_type)
        .ok()
        .and_then(DatabentoStatisticType::from_u8)
        .is_some()
}
