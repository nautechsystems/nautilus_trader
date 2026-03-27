#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CalculationType {
    External = 0x1_u8,
    ArithmeticMean = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for CalculationType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::External,
            0x2_u8 => Self::ArithmeticMean,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<CalculationType> for u8 {
    #[inline]
    fn from(v: CalculationType) -> Self {
        match v {
            CalculationType::External => 0x1_u8,
            CalculationType::ArithmeticMean => 0x2_u8,
            CalculationType::NonRepresentable => 0xfe_u8,
            CalculationType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for CalculationType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "External" => Ok(Self::External),
            "ArithmeticMean" => Ok(Self::ArithmeticMean),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for CalculationType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::External => write!(f, "External"),
            Self::ArithmeticMean => write!(f, "ArithmeticMean"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
