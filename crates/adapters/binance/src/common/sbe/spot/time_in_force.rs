#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TimeInForce {
    Gtc = 0x0_u8,
    Ioc = 0x1_u8,
    Fok = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for TimeInForce {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Gtc,
            0x1_u8 => Self::Ioc,
            0x2_u8 => Self::Fok,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<TimeInForce> for u8 {
    #[inline]
    fn from(v: TimeInForce) -> Self {
        match v {
            TimeInForce::Gtc => 0x0_u8,
            TimeInForce::Ioc => 0x1_u8,
            TimeInForce::Fok => 0x2_u8,
            TimeInForce::NonRepresentable => 0xfe_u8,
            TimeInForce::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for TimeInForce {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Gtc" => Ok(Self::Gtc),
            "Ioc" => Ok(Self::Ioc),
            "Fok" => Ok(Self::Fok),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for TimeInForce {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Gtc => write!(f, "Gtc"),
            Self::Ioc => write!(f, "Ioc"),
            Self::Fok => write!(f, "Fok"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
