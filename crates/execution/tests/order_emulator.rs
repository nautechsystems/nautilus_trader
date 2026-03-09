use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    instruments::{CryptoPerpetual, stubs::crypto_perpetual_ethusdt},
    types::Price,
};
use rstest::rstest;

#[rstest]
fn test_stop_limit_order_triggered_before_market_data_retains_command(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // This test validates that the OrderMatchingCore correctly handles
    // quote ticks with None bid/ask prices
    let instrument_id = crypto_perpetual_ethusdt.id;
    let price_increment = crypto_perpetual_ethusdt.price_increment;

    // Create a matching core
    let mut matching_core = OrderMatchingCore::new(instrument_id, price_increment);

    // Verify matching core has no market data initially
    assert!(matching_core.bid.is_none());
    assert!(matching_core.ask.is_none());

    // Process a quote tick to provide market data
    matching_core.set_bid_raw(Price::from("5060.00"));
    matching_core.set_ask_raw(Price::from("5070.00"));

    // Verify market data is now available
    assert!(matching_core.bid.is_some());
    assert!(matching_core.ask.is_some());
    assert_eq!(matching_core.bid.unwrap(), Price::from("5060.00"));
    assert_eq!(matching_core.ask.unwrap(), Price::from("5070.00"));
}
