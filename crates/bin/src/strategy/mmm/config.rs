use nautilus_model::{
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};
use nautilus_trading::StrategyConfig;
use rust_decimal::Decimal;

use crate::config::MattiasMarketMakerTomlConfig;

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
    pub Q_max: Quantity,
    pub Δ_0: Decimal,
    pub Δ_μ: Decimal,
    pub β: Decimal,
}

impl TryFrom<&MattiasMarketMakerTomlConfig> for MattiasMarketMakerConfig {
    type Error = anyhow::Error;

    fn try_from(cfg: &MattiasMarketMakerTomlConfig) -> Result<Self, Self::Error> {
        Ok(Self::builder()
            .instrument_id(InstrumentId::from(cfg.instrument_id.as_str()))
            .catalog_path(cfg.path.clone())
            .Φ_n(cfg.Φ_n)
            .Φ_0(cfg.Φ_0)
            .Q_max(cfg.Q_max)
            .Δ_0(cfg.Δ_0)
            .Δ_μ(cfg.Δ_μ)
            .β(cfg.β)
            .build())
    }
}
