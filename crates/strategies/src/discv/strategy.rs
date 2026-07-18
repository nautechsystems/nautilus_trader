use std::fmt::Debug;

use ahash::AHashSet;
use nautilus_common::actor::DataActor;
use nautilus_hyperliquid::common::consts::HYPERLIQUID_VENUE;
use nautilus_model::data::{Participant, ParticipantProfile};
use nautilus_trading::{nautilus_strategy, strategy::StrategyCore};
use ustr::Ustr;

use super::config::AddrDiscoveryConfig;

/// Subscribes to trades for configured instruments and collects unique
/// wallet addresses observed in real time.
///
pub struct AddrDiscovery {
    core: StrategyCore,
    max_addresses: usize,
    /// Unique wallet addresses observed.
    observed_addrs: AHashSet<Ustr>,
}

impl AddrDiscovery {
    #[must_use]
    pub fn from_config(config: AddrDiscoveryConfig) -> Self {
        Self {
            core: StrategyCore::new(config.base),
            max_addresses: config.max_addresses,
            observed_addrs: AHashSet::new(),
        }
    }

    #[must_use]
    pub fn unique_count(&self) -> usize {
        self.observed_addrs.len()
    }
}

nautilus_strategy!(AddrDiscovery);

impl Debug for AddrDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddrDiscovery")
            .field("unique_addresses", &self.observed_addrs.len())
            .finish()
    }
}

impl DataActor for AddrDiscovery {
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Subscribing to participant profiles for {}",
            *HYPERLIQUID_VENUE
        );
        self.subscribe_participant_profiles();

        log::info!("Subscribing to participant discovery for all Hyperliquid instruments");
        self.subscribe_all_participants(*HYPERLIQUID_VENUE);

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Stopping address discovery: {} unique participants",
            self.observed_addrs.len(),
        );

        self.unsubscribe_participants(*HYPERLIQUID_VENUE);
        self.unsubscribe_participant_profiles();
        Ok(())
    }

    fn on_participants(&mut self, participants: &[Participant]) -> anyhow::Result<()> {
        for participant in participants {
            let id = participant.id.inner();
            if self.observed_addrs.insert(id) {
                log::info!(
                    "New: id={id} venue={} kind={} first_seen={} last_seen={}",
                    participant.venue,
                    participant.kind,
                    participant.first_seen_at,
                    participant.last_seen_at,
                );
            }
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

    fn on_participant_profiles(&mut self, profiles: &[ParticipantProfile]) -> anyhow::Result<()> {
        for profile in profiles {
            log::info!(
                "Participant: id={} b={} m={} p={} o={} t={} ts={}",
                profile.participant_id,
                profile.balances.as_ref().map_or(0, Vec::len),
                profile.margins.as_ref().map_or(0, Vec::len),
                profile.positions.as_ref().map_or(0, Vec::len),
                profile.open_orders.as_ref().map_or(0, Vec::len),
                profile.transactions.as_ref().map_or(0, Vec::len),
                profile.ts_init,
            );
        }

        Ok(())
    }
}
