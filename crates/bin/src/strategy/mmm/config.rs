use nautilus_model::{identifiers::{InstrumentId, StrategyId}, types::Quantity};
use nautilus_trading::StrategyConfig;


#[allow(non_snake_case)]
#[derive(Debug, Clone, bon::Builder)]
pub struct MattiasMarketMakerConfig {
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("MMM-001")),
        order_id_tag: Some("003".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    pub instrument_id: InstrumentId,
    pub catalog_path: String,

    pub Φ_n: u8,
    pub Φ_0: Quantity,
    pub Q_max: Quantity
}