use nautilus_model::identifiers::{InstrumentId, StrategyId};
use nautilus_trading::StrategyConfig;

#[derive(Debug, Clone, bon::Builder)]
pub struct RecorderConfig {
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("RECORDER-001")),
        order_id_tag: Some("002".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    pub instrument_id: Vec<InstrumentId>,
    pub catalog_path: String,
    pub book_depth: usize,
    pub interval_parquet_dump_seconds: u64,
}
