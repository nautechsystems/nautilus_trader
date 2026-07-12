use nautilus_core::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::InstrumentId,
    instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Price, Quantity},
};
use ustr::Ustr;

use super::{config::AddrDiscoveryConfig, strategy::AddrDiscovery};

fn dummy_trade(
    instrument_id: InstrumentId,
    trade_id: &str,
    buyer: &str,
    seller: &str,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from("100.00"),
        Quantity::from("1.0"),
        AggressorSide::Buyer,
        trade_id.into(),
        UnixNanos::from(1_000_000_000),
        UnixNanos::from(1_000_000_000),
    )
    .with_participants(Ustr::from(buyer), Ustr::from(seller))
}

#[test]
fn collects_unique_addresses() {
    let instrument_id = InstrumentId::from("ETH-PERP.HYPERLIQUID");
    let config = AddrDiscoveryConfig::builder()
        .instrument_ids(vec![instrument_id])
        .max_addresses(1000)
        .build();
    let mut strategy = AddrDiscovery::from_config(config);

    let trade1 = dummy_trade(instrument_id, "1001", "0xbuyer1", "0xseller1");
    let trade2 = dummy_trade(instrument_id, "1002", "0xbuyer2", "0xseller1");
    let trade_dup = dummy_trade(instrument_id, "1003", "0xbuyer1", "0xseller1");

    use nautilus_common::actor::DataActor;
    strategy.on_trade(&trade1).unwrap();
    strategy.on_trade(&trade2).unwrap();
    strategy.on_trade(&trade_dup).unwrap();

    assert_eq!(strategy.unique_count(), 3);
    assert_eq!(strategy.trade_count(), 3);
}

#[test]
fn stores_received_instruments() {
    let config = AddrDiscoveryConfig::builder()
        .instrument_ids(Vec::new())
        .build();
    let mut strategy = AddrDiscovery::from_config(config);
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

    use nautilus_common::actor::DataActor;
    strategy.on_instrument(&instrument).unwrap();

    assert_eq!(strategy.instrument_count(), 1);
}
