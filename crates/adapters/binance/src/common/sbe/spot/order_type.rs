#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum OrderType {
    Market = 0x0_u8,
    Limit = 0x1_u8,
    StopLoss = 0x2_u8,
    StopLossLimit = 0x3_u8,
    TakeProfit = 0x4_u8,
    TakeProfitLimit = 0x5_u8,
    LimitMaker = 0x6_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for OrderType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Market,
            0x1_u8 => Self::Limit,
            0x2_u8 => Self::StopLoss,
            0x3_u8 => Self::StopLossLimit,
            0x4_u8 => Self::TakeProfit,
            0x5_u8 => Self::TakeProfitLimit,
            0x6_u8 => Self::LimitMaker,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<OrderType> for u8 {
    #[inline]
    fn from(v: OrderType) -> Self {
        match v {
            OrderType::Market => 0x0_u8,
            OrderType::Limit => 0x1_u8,
            OrderType::StopLoss => 0x2_u8,
            OrderType::StopLossLimit => 0x3_u8,
            OrderType::TakeProfit => 0x4_u8,
            OrderType::TakeProfitLimit => 0x5_u8,
            OrderType::LimitMaker => 0x6_u8,
            OrderType::NonRepresentable => 0xfe_u8,
            OrderType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for OrderType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Market" => Ok(Self::Market),
            "Limit" => Ok(Self::Limit),
            "StopLoss" => Ok(Self::StopLoss),
            "StopLossLimit" => Ok(Self::StopLossLimit),
            "TakeProfit" => Ok(Self::TakeProfit),
            "TakeProfitLimit" => Ok(Self::TakeProfitLimit),
            "LimitMaker" => Ok(Self::LimitMaker),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for OrderType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Market => write!(f, "Market"),
            Self::Limit => write!(f, "Limit"),
            Self::StopLoss => write!(f, "StopLoss"),
            Self::StopLossLimit => write!(f, "StopLossLimit"),
            Self::TakeProfit => write!(f, "TakeProfit"),
            Self::TakeProfitLimit => write!(f, "TakeProfitLimit"),
            Self::LimitMaker => write!(f, "LimitMaker"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
