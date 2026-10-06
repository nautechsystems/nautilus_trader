// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Interactive Brokers spread execution handling.

use nautilus_common::live::sender::EventSender;

use super::core::*;
use crate::{
    common::{enums::IbAction, spreads::parse_spread_instrument_id_to_legs},
    execution::parse,
};

const SPREAD_LEG_FILLS_TIMEOUT: Duration = Duration::from_secs(5);

impl InteractiveBrokersExecutionClient {
    // Returns the leg instrument of a new leg execution, or `None` for a duplicate
    pub(super) async fn handle_spread_execution(
        exec_data: &ExecutionData,
        fill: &SpreadFillContext<'_>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        orders: &OrderTracker,
        context: &TrackedOrder,
    ) -> anyhow::Result<Option<InstrumentId>> {
        let trade_id = TradeId::new(&exec_data.execution.execution_id);
        let fill_id = trade_id.to_string();

        {
            let mut state = orders.lock()?;
            let Some(order) = state.order_mut(exec_data.execution.order_id) else {
                anyhow::bail!(
                    "Tracked state not found for Interactive Brokers order {}",
                    exec_data.execution.order_id,
                );
            };

            if order.spread_fill_ids.contains(&fill_id) {
                tracing::debug!(
                    "Fill {} already processed for spread order {}, skipping",
                    fill_id,
                    fill.client_order_id,
                );
                return Ok(None);
            }

            order.spread_fill_ids.insert(fill_id);
        }

        // A leg execution carries the leg contract itself, so its instrument resolves as for a
        // plain fill and must be one of the spread's legs
        let leg_id = match instrument_provider
            .get_instrument_id_by_contract_id(exec_data.contract.contract_id)
        {
            Some(leg_id) => leg_id,
            None => Self::resolve_contract_instrument_id(instrument_provider, &exec_data.contract)?,
        };
        let spread_legs = parse_spread_instrument_id_to_legs(&fill.spread_instrument_id)?;
        anyhow::ensure!(
            spread_legs.iter().any(|(id, _)| *id == leg_id),
            "Execution instrument {leg_id} is not a leg of spread {}",
            fill.spread_instrument_id
        );

        Self::generate_leg_fill(
            exec_data,
            fill,
            context,
            leg_id,
            instrument_provider,
            exec_sender,
        )?;

        Ok(Some(leg_id))
    }

    pub(super) fn hold_spread_fill(
        orders: &OrderTracker,
        order_id: i32,
        fill: OrderFilled,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let released = {
            let mut state = orders.lock()?;
            let order = state.order_mut(order_id).with_context(|| {
                format!("Tracked state not found for Interactive Brokers order {order_id}")
            })?;
            order
                .spread_fills_held
                .fills
                .push_back((tokio::time::Instant::now(), fill));
            Self::release_spread_fills(&mut order.spread_fills_held)?
        };
        Self::send_spread_fills(released, exec_sender)
    }

    pub(super) fn record_spread_leg_fill(
        orders: &OrderTracker,
        order_id: i32,
        leg_instrument_id: InstrumentId,
        quantity: f64,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let released = {
            let mut state = orders.lock()?;
            let order = state.order_mut(order_id).with_context(|| {
                format!("Tracked state not found for Interactive Brokers order {order_id}")
            })?;
            *order
                .spread_fills_held
                .leg_quantities
                .entry(leg_instrument_id)
                .or_default() += quantity;
            Self::release_spread_fills(&mut order.spread_fills_held)?
        };
        Self::send_spread_fills(released, exec_sender)
    }

    // Sends spread fills held longer than the timeout without their legs, so a lost leg
    // execution cannot leave the spread order unfilled
    pub(super) fn flush_spread_fills_without_legs(
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let released = {
            let mut state = orders.lock()?;
            let order_ids: Vec<i32> = state
                .active_orders
                .iter()
                .chain(state.terminal_orders.iter())
                .filter(|(_, order)| !order.spread_fills_held.fills.is_empty())
                .map(|(order_id, _)| *order_id)
                .collect();
            let mut released = Vec::new();

            for order_id in order_ids {
                let Some(order) = state.order_mut(order_id) else {
                    continue;
                };
                let held = &mut order.spread_fills_held.fills;

                while held
                    .front()
                    .is_some_and(|(held_at, _)| held_at.elapsed() >= SPREAD_LEG_FILLS_TIMEOUT)
                {
                    let (_, fill) = held.pop_front().expect("front checked above");
                    tracing::warn!(
                        trade_id = %fill.trade_id,
                        "IB leg executions for spread fill did not arrive within 5 seconds; \
                         emitting the spread fill",
                    );
                    released.push(fill);
                }
            }
            released
        };
        Self::send_spread_fills(released, exec_sender)
    }

    // Releases held spread fills in arrival order while the sent leg quantities cover them
    fn release_spread_fills(held: &mut HeldSpreadFills) -> anyhow::Result<Vec<OrderFilled>> {
        const QUANTITY_TOLERANCE: f64 = 1e-9;
        let mut released = Vec::new();

        while let Some((_, fill)) = held.fills.front() {
            let required: Vec<(InstrumentId, f64)> =
                parse_spread_instrument_id_to_legs(&fill.instrument_id)?
                    .into_iter()
                    .map(|(leg_id, ratio)| {
                        (
                            leg_id,
                            fill.last_qty.as_f64() * f64::from(ratio.unsigned_abs()),
                        )
                    })
                    .collect();
            let covered = required.iter().all(|(leg_id, quantity)| {
                held.leg_quantities.get(leg_id).copied().unwrap_or_default()
                    >= quantity - QUANTITY_TOLERANCE
            });

            if !covered {
                break;
            }

            for (leg_id, quantity) in required {
                if let Some(sent) = held.leg_quantities.get_mut(&leg_id) {
                    *sent -= quantity;
                }
            }
            held.leg_quantities
                .retain(|_, quantity| *quantity > QUANTITY_TOLERANCE);
            let (_, fill) = held.fills.pop_front().expect("front checked above");
            released.push(fill);
        }
        Ok(released)
    }

    fn send_spread_fills(
        fills: Vec<OrderFilled>,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        for fill in fills {
            exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(fill)))?;
        }
        Ok(())
    }

    // Leg fills are `OrderFilled` events so the engine applies them to leg positions without an
    // order of their own; a report would be held for an order status no leg can have.
    pub(super) fn generate_leg_fill(
        exec_data: &ExecutionData,
        fill: &SpreadFillContext<'_>,
        context: &TrackedOrder,
        leg_instrument_id: InstrumentId,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let leg_instrument = instrument_provider
            .find(&leg_instrument_id)
            .context("Leg instrument not found")?;

        let price_magnifier = instrument_provider.get_price_magnifier(&leg_instrument_id) as f64;
        let execution_price = exec_data.execution.price * price_magnifier;
        let leg_price = Price::new(execution_price, leg_instrument.price_precision());

        let leg_quantity =
            Quantity::new(exec_data.execution.shares, leg_instrument.size_precision());

        let order_side = IbAction::from_str(exec_data.execution.side.as_str())?.order_side();

        let commission_money =
            Money::new(fill.commission, Currency::from(fill.commission_currency));

        let leg_position = Self::get_leg_position(&fill.spread_instrument_id, &leg_instrument_id);
        let leg_client_order_id = ClientOrderId::new(format!(
            "{}-LEG-{}",
            fill.client_order_id, leg_instrument_id.symbol
        ));
        let leg_trade_id = TradeId::new(format!(
            "{}-{}",
            exec_data.execution.execution_id, leg_position
        ));
        let venue_order_id =
            parse::ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id);
        let leg_venue_order_id =
            VenueOrderId::new(format!("{}-LEG-{}", venue_order_id.as_str(), leg_position));

        let ts_event = parse_execution_time(&exec_data.execution.time)?;

        let event = OrderFilled::new(
            context.trader_id,
            context.strategy_id,
            leg_instrument_id,
            leg_client_order_id,
            leg_venue_order_id,
            fill.account_id,
            leg_trade_id,
            order_side,
            context.order_type,
            leg_quantity,
            leg_price,
            leg_instrument.quote_currency(),
            LiquiditySide::NoLiquiditySide,
            UUID4::new(),
            ts_event,
            fill.ts_init,
            false,
            None,
            Some(commission_money),
            None,
        );
        exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(event)))?;

        tracing::debug!(
            "Generated leg fill: instrument_id={}, client_order_id={}, quantity={}, price={}",
            leg_instrument_id,
            leg_client_order_id,
            leg_quantity,
            leg_price
        );

        Ok(())
    }

    pub(super) fn get_leg_position(
        spread_instrument_id: &InstrumentId,
        leg_instrument_id: &InstrumentId,
    ) -> usize {
        let legs = match parse_spread_instrument_id_to_legs(spread_instrument_id) {
            Ok(legs) => legs,
            Err(e) => {
                log::warn!(
                    "Failed to parse spread instrument ID {spread_instrument_id} for leg position: {e}"
                );
                return 0;
            }
        };

        for (idx, (parsed_leg_id, _)) in legs.iter().enumerate() {
            if *parsed_leg_id == *leg_instrument_id {
                return idx;
            }
        }

        log::warn!(
            "Leg instrument ID {leg_instrument_id} not found in spread instrument ID {spread_instrument_id}"
        );
        0
    }
}
