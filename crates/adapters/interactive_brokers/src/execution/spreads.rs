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

impl InteractiveBrokersExecutionClient {
    pub(super) async fn handle_spread_execution(
        exec_data: &ExecutionData,
        fill: &SpreadFillContext<'_>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        orders: &OrderTracker,
        _context: &TrackedOrder,
    ) -> anyhow::Result<()> {
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
                return Ok(());
            }

            order.spread_fill_ids.insert(fill_id);
        }

        let (leg_id, _) = Self::get_leg_instrument_id_and_ratio(
            &exec_data.contract,
            &fill.leg_instrument_id,
            instrument_provider,
        );
        Self::generate_leg_fill(exec_data, fill, leg_id, instrument_provider, exec_sender)?;

        Ok(())
    }

    pub(super) fn get_leg_instrument_id_and_ratio(
        contract: &ibapi::contracts::Contract,
        leg_instrument_id: &InstrumentId,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
    ) -> (InstrumentId, i32) {
        if let Some(leg_id) =
            instrument_provider.get_instrument_id_by_contract_id(contract.contract_id)
            && let Some(combo_leg) = contract.combo_legs.iter().find(|leg| {
                if let Some(matched_id) =
                    instrument_provider.get_instrument_id_by_contract_id(leg.contract_id)
                {
                    matched_id == leg_id
                } else {
                    false
                }
            })
        {
            let ratio = IbAction::from_str(combo_leg.action.as_str())
                .map_or(-combo_leg.ratio, |action| {
                    action.signed_multiplier() * combo_leg.ratio
                });
            return (leg_id, ratio);
        }

        (*leg_instrument_id, 1)
    }

    pub(super) fn generate_leg_fill(
        exec_data: &ExecutionData,
        fill: &SpreadFillContext<'_>,
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

        let mut fill_report = FillReport::new(
            fill.account_id,
            leg_instrument_id,
            leg_venue_order_id,
            leg_trade_id,
            order_side,
            leg_quantity,
            leg_price,
            commission_money,
            LiquiditySide::NoLiquiditySide,
            Some(leg_client_order_id),
            None,
            ts_event,
            fill.ts_init,
            None,
        );

        if let Some(price) = fill.avg_px {
            fill_report.avg_px = Some(price.as_decimal());
        }

        exec_sender.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
            fill_report,
        ))))?;

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
