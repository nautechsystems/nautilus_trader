use strum::Display;

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleBodySize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
    Trend = 4,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleDirection {
    Bull = 1,
    None = 0,
    Bear = -1,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleSize {
    None = 0,
    VerySmall = 1,
    Small = 2,
    Medium = 3,
    Large = 4,
    VeryLarge = 5,
    ExtremelyLarge = 6,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleWickSize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
}
