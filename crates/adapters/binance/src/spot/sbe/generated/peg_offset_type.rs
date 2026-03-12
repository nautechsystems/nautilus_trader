#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PegOffsetType {
    PriceLevel = 0x1_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for PegOffsetType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::PriceLevel,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<PegOffsetType> for u8 {
    #[inline]
    fn from(v: PegOffsetType) -> Self {
        match v {
            PegOffsetType::PriceLevel => 0x1_u8,
            PegOffsetType::NonRepresentable => 0xfe_u8,
            PegOffsetType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for PegOffsetType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "PriceLevel" => Ok(Self::PriceLevel),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for PegOffsetType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PriceLevel => write!(f, "PriceLevel"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
