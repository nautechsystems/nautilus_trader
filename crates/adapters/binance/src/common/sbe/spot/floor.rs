#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Floor {
    Exchange = 0x1_u8,
    Broker = 0x2_u8,
    Sor = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for Floor {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::Exchange,
            0x2_u8 => Self::Broker,
            0x3_u8 => Self::Sor,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<Floor> for u8 {
    #[inline]
    fn from(v: Floor) -> Self {
        match v {
            Floor::Exchange => 0x1_u8,
            Floor::Broker => 0x2_u8,
            Floor::Sor => 0x3_u8,
            Floor::NonRepresentable => 0xfe_u8,
            Floor::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for Floor {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Exchange" => Ok(Self::Exchange),
            "Broker" => Ok(Self::Broker),
            "Sor" => Ok(Self::Sor),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for Floor {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Exchange => write!(f, "Exchange"),
            Self::Broker => write!(f, "Broker"),
            Self::Sor => write!(f, "Sor"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
