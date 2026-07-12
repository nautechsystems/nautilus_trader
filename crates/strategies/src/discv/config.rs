use nautilus_model::identifiers::{InstrumentId, StrategyId};
use nautilus_trading::strategy::StrategyConfig;

/// Configuration for the address discovery strategy.
#[derive(Debug, Clone, bon::Builder)]
pub struct AddrDiscoveryConfig {
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("ADDR_DISCV-001")),
        order_id_tag: Some("001".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    /// Instruments to subscribe to for trade monitoring.
    pub instrument_ids: Vec<InstrumentId>,
    /// Maximum number of unique addresses to collect before logging a summary.
    #[builder(default = 1000)]
    pub max_addresses: usize,
}
