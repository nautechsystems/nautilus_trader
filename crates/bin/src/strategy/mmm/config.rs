use nautilus_model::identifiers::{InstrumentId, StrategyId};
use nautilus_trading::StrategyConfig;


#[derive(Debug, Clone, bon::Builder)]
pub struct MattiasMarketMakerConfig {
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("MMM-001")),
        order_id_tag: Some("003".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    pub instrument_id: InstrumentId,
    pub path: String
}