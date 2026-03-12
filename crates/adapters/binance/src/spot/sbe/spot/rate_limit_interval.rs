#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RateLimitInterval {
    Second = 0x0_u8,
    Minute = 0x1_u8,
    Hour = 0x2_u8,
    Day = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for RateLimitInterval {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Second,
            0x1_u8 => Self::Minute,
            0x2_u8 => Self::Hour,
            0x3_u8 => Self::Day,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<RateLimitInterval> for u8 {
    #[inline]
    fn from(v: RateLimitInterval) -> Self {
        match v {
            RateLimitInterval::Second => 0x0_u8,
            RateLimitInterval::Minute => 0x1_u8,
            RateLimitInterval::Hour => 0x2_u8,
            RateLimitInterval::Day => 0x3_u8,
            RateLimitInterval::NonRepresentable => 0xfe_u8,
            RateLimitInterval::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for RateLimitInterval {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Second" => Ok(Self::Second),
            "Minute" => Ok(Self::Minute),
            "Hour" => Ok(Self::Hour),
            "Day" => Ok(Self::Day),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for RateLimitInterval {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Second => write!(f, "Second"),
            Self::Minute => write!(f, "Minute"),
            Self::Hour => write!(f, "Hour"),
            Self::Day => write!(f, "Day"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
