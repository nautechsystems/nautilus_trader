use crate::types::quantity::{Quantity, QuantityRaw};

// TODO: Document panic
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn quantity_new(value: f64, precision: u8) -> Quantity {
    Quantity::new(value, precision)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn quantity_from_raw(raw: QuantityRaw, precision: u8) -> Quantity {
    Quantity::from_raw(raw, precision)
}

#[unsafe(no_mangle)]
pub extern "C" fn quantity_as_f64(qty: &Quantity) -> f64 {
    qty.as_f64()
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn quantity_saturating_sub(a: Quantity, b: Quantity) -> Quantity {
    a.saturating_sub(b)
}
