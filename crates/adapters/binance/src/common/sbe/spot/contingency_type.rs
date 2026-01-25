#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ContingencyType {
    Oco = 0x1_u8,
    Oto = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ContingencyType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::Oco,
            0x2_u8 => Self::Oto,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ContingencyType> for u8 {
    #[inline]
    fn from(v: ContingencyType) -> Self {
        match v {
            ContingencyType::Oco => 0x1_u8,
            ContingencyType::Oto => 0x2_u8,
            ContingencyType::NonRepresentable => 0xfe_u8,
            ContingencyType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ContingencyType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Oco" => Ok(Self::Oco),
            "Oto" => Ok(Self::Oto),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ContingencyType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Oco => write!(f, "Oco"),
            Self::Oto => write!(f, "Oto"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
