#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MatchType {
    AutoMatch = 0x1_u8,
    OnePartyTradeReport = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for MatchType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::AutoMatch,
            0x2_u8 => Self::OnePartyTradeReport,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<MatchType> for u8 {
    #[inline]
    fn from(v: MatchType) -> Self {
        match v {
            MatchType::AutoMatch => 0x1_u8,
            MatchType::OnePartyTradeReport => 0x2_u8,
            MatchType::NonRepresentable => 0xfe_u8,
            MatchType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for MatchType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "AutoMatch" => Ok(Self::AutoMatch),
            "OnePartyTradeReport" => Ok(Self::OnePartyTradeReport),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for MatchType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AutoMatch => write!(f, "AutoMatch"),
            Self::OnePartyTradeReport => write!(f, "OnePartyTradeReport"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
