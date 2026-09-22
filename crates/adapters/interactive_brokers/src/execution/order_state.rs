// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Interactive Brokers order tracking and cached instrument state.

use nautilus_common::live::sender::EventSender;

use super::core::*;
use crate::common::spreads::parse_spread_instrument_id_to_legs;

impl OrderTrackerState {
    pub(super) fn correlate(
        &self,
        client_id: i32,
        order_id: i32,
        perm_id: i64,
        order_ref: &str,
    ) -> anyhow::Result<OrderCorrelation> {
        let reference = parse::normalized_order_ref(order_ref).map(ClientOrderId::from);
        if let Some((primary_order_id, context, account_id)) = self.group_context(perm_id) {
            let child_id = ClientOrderId::for_duplicate_order(
                account_id,
                parse::ib_venue_order_id(0, perm_id),
            )?;
            anyhow::ensure!(
                reference.is_none_or(|id| id == context.client_order_id || id == child_id),
                "IB permanent ID {perm_id} has a conflicting order reference {order_ref}"
            );
            return if perm_id == context.perm_id {
                Ok(OrderCorrelation::Tracked {
                    order_id: primary_order_id,
                    context,
                })
            } else {
                Ok(OrderCorrelation::Duplicate {
                    order_id: primary_order_id,
                    context,
                })
            };
        }

        let raw = (client_id == self.client_id)
            .then(|| self.order(order_id).map(|order| (order_id, order.clone())))
            .flatten();
        let by_perm = if perm_id > 0 {
            self.active_orders
                .iter()
                .chain(self.terminal_orders.iter())
                .find(|(_, order)| order.perm_id == perm_id)
                .map(|(id, order)| (*id, order.clone()))
        } else {
            None
        };
        let by_ref = reference.and_then(|reference| {
            self.order_id_map
                .get(&reference)
                .and_then(|id| self.order(*id).map(|order| (*id, order.clone())))
                .or_else(|| {
                    self.active_orders
                        .iter()
                        .chain(self.terminal_orders.iter())
                        .find(|(_, order)| order.client_order_id == reference)
                        .map(|(id, order)| (*id, order.clone()))
                })
        });
        let identities: Vec<_> = [raw.as_ref(), by_perm.as_ref(), by_ref.as_ref()]
            .into_iter()
            .flatten()
            .collect();

        if let Some((_, first)) = identities.first() {
            anyhow::ensure!(
                identities
                    .iter()
                    .all(|(_, order)| order.client_order_id == first.client_order_id),
                "Conflicting IB order identities for client {client_id}, order {order_id}, permanent ID {perm_id}"
            );
            anyhow::ensure!(
                reference.is_none_or(|id| id == first.client_order_id),
                "IB order reference {order_ref} conflicts with tracked order {}",
                first.client_order_id
            );
        }

        if client_id != self.client_id && by_ref.is_some() && by_perm.is_none() {
            return Ok(OrderCorrelation::Untracked {
                client_order_id: None,
            });
        }
        let Some((tracking_id, mut context)) = by_ref.or(by_perm).or(raw) else {
            return Ok(OrderCorrelation::Untracked {
                client_order_id: reference,
            });
        };

        if context.perm_id > 0 && perm_id > 0 && context.perm_id != perm_id {
            return Ok(OrderCorrelation::Duplicate {
                order_id: tracking_id,
                context,
            });
        }

        if context.perm_id == 0 && perm_id > 0 {
            context.perm_id = perm_id;
        }
        Ok(OrderCorrelation::Tracked {
            order_id: tracking_id,
            context,
        })
    }
}

impl InteractiveBrokersExecutionClient {
    pub(super) fn cached_instrument_ids_for_preload(
        cache: &Cache,
        instrument_provider: &InteractiveBrokersInstrumentProvider,
        client_id: ClientId,
        account_id: AccountId,
    ) -> Vec<InstrumentId> {
        let mut instrument_ids = ahash::AHashSet::new();

        for client_order_id in cache.iter_client_order_ids(None, None, None, None) {
            if cache
                .client_id(&client_order_id)
                .is_some_and(|id| *id != client_id)
            {
                continue;
            }

            if let Some(order) = cache.order(&client_order_id) {
                if order.account_id().is_some_and(|id| id != account_id) {
                    continue;
                }
                let instrument_id = order.instrument_id();
                if instrument_provider.find(&instrument_id).is_none() {
                    instrument_ids.insert(instrument_id);
                }
            }
        }
        let mut instrument_ids: Vec<_> = instrument_ids.into_iter().collect();
        instrument_ids.sort_unstable();
        instrument_ids
    }

    pub(super) async fn preload_cached_instruments(&self, client: &Client) {
        let instrument_ids = {
            let cache = self.core.cache();
            self.instrument_provider.seed_from_cache(&cache);
            Self::cached_instrument_ids_for_preload(
                &cache,
                &self.instrument_provider,
                self.core.client_id,
                self.core.account_id,
            )
        };

        for instrument_id in instrument_ids {
            match self
                .instrument_provider
                .load_with_return_async(client, instrument_id, None)
                .await
            {
                Ok(Some(loaded)) if loaded == instrument_id => {
                    tracing::debug!("Preloaded cached IB instrument {instrument_id}");
                }
                Ok(_) => tracing::warn!(
                    "Cached IB instrument {instrument_id} is unavailable, its cached orders cannot be reconciled"
                ),
                Err(e) => tracing::warn!(
                    "Failed to preload cached IB instrument {instrument_id}, its cached orders cannot be reconciled: {e:#}"
                ),
            }
        }
    }

    // A combo order's report resolves only once its spread is cached, and startup
    // reconciliation needs the spread and its legs in the cache, which the data client fills
    // from its own load set alone. Published here at connect, they reach the cache before
    // reconciliation runs.
    pub(super) async fn publish_combo_order_instruments(&self, client: &Client) {
        let mut contracts = Vec::new();
        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        let queries = tokio::time::timeout(timeout_dur, async {
            let mut open = client.all_open_orders().await?;
            while let Some(item) = open.next().await {
                if let SubscriptionItem::Data(Orders::OrderData(data)) = item? {
                    contracts.push(data.contract);
                }
            }
            let mut completed = client.completed_orders(false).await?;
            while let Some(item) = completed.next().await {
                if let SubscriptionItem::Data(Orders::OrderData(data)) = item? {
                    contracts.push(data.contract);
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;

        match queries {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("Failed to read IB orders for combo instruments: {e:#}"),
            Err(_) => tracing::warn!("Timed out reading IB orders for combo instruments"),
        }

        let mut loaded = AHashSet::new();

        for contract in contracts
            .iter()
            .filter(|contract| contract.security_type == SecurityType::Spread)
        {
            if !loaded.insert(contract.contract_id) {
                continue;
            }

            if let Err(e) = self.publish_combo_instruments(client, contract).await {
                tracing::warn!(
                    "Failed to load IB combo contract ID {}, its orders cannot be reconciled: {e:#}",
                    contract.contract_id
                );
            }
        }
    }

    async fn publish_combo_instruments(
        &self,
        client: &Client,
        contract: &Contract,
    ) -> anyhow::Result<()> {
        // IB serves contract details for the legs but not for a BAG itself
        for combo_leg in &contract.combo_legs {
            if self
                .instrument_provider
                .get_instrument_id_by_contract_id(combo_leg.contract_id)
                .is_none()
            {
                let leg_contract = Contract {
                    contract_id: combo_leg.contract_id,
                    exchange: Exchange::from(combo_leg.exchange.as_str()),
                    ..Default::default()
                };
                self.instrument_provider
                    .get_instrument(client, &leg_contract)
                    .await?
                    .with_context(|| {
                        format!(
                            "IB returned no instrument for BAG leg {}",
                            combo_leg.contract_id
                        )
                    })?;
            }
        }

        let spread_id = self
            .instrument_provider
            .spread_instrument_id_for_contract(contract)?;
        self.instrument_provider
            .fetch_spread_instrument(client, spread_id, false, None)
            .await?;
        let spread = self
            .instrument_provider
            .find(&spread_id)
            .with_context(|| format!("Spread instrument {spread_id} was not loaded"))?;
        let legs = parse_spread_instrument_id_to_legs(&spread_id)?;
        let data_sender = get_data_event_sender();

        for instrument in legs
            .iter()
            .filter_map(|(leg_id, _)| self.instrument_provider.find(leg_id))
            .chain(std::iter::once(spread))
        {
            if self.core.cache().instrument(&instrument.id()).is_none()
                && let Err(e) = data_sender.send(DataEvent::Instrument(instrument))
            {
                tracing::warn!("Failed to publish IB spread instrument: {e}");
            }
        }

        Ok(())
    }

    // Startup reconciliation recovers a venue position only when its instrument is in the
    // cache, which the data client fills from its own load set alone.
    pub(super) async fn publish_position_instruments(
        &self,
        client: &Client,
        contracts: Vec<Contract>,
    ) {
        let data_sender = get_data_event_sender();

        for contract in contracts {
            let instrument = match self
                .instrument_provider
                .get_instrument(client, &contract)
                .await
            {
                Ok(Some(instrument)) => instrument,
                Ok(None) => {
                    tracing::warn!(
                        "Cannot resolve instrument for IB position contract ID {}, its position cannot be reconciled",
                        contract.contract_id
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to resolve instrument for IB position contract ID {}, its position cannot be reconciled: {e:#}",
                        contract.contract_id
                    );
                    continue;
                }
            };

            if self.core.cache().instrument(&instrument.id()).is_some() {
                continue;
            }

            if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
                tracing::warn!("Failed to publish IB position instrument: {e}");
            }
        }
    }

    pub(super) fn get_mapped_instrument_id(
        order_id: i32,
        orders: &OrderTracker,
    ) -> anyhow::Result<Option<InstrumentId>> {
        Ok(orders
            .lock()?
            .active_orders
            .get(&order_id)
            .map(|order| order.instrument_id))
    }

    pub(super) fn get_required_order_actor_ids(
        order_id: i32,
        orders: &OrderTracker,
    ) -> anyhow::Result<(TraderId, StrategyId)> {
        let state = orders.lock()?;
        let order = state.active_orders.get(&order_id).with_context(|| {
            format!("Tracked state not found for Interactive Brokers order {order_id}")
        })?;

        Ok((order.trader_id, order.strategy_id))
    }

    pub(super) fn resolve_contract_for_instrument(
        instrument_id: InstrumentId,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
    ) -> anyhow::Result<ibapi::contracts::Contract> {
        instrument_provider
            .resolve_contract_for_instrument(instrument_id)
            .context("Failed to convert instrument ID to IB contract")
    }

    pub(super) fn contract_with_order_exchange_param(
        mut contract: ibapi::contracts::Contract,
        params: Option<&nautilus_core::Params>,
    ) -> anyhow::Result<ibapi::contracts::Contract> {
        let Some(params) = params else {
            return Ok(contract);
        };

        let Some(exchange_value) = params.get("exchange") else {
            return Ok(contract);
        };

        let Some(exchange) = exchange_value.as_str() else {
            anyhow::bail!("`exchange` order param must be a string");
        };

        if exchange.is_empty() {
            return Ok(contract);
        }

        contract.exchange = ibapi::contracts::Exchange::from(exchange);
        Ok(contract)
    }

    #[allow(clippy::too_many_arguments)] // The fields form one tracked order record.
    pub(super) fn cache_order_tracking(
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order_side: OrderSide,
        order_type: OrderType,
        orders: &OrderTracker,
    ) -> anyhow::Result<()> {
        let mut state = orders.lock()?;
        state.order_id_map.insert(client_order_id, ib_order_id);
        state
            .venue_order_id_map
            .insert(ib_order_id, client_order_id);
        state.terminal_orders.remove(&ib_order_id);
        state.active_orders.insert(
            ib_order_id,
            TrackedOrder {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                order_side,
                order_type,
                accepted: false,
                avg_px: None,
                pending_cancel: false,
                pending_modify: None,
                perm_id: 0,
                spread_fill_ids: ahash::AHashSet::new(),
                last_update: None,
            },
        );

        Ok(())
    }

    pub(super) fn cache_recovered_order_tracking(
        ib_order_id: i32,
        target_order: &OrderAny,
        orders: &OrderTracker,
    ) -> anyhow::Result<()> {
        let client_order_id = target_order.client_order_id();
        // Publish the raw IB route only after the recovered identity is complete.
        let mut state = orders.lock()?;
        let perm_id = target_order
            .venue_order_id()
            .and_then(|id| IbOrderSelector::from_venue_order_id(&id).ok())
            .and_then(|selector| match selector {
                IbOrderSelector::PermId(id) => Some(id),
                _ => None,
            })
            .unwrap_or(0);

        if let Some(existing) = state.venue_order_id_map.get(&ib_order_id).copied() {
            if existing != client_order_id {
                let parent = state.group_for_client(client_order_id);
                anyhow::ensure!(
                    parent.is_some() && parent == state.group_for_client(existing),
                    "IB order ID {ib_order_id} is already mapped to client order {existing}"
                );
                state
                    .auxiliary_orders
                    .entry(client_order_id)
                    .or_insert_with(|| TrackedOrder {
                        client_order_id: target_order.client_order_id(),
                        trader_id: target_order.trader_id(),
                        strategy_id: target_order.strategy_id(),
                        instrument_id: target_order.instrument_id(),
                        order_side: target_order.order_side(),
                        order_type: target_order.order_type(),
                        accepted: true,
                        avg_px: None,
                        pending_cancel: false,
                        pending_modify: None,
                        perm_id,
                        spread_fill_ids: ahash::AHashSet::new(),
                        last_update: None,
                    });
                return Ok(());
            }
        }
        state.order_id_map.insert(client_order_id, ib_order_id);
        state.terminal_orders.remove(&ib_order_id);
        state
            .active_orders
            .entry(ib_order_id)
            .or_insert_with(|| TrackedOrder {
                client_order_id: target_order.client_order_id(),
                trader_id: target_order.trader_id(),
                strategy_id: target_order.strategy_id(),
                instrument_id: target_order.instrument_id(),
                order_side: target_order.order_side(),
                order_type: target_order.order_type(),
                accepted: true,
                avg_px: None,
                pending_cancel: false,
                pending_modify: None,
                perm_id,
                spread_fill_ids: ahash::AHashSet::new(),
                last_update: Some((
                    target_order.quantity(),
                    target_order.price(),
                    target_order.trigger_price(),
                )),
            });
        state
            .venue_order_id_map
            .insert(ib_order_id, client_order_id);

        Ok(())
    }

    pub(super) fn get_tracked_order_context(
        ib_order_id: i32,
        orders: &OrderTracker,
    ) -> anyhow::Result<Option<TrackedOrder>> {
        let state = orders.lock()?;
        if let Some(context) = state.active_orders.get(&ib_order_id).cloned() {
            return Ok(Some(context));
        }

        Ok(state.terminal_orders.get(&ib_order_id).cloned())
    }

    pub(super) fn emit_order_accepted_if_needed(
        ib_order_id: i32,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        ts_event: UnixNanos,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<bool> {
        let mut state = orders.lock()?;
        let Some(context) = state.active_orders.get_mut(&ib_order_id) else {
            return Ok(false);
        };

        Self::emit_order_accepted(context, venue_order_id, account_id, ts_event, exec_sender)
    }

    pub(super) fn emit_order_accepted_for_fill_if_needed(
        ib_order_id: i32,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        ts_event: UnixNanos,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<bool> {
        let mut state = orders.lock()?;
        let context = if let Some(context) = state.active_orders.get_mut(&ib_order_id) {
            context
        } else if let Some(context) = state.terminal_orders.get_mut(&ib_order_id) {
            context
        } else {
            return Ok(false);
        };

        Self::emit_order_accepted(context, venue_order_id, account_id, ts_event, exec_sender)
    }

    fn emit_order_accepted(
        context: &mut TrackedOrder,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        ts_event: UnixNanos,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<bool> {
        if context.accepted {
            return Ok(false);
        }

        let event = OrderAccepted::new(
            context.trader_id,
            context.strategy_id,
            context.instrument_id,
            context.client_order_id,
            venue_order_id,
            account_id,
            UUID4::new(),
            ts_event,
            ts_event,
            false,
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Accepted(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order accepted event: {e}"))?;
        context.accepted = true;

        Ok(true)
    }

    pub(super) fn remove_order_tracking(
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        orders: &OrderTracker,
    ) -> anyhow::Result<()> {
        let mut state = orders.lock()?;
        state.order_id_map.remove(&client_order_id);
        state.venue_order_id_map.remove(&ib_order_id);
        state.active_orders.remove(&ib_order_id);
        state.terminal_orders.remove(&ib_order_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ibapi::contracts::{Contract, Exchange};
    use nautilus_core::Params;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    fn contract_with_exchange(exchange: &str) -> Contract {
        Contract {
            exchange: Exchange::from(exchange),
            ..Default::default()
        }
    }

    #[rstest]
    fn test_contract_with_order_exchange_param_overrides_exchange() {
        let contract = contract_with_exchange("SMART");
        let mut params = Params::new();
        params.insert("exchange".to_string(), Value::String("IEX".to_string()));

        let updated = InteractiveBrokersExecutionClient::contract_with_order_exchange_param(
            contract.clone(),
            Some(&params),
        )
        .unwrap();

        assert_eq!(updated.exchange.as_str(), "IEX");
        assert_eq!(contract.exchange.as_str(), "SMART");
    }

    #[rstest]
    fn test_contract_with_order_exchange_param_keeps_contract_without_exchange() {
        let contract = contract_with_exchange("SMART");
        let params = Params::new();

        let updated = InteractiveBrokersExecutionClient::contract_with_order_exchange_param(
            contract,
            Some(&params),
        )
        .unwrap();

        assert_eq!(updated.exchange.as_str(), "SMART");
    }

    #[rstest]
    fn test_contract_with_order_exchange_param_keeps_contract_with_empty_exchange() {
        let contract = contract_with_exchange("SMART");
        let mut params = Params::new();
        params.insert("exchange".to_string(), Value::String(String::new()));

        let updated = InteractiveBrokersExecutionClient::contract_with_order_exchange_param(
            contract,
            Some(&params),
        )
        .unwrap();

        assert_eq!(updated.exchange.as_str(), "SMART");
    }

    #[rstest]
    fn test_contract_with_order_exchange_param_rejects_non_string_exchange() {
        let contract = contract_with_exchange("SMART");
        let mut params = Params::new();
        params.insert("exchange".to_string(), Value::Bool(true));

        let err = InteractiveBrokersExecutionClient::contract_with_order_exchange_param(
            contract,
            Some(&params),
        )
        .unwrap_err();

        assert!(err.to_string().contains("must be a string"));
    }
}
