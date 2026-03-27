#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ExecutionRuleType {
    PriceRange = 0x1_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ExecutionRuleType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::PriceRange,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ExecutionRuleType> for u8 {
    #[inline]
    fn from(v: ExecutionRuleType) -> Self {
        match v {
            ExecutionRuleType::PriceRange => 0x1_u8,
            ExecutionRuleType::NonRepresentable => 0xfe_u8,
            ExecutionRuleType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ExecutionRuleType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "PriceRange" => Ok(Self::PriceRange),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ExecutionRuleType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PriceRange => write!(f, "PriceRange"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
