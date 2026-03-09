use crate::types::price::{Price, PriceRaw};

// TODO: Document panic
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    Price::new(value, precision)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn price_from_raw(raw: PriceRaw, precision: u8) -> Price {
    Price::from_raw(raw, precision)
}

#[unsafe(no_mangle)]
pub extern "C" fn price_as_f64(price: &Price) -> f64 {
    price.as_f64()
}
