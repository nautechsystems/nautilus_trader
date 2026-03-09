use crate::types::{Currency, Money, money::MoneyRaw};

// TODO: Document panic
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn money_new(amount: f64, currency: Currency) -> Money {
    Money::new(amount, currency)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn money_from_raw(raw: MoneyRaw, currency: Currency) -> Money {
    Money::from_raw(raw, currency)
}

#[unsafe(no_mangle)]
pub extern "C" fn money_as_f64(money: &Money) -> f64 {
    money.as_f64()
}
