#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RateLimitType {
    RawRequests = 0x0_u8,
    Connections = 0x1_u8,
    RequestWeight = 0x2_u8,
    Orders = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for RateLimitType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::RawRequests,
            0x1_u8 => Self::Connections,
            0x2_u8 => Self::RequestWeight,
            0x3_u8 => Self::Orders,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<RateLimitType> for u8 {
    #[inline]
    fn from(v: RateLimitType) -> Self {
        match v {
            RateLimitType::RawRequests => 0x0_u8,
            RateLimitType::Connections => 0x1_u8,
            RateLimitType::RequestWeight => 0x2_u8,
            RateLimitType::Orders => 0x3_u8,
            RateLimitType::NonRepresentable => 0xfe_u8,
            RateLimitType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for RateLimitType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "RawRequests" => Ok(Self::RawRequests),
            "Connections" => Ok(Self::Connections),
            "RequestWeight" => Ok(Self::RequestWeight),
            "Orders" => Ok(Self::Orders),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for RateLimitType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RawRequests => write!(f, "RawRequests"),
            Self::Connections => write!(f, "Connections"),
            Self::RequestWeight => write!(f, "RequestWeight"),
            Self::Orders => write!(f, "Orders"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
