use nautilus_model::identifiers::StrategyId;
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
    /// Maximum number of unique addresses to collect before logging a summary.
    #[builder(default = 10000000)]
    pub max_addresses: usize,
}
