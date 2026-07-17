// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Shared position application planning.

use nautilus_core::UUID4;
use nautilus_model::{
    events::OrderFilled,
    identifiers::PositionId,
    position::Position,
    types::{Money, Quantity},
};

/// The two fill fragments produced when a fill flips a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlipFragments {
    /// Fragment which closes the existing position.
    pub closing: OrderFilled,
    /// Fragment which opens the position on the opposite side.
    pub opening: OrderFilled,
}

/// Plans the deterministic fill fragments for a position flip.
///
/// # Panics
///
/// Panics if the commission split cannot be represented as a [`Money`].
#[must_use]
pub fn plan_position_flip(
    position: &Position,
    fill: &OrderFilled,
    opening_position_id: PositionId,
) -> FlipFragments {
    let opening_qty = Quantity::from_raw(
        fill.last_qty.raw.abs_diff(position.quantity.raw),
        position.size_precision,
    );
    let closing_fraction = position.quantity.as_decimal() / fill.last_qty.as_decimal();
    let (closing_commission, opening_commission) = if let Some(commission) = fill.commission {
        let closing = Money::from_decimal(
            commission.as_decimal() * closing_fraction,
            commission.currency,
        )
        .expect("Invalid split commission");
        (Some(closing), Some(commission - closing))
    } else {
        (None, None)
    };

    let mut closing = OrderFilled::new(
        fill.trader_id,
        fill.strategy_id,
        fill.instrument_id,
        fill.client_order_id,
        fill.venue_order_id,
        fill.account_id,
        fill.trade_id,
        fill.order_side,
        fill.order_type,
        position.quantity,
        fill.last_px,
        fill.currency,
        fill.liquidity_side,
        fill.event_id,
        fill.ts_event,
        fill.ts_init,
        fill.reconciliation,
        fill.position_id,
        closing_commission,
        fill.info.clone(),
    );
    closing.causation_id = fill.causation_id;

    let mut opening = OrderFilled::new(
        fill.trader_id,
        fill.strategy_id,
        fill.instrument_id,
        fill.client_order_id,
        fill.venue_order_id,
        fill.account_id,
        fill.trade_id,
        fill.order_side,
        fill.order_type,
        opening_qty,
        fill.last_px,
        fill.currency,
        fill.liquidity_side,
        flip_opening_event_id(fill.event_id),
        fill.ts_event,
        fill.ts_init,
        fill.reconciliation,
        Some(opening_position_id),
        opening_commission,
        fill.info.clone(),
    );
    opening.causation_id = Some(fill.event_id);

    FlipFragments { closing, opening }
}

/// Carries replay history forward when a closed netting position reuses its ID.
pub fn merge_reopened_position_history(prior: &Position, reopened: &mut Position) {
    if prior.id != reopened.id {
        return;
    }

    let current_replay = std::mem::take(&mut reopened.replay_events);
    reopened.replay_events.clone_from(&prior.replay_events);
    reopened.replay_events.extend(current_replay);
    reopened.fill_voids.clone_from(&prior.fill_voids);
}

fn flip_opening_event_id(source: UUID4) -> UUID4 {
    const DOMAIN: [u8; 16] = *b"NT-FLIP-OPEN-v1!";
    let mut bytes = source.as_bytes();
    for (byte, domain) in bytes.iter_mut().zip(DOMAIN) {
        *byte ^= domain;
    }
    UUID4::from_bytes(bytes)
}
