#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum SelfTradePreventionMode {
    None = 0x1_u8,
    ExpireTaker = 0x2_u8,
    ExpireMaker = 0x3_u8,
    ExpireBoth = 0x4_u8,
    Decrement = 0x5_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for SelfTradePreventionMode {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::None,
            0x2_u8 => Self::ExpireTaker,
            0x3_u8 => Self::ExpireMaker,
            0x4_u8 => Self::ExpireBoth,
            0x5_u8 => Self::Decrement,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<SelfTradePreventionMode> for u8 {
    #[inline]
    fn from(v: SelfTradePreventionMode) -> Self {
        match v {
            SelfTradePreventionMode::None => 0x1_u8,
            SelfTradePreventionMode::ExpireTaker => 0x2_u8,
            SelfTradePreventionMode::ExpireMaker => 0x3_u8,
            SelfTradePreventionMode::ExpireBoth => 0x4_u8,
            SelfTradePreventionMode::Decrement => 0x5_u8,
            SelfTradePreventionMode::NonRepresentable => 0xfe_u8,
            SelfTradePreventionMode::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for SelfTradePreventionMode {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "None" => Ok(Self::None),
            "ExpireTaker" => Ok(Self::ExpireTaker),
            "ExpireMaker" => Ok(Self::ExpireMaker),
            "ExpireBoth" => Ok(Self::ExpireBoth),
            "Decrement" => Ok(Self::Decrement),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for SelfTradePreventionMode {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::ExpireTaker => write!(f, "ExpireTaker"),
            Self::ExpireMaker => write!(f, "ExpireMaker"),
            Self::ExpireBoth => write!(f, "ExpireBoth"),
            Self::Decrement => write!(f, "Decrement"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
