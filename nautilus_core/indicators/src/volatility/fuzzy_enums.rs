use strum::Display;

#[derive(Debug, Display, Clone, PartialEq)]
pub enum CandleBodySize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
    Trend = 4,
}

#[derive(Debug, Display, Clone, PartialEq)]
pub enum CandleDirection {
    Bull = 1,
    None = 0,
    Bear = -1,
}

#[derive(Debug, Display, Clone, PartialEq)]
pub enum CandleSize {
    None = 0,
    VerySmall = 1,
    Small = 2,
    Medium = 3,
    Large = 4,
    VeryLarge = 5,
    ExtremelyLarge = 6,
}

#[derive(Debug, Display, Clone, PartialEq)]
pub enum CandleWickSize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
}
