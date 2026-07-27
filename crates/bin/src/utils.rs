use nautilus_model::types::{Price, Quantity};

pub fn micro_price(
    ask_price: Price,
    bid_price: Price,
    ask_size: Quantity,
    bid_size: Quantity,
    precision: u8,
) -> Price {
    Price::new(
        ((bid_size.as_decimal() * ask_price.as_decimal()
            + ask_size.as_decimal() * bid_price.as_decimal())
            / (bid_size + ask_size).as_decimal())
        .as_f64(),
        precision,
    )
}