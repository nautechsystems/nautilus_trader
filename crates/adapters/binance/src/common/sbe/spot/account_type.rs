#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AccountType {
    Spot = 0x0_u8,
    Unknown = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for AccountType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Spot,
            0x2_u8 => Self::Unknown,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<AccountType> for u8 {
    #[inline]
    fn from(v: AccountType) -> Self {
        match v {
            AccountType::Spot => 0x0_u8,
            AccountType::Unknown => 0x2_u8,
            AccountType::NonRepresentable => 0xfe_u8,
            AccountType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for AccountType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Spot" => Ok(Self::Spot),
            "Unknown" => Ok(Self::Unknown),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for AccountType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Spot => write!(f, "Spot"),
            Self::Unknown => write!(f, "Unknown"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
