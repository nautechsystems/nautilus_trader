#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum OrderSide {
    Buy = 0x0_u8,
    Sell = 0x1_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for OrderSide {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Buy,
            0x1_u8 => Self::Sell,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<OrderSide> for u8 {
    #[inline]
    fn from(v: OrderSide) -> Self {
        match v {
            OrderSide::Buy => 0x0_u8,
            OrderSide::Sell => 0x1_u8,
            OrderSide::NonRepresentable => 0xfe_u8,
            OrderSide::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for OrderSide {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Buy" => Ok(Self::Buy),
            "Sell" => Ok(Self::Sell),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for OrderSide {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Buy => write!(f, "Buy"),
            Self::Sell => write!(f, "Sell"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
