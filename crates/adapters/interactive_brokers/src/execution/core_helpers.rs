// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use super::*;

impl InteractiveBrokersExecutionClient {
    pub(super) fn cached_spread_instrument_ids_for_preload(
        cache: &Cache,
        instrument_provider: &InteractiveBrokersInstrumentProvider,
    ) -> Vec<InstrumentId> {
        let mut spread_ids = ahash::AHashSet::new();

        for client_order_id in cache.iter_client_order_ids(None, None, None, None) {
            if let Some(order) = cache.order(&client_order_id) {
                let instrument_id = order.instrument_id();
                if is_spread_instrument_id(&instrument_id)
                    && instrument_provider.find(&instrument_id).is_none()
                {
                    spread_ids.insert(instrument_id);
                }
            }
        }

        let mut spread_ids: Vec<InstrumentId> = spread_ids.into_iter().collect();
        spread_ids.sort_by_key(|a| a.to_string());
        spread_ids
    }

    pub(super) async fn preload_cached_spread_instruments(
        &self,
        client: &Client,
    ) -> anyhow::Result<()> {
        let spread_ids = {
            let cache = self.core.cache();
            Self::cached_spread_instrument_ids_for_preload(&cache, &self.instrument_provider)
        };

        if spread_ids.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "Preloading {} cached Interactive Brokers spread instrument(s) before reconciliation",
            spread_ids.len()
        );

        for instrument_id in spread_ids {
            match self
                .instrument_provider
                .fetch_spread_instrument(client, instrument_id, false, None)
                .await
            {
                Ok(true) => {
                    tracing::debug!("Preloaded cached spread instrument {}", instrument_id);
                }
                Ok(false) => {
                    tracing::warn!(
                        "Failed to preload cached spread instrument {}",
                        instrument_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to preload cached spread instrument {}: {}",
                        instrument_id,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    pub(super) fn get_mapped_instrument_id(
        order_id: i32,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
    ) -> anyhow::Result<Option<InstrumentId>> {
        let map = instrument_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?;
        Ok(map.get(&order_id).copied())
    }

    pub(super) fn get_required_order_actor_ids(
        order_id: i32,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
    ) -> anyhow::Result<(TraderId, StrategyId)> {
        let trader_id = {
            let map = trader_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock trader ID map"))?;
            map.get(&order_id).copied()
        }
        .with_context(|| format!("Trader ID not found for Interactive Brokers order {order_id}"))?;

        let strategy_id = {
            let map = strategy_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock strategy ID map"))?;
            map.get(&order_id).copied()
        }
        .with_context(|| {
            format!("Strategy ID not found for Interactive Brokers order {order_id}")
        })?;

        Ok((trader_id, strategy_id))
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn cache_order_tracking(
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
    ) -> anyhow::Result<()> {
        {
            let mut order_map = order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
            order_map.insert(client_order_id, ib_order_id);
        }

        {
            let mut venue_map = venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
            venue_map.insert(ib_order_id, client_order_id);
        }

        {
            let mut instrument_map = instrument_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?;
            instrument_map.insert(ib_order_id, instrument_id);
        }

        {
            let mut trader_map = trader_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock trader_id map"))?;
            trader_map.insert(ib_order_id, trader_id);
        }

        {
            let mut strategy_map = strategy_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock strategy_id map"))?;
            strategy_map.insert(ib_order_id, strategy_id);
        }

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
