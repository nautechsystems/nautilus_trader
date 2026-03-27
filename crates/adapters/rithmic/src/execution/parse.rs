//! Parsing utilities for execution messages.

use rithmic_rs::{OrderSide, OrderStatus, OrderType, TimeInForce};

use crate::error::{Result, RithmicError};

use super::client::OrderRequest;

#[allow(dead_code)]
pub fn parse_order_status(status_code: i32) -> Result<OrderStatus> {
    match status_code {
        0 => Ok(OrderStatus::Pending),
        1 => Ok(OrderStatus::Open),
        2 => Ok(OrderStatus::Partial),
        3 => Ok(OrderStatus::Complete),
        4 => Ok(OrderStatus::Cancelled),
        5 => Ok(OrderStatus::Rejected),
        6 => Ok(OrderStatus::Expired),
        _ => Err(RithmicError::Parse(format!(
            "Unknown order status code: {status_code}"
        ))),
    }
}

#[allow(dead_code)]
pub fn parse_order_side(side_code: i32) -> Result<OrderSide> {
    match side_code {
        1 => Ok(OrderSide::Buy),
        2 => Ok(OrderSide::Sell),
        _ => Err(RithmicError::Parse(format!(
            "Unknown order side code: {side_code}"
        ))),
    }
}

#[allow(dead_code)]
pub fn parse_order_type(type_code: i32) -> Result<OrderType> {
    match type_code {
        1 => Ok(OrderType::Market),
        2 => Ok(OrderType::Limit),
        3 => Ok(OrderType::StopMarket),
        4 => Ok(OrderType::StopLimit),
        _ => Err(RithmicError::Parse(format!(
            "Unknown order type code: {type_code}"
        ))),
    }
}

#[allow(dead_code)]
pub fn parse_time_in_force(tif_code: i32) -> Result<TimeInForce> {
    match tif_code {
        1 => Ok(TimeInForce::Day),
        2 => Ok(TimeInForce::Gtc),
        3 => Ok(TimeInForce::Ioc),
        4 => Ok(TimeInForce::Fok),
        _ => Err(RithmicError::Parse(format!("Unknown TIF code: {tif_code}"))),
    }
}

#[allow(dead_code)]
pub fn build_order_request(
    client_order_id: &str,
    symbol: &str,
    exchange: &str,
    side: &str,
    order_type: &str,
    time_in_force: &str,
    quantity: f64,
    price: Option<f64>,
    stop_price: Option<f64>,
) -> Result<OrderRequest> {
    let side: OrderSide = side
        .parse()
        .map_err(|e| RithmicError::Parse(format!("{e}")))?;
    let order_type: OrderType = order_type
        .parse()
        .map_err(|e| RithmicError::Parse(format!("{e}")))?;
    let tif: TimeInForce = time_in_force
        .parse()
        .map_err(|e| RithmicError::Parse(format!("{e}")))?;

    // Validate price for limit orders
    if (order_type == OrderType::Limit || order_type == OrderType::StopLimit) && price.is_none() {
        return Err(RithmicError::Order(
            "Limit orders require a price".to_string(),
        ));
    }

    // Validate stop price for stop orders
    if (order_type == OrderType::StopMarket || order_type == OrderType::StopLimit)
        && stop_price.is_none()
    {
        return Err(RithmicError::Order(
            "Stop orders require a stop price".to_string(),
        ));
    }

    if quantity <= 0.0 {
        return Err(RithmicError::Order(format!("Invalid quantity: {quantity}")));
    }

    Ok(OrderRequest {
        client_order_id: client_order_id.to_string(),
        symbol: symbol.to_string(),
        exchange: exchange.to_string(),
        side,
        order_type,
        time_in_force: tif,
        quantity,
        price,
        stop_price,
        trailing_stop: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_order_request_market() {
        let request = build_order_request(
            "ORDER123", "ESZ4", "CME", "BUY", "MARKET", "DAY", 5.0, None, None,
        )
        .unwrap();

        assert_eq!(request.client_order_id, "ORDER123");
        assert_eq!(request.symbol, "ESZ4");
        assert_eq!(request.side, OrderSide::Buy);
        assert_eq!(request.order_type, OrderType::Market);
    }

    #[test]
    fn test_build_order_request_limit() {
        let request = build_order_request(
            "ORDER123",
            "ESZ4",
            "CME",
            "SELL",
            "LIMIT",
            "GTC",
            10.0,
            Some(4500.00),
            None,
        )
        .unwrap();

        assert_eq!(request.side, OrderSide::Sell);
        assert_eq!(request.order_type, OrderType::Limit);
        assert_eq!(request.price, Some(4500.00));
    }

    #[test]
    fn test_build_order_request_limit_missing_price() {
        let result = build_order_request(
            "ORDER123", "ESZ4", "CME", "BUY", "LIMIT", "DAY", 5.0, None, // Missing price
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_order_status() {
        assert_eq!(parse_order_status(0).unwrap(), OrderStatus::Pending);
        assert_eq!(parse_order_status(3).unwrap(), OrderStatus::Complete);
        assert!(parse_order_status(99).is_err());
    }

    #[test]
    fn test_build_order_request_zero_quantity() {
        let result = build_order_request(
            "ORDER123", "ESZ4", "CME", "BUY", "MARKET", "DAY", 0.0, None, None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid quantity"));
    }

    #[test]
    fn test_build_order_request_negative_quantity() {
        let result = build_order_request(
            "ORDER123", "ESZ4", "CME", "BUY", "MARKET", "DAY", -5.0, None, None,
        );
        assert!(result.is_err());
    }
}
