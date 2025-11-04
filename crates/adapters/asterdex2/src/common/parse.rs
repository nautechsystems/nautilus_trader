use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};

use crate::common::{
    enums::{
        AsterdexOrderSide, AsterdexOrderStatus, AsterdexOrderType, AsterdexTimeInForce,
    },
    models::AsterdexSymbol,
    ASTERDEX,
};

// Parse Asterdex spot symbol to Nautilus CurrencyPair
pub fn parse_spot_instrument(
    symbol: &AsterdexSymbol,
) -> anyhow::Result<InstrumentAny> {
    // Create venue
    let venue = Venue::from(ASTERDEX);

    // Create symbol
    let symbol_str = Symbol::from(symbol.symbol.as_str());

    // Create instrument ID
    let instrument_id = InstrumentId::new(symbol_str, venue);

    // Create base and quote currencies
    let base_currency = Currency::from(symbol.base_asset.as_str());
    let quote_currency = Currency::from(symbol.quote_asset.as_str());

    // Default precision values (should be extracted from symbol data in production)
    let price_precision: u8 = 8;
    let size_precision: u8 = 8;

    // Calculate increments (10^-precision)
    let price_increment = Price::from(format!("0.{:0>width$}1", "", width = (price_precision - 1) as usize).as_str());
    let size_increment = Quantity::from(format!("0.{:0>width$}1", "", width = (size_precision - 1) as usize).as_str());

    // Minimum quantity (should be extracted from filters in production)
    let min_quantity = Some(Quantity::from("0.00001"));

    // Create CurrencyPair instrument
    let instrument = CurrencyPair::new(
        instrument_id,
        symbol_str,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,        // lot_size
        None,        // max_quantity
        None,        // min_notional
        min_quantity,
        None,        // max_notional
        None,        // max_price
        None,        // min_price
        None,        // margin_init
        None,        // margin_maint
        None,        // maker_fee
        None,        // taker_fee
        None,        // info
        0.into(),    // ts_event
        0.into(),    // ts_init
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

// Parse Asterdex futures symbol to Nautilus CryptoPerpetual
pub fn parse_futures_instrument(
    symbol: &AsterdexSymbol,
) -> anyhow::Result<InstrumentAny> {
    // Create venue
    let venue = Venue::from(ASTERDEX);

    // Create symbol
    let symbol_str = Symbol::from(symbol.symbol.as_str());

    // Create instrument ID
    let instrument_id = InstrumentId::new(symbol_str, venue);

    // Create base, quote, and settlement currencies
    let base_currency = Currency::from(symbol.base_asset.as_str());
    let quote_currency = Currency::from(symbol.quote_asset.as_str());
    let settlement_currency = quote_currency; // Typically USDT for perpetuals

    // Default precision values
    let price_precision: u8 = 8;
    let size_precision: u8 = 8;

    // Calculate increments
    let price_increment = Price::from(format!("0.{:0>width$}1", "", width = (price_precision - 1) as usize).as_str());
    let size_increment = Quantity::from(format!("0.{:0>width$}1", "", width = (size_precision - 1) as usize).as_str());

    // Minimum quantity
    let min_quantity = Some(Quantity::from("0.00001"));

    // Create CryptoPerpetual instrument
    let instrument = CryptoPerpetual::new(
        instrument_id,
        symbol_str,
        base_currency,
        quote_currency,
        settlement_currency,
        false,        // is_inverse
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,        // multiplier
        None,        // lot_size
        None,        // max_quantity
        min_quantity,
        None,        // max_notional
        None,        // min_notional
        None,        // max_price
        None,        // min_price
        None,        // margin_init
        None,        // margin_maint
        None,        // maker_fee
        None,        // taker_fee
        0.into(),    // ts_event
        0.into(),    // ts_init
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

// Parse Asterdex order side to Nautilus OrderSide
pub fn parse_order_side(side: &AsterdexOrderSide) -> OrderSide {
    match side {
        AsterdexOrderSide::Buy => OrderSide::Buy,
        AsterdexOrderSide::Sell => OrderSide::Sell,
    }
}

// Parse Asterdex order type to Nautilus OrderType
pub fn parse_order_type(order_type: &AsterdexOrderType) -> OrderType {
    match order_type {
        AsterdexOrderType::Limit => OrderType::Limit,
        AsterdexOrderType::Market => OrderType::Market,
        AsterdexOrderType::Stop | AsterdexOrderType::StopMarket => OrderType::StopMarket,
        AsterdexOrderType::TakeProfit | AsterdexOrderType::TakeProfitMarket => {
            OrderType::Limit // TakeProfit as Limit with trigger
        }
        AsterdexOrderType::TrailingStopMarket => OrderType::TrailingStopMarket,
    }
}

// Parse Asterdex order status to Nautilus OrderStatus
pub fn parse_order_status(status: &AsterdexOrderStatus) -> OrderStatus {
    match status {
        AsterdexOrderStatus::New => OrderStatus::Accepted,
        AsterdexOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        AsterdexOrderStatus::Filled => OrderStatus::Filled,
        AsterdexOrderStatus::Canceled => OrderStatus::Canceled,
        AsterdexOrderStatus::Rejected => OrderStatus::Rejected,
        AsterdexOrderStatus::Expired => OrderStatus::Expired,
    }
}

// Parse Asterdex time in force to Nautilus TimeInForce
pub fn parse_time_in_force(tif: &AsterdexTimeInForce) -> TimeInForce {
    match tif {
        AsterdexTimeInForce::Gtc => TimeInForce::Gtc,
        AsterdexTimeInForce::Ioc => TimeInForce::Ioc,
        AsterdexTimeInForce::Fok => TimeInForce::Fok,
        AsterdexTimeInForce::Gtx => TimeInForce::Gtd, // Post-only mapped to GTD
        AsterdexTimeInForce::Hidden => TimeInForce::Gtc, // Hidden as GTC
    }
}

// Parse Asterdex OrderSide string to enum
pub fn parse_order_side_str(side: &str) -> anyhow::Result<AsterdexOrderSide> {
    match side.to_uppercase().as_str() {
        "BUY" => Ok(AsterdexOrderSide::Buy),
        "SELL" => Ok(AsterdexOrderSide::Sell),
        _ => Err(anyhow::anyhow!("Invalid order side: {}", side)),
    }
}

// Parse Asterdex OrderType string to enum
pub fn parse_order_type_str(order_type: &str) -> anyhow::Result<AsterdexOrderType> {
    match order_type.to_uppercase().as_str() {
        "LIMIT" => Ok(AsterdexOrderType::Limit),
        "MARKET" => Ok(AsterdexOrderType::Market),
        "STOP" => Ok(AsterdexOrderType::Stop),
        "STOP_MARKET" => Ok(AsterdexOrderType::StopMarket),
        "TAKE_PROFIT" => Ok(AsterdexOrderType::TakeProfit),
        "TAKE_PROFIT_MARKET" => Ok(AsterdexOrderType::TakeProfitMarket),
        "TRAILING_STOP_MARKET" => Ok(AsterdexOrderType::TrailingStopMarket),
        _ => Err(anyhow::anyhow!("Invalid order type: {}", order_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side(&AsterdexOrderSide::Buy), OrderSide::Buy);
        assert_eq!(parse_order_side(&AsterdexOrderSide::Sell), OrderSide::Sell);
    }

    #[test]
    fn test_parse_order_type() {
        assert_eq!(
            parse_order_type(&AsterdexOrderType::Limit),
            OrderType::Limit
        );
        assert_eq!(
            parse_order_type(&AsterdexOrderType::Market),
            OrderType::Market
        );
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(
            parse_order_status(&AsterdexOrderStatus::New),
            OrderStatus::Accepted
        );
        assert_eq!(
            parse_order_status(&AsterdexOrderStatus::Filled),
            OrderStatus::Filled
        );
    }

    #[test]
    fn test_parse_time_in_force() {
        assert_eq!(
            parse_time_in_force(&AsterdexTimeInForce::Gtc),
            TimeInForce::Gtc
        );
        assert_eq!(
            parse_time_in_force(&AsterdexTimeInForce::Ioc),
            TimeInForce::Ioc
        );
    }

    #[test]
    fn test_parse_order_side_str() {
        assert!(matches!(
            parse_order_side_str("BUY"),
            Ok(AsterdexOrderSide::Buy)
        ));
        assert!(matches!(
            parse_order_side_str("SELL"),
            Ok(AsterdexOrderSide::Sell)
        ));
        assert!(parse_order_side_str("INVALID").is_err());
    }

    #[test]
    fn test_parse_order_type_str() {
        assert!(matches!(
            parse_order_type_str("LIMIT"),
            Ok(AsterdexOrderType::Limit)
        ));
        assert!(matches!(
            parse_order_type_str("MARKET"),
            Ok(AsterdexOrderType::Market)
        ));
        assert!(parse_order_type_str("INVALID").is_err());
    }
}
