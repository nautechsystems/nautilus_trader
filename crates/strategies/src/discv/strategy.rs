use std::fmt::Debug;

use ahash::{AHashMap, AHashSet};
use nautilus_common::actor::DataActor;
use nautilus_model::{
    data::TradeTick,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_trading::{nautilus_strategy, strategy::StrategyCore};
use ustr::Ustr;

use super::config::AddrDiscoveryConfig;

/// Subscribes to trades for configured instruments and collects unique
/// wallet addresses observed in real time.
///
/// The `TradeTick` type carries optional `buyer` and `seller` fields.
/// On Hyperliquid these are populated with wallet addresses from the
/// WebSocket `users` array. On venues that don't provide participant
/// info these fields are `None`.
pub struct AddrDiscovery {
    core: StrategyCore,
    instrument_ids: Vec<InstrumentId>,
    max_addresses: usize,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    /// Unique wallet addresses observed.
    observed_addrs: AHashSet<Ustr>,
    trade_count: u64,
}

impl AddrDiscovery {
    #[must_use]
    pub fn from_config(config: AddrDiscoveryConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base),
            instrument_ids: config.instrument_ids,
            max_addresses: config.max_addresses,
            instruments: AHashMap::new(),
            observed_addrs: AHashSet::new(),
            trade_count: 0,
        }
    }

    #[must_use]
    pub fn unique_count(&self) -> usize {
        self.observed_addrs.len()
    }

    #[must_use]
    pub fn trade_count(&self) -> u64 {
        self.trade_count
    }

    #[must_use]
    pub fn instrument_count(&self) -> usize {
        self.instruments.len()
    }
}

nautilus_strategy!(AddrDiscovery);

impl Debug for AddrDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddrDiscovery")
            .field("instrument_ids", &self.instrument_ids)
            .field("instruments", &self.instruments.len())
            .field("unique_addresses", &self.observed_addrs.len())
            .field("trade_count", &self.trade_count)
            .finish()
    }
}

impl DataActor for AddrDiscovery {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let venues: AHashSet<_> = self
            .instrument_ids
            .iter()
            .map(|instrument_id| instrument_id.venue)
            .collect();
        for venue in venues {
            log::info!("Requesting all instruments for {venue}");
            self.request_instruments(Some(venue), None, None, None, None)?;
        }

        let ids = self.instrument_ids.clone();
        for instrument_id in &ids {
            log::info!("Subscribing to trades for {instrument_id}");
            self.subscribe_trades(*instrument_id, None, None);
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Stopping address discovery: {} instruments received, {} trades observed, {} unique addresses collected",
            self.instruments.len(),
            self.trade_count,
            self.observed_addrs.len(),
        );
        let ids = self.instrument_ids.clone();
        for instrument_id in &ids {
            self.unsubscribe_trades(*instrument_id, None, None);
        }
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let instrument_id = instrument.id();
        self.instruments.insert(instrument_id, instrument.clone());
        log::info!(
            "Received instrument {} with raw symbol {}",
            instrument_id,
            instrument.raw_symbol(),
        );
        Ok(())
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        self.trade_count += 1;

        if let Some(buyer) = tick.buyer
            && let Some(seller) = tick.seller
        {
            if self.observed_addrs.insert(buyer) {
                log::info!("New address: {buyer}");
            }

            if self.observed_addrs.insert(seller) {
                log::info!("New address: {seller}");
            }
        }

        if tick.buyer.is_none() || tick.seller.is_none() {
            log::warn!("Trade tick has no buyer or seller address; venue may not provide");
        }

        if self.observed_addrs.len() >= self.max_addresses {
            log::info!(
                "Reached maximum address count {}, clearing collected addresses",
                self.max_addresses,
            );
            self.observed_addrs.clear();
        }

        Ok(())
    }
}
