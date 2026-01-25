#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PegPriceType {
    PrimaryPeg = 0x1_u8,
    MarketPeg = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for PegPriceType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::PrimaryPeg,
            0x2_u8 => Self::MarketPeg,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<PegPriceType> for u8 {
    #[inline]
    fn from(v: PegPriceType) -> Self {
        match v {
            PegPriceType::PrimaryPeg => 0x1_u8,
            PegPriceType::MarketPeg => 0x2_u8,
            PegPriceType::NonRepresentable => 0xfe_u8,
            PegPriceType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for PegPriceType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "PrimaryPeg" => Ok(Self::PrimaryPeg),
            "MarketPeg" => Ok(Self::MarketPeg),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for PegPriceType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PrimaryPeg => write!(f, "PrimaryPeg"),
            Self::MarketPeg => write!(f, "MarketPeg"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
