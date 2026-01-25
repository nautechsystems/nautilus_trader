#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum SymbolStatus {
    Trading = 0x0_u8,
    EndOfDay = 0x1_u8,
    Halt = 0x2_u8,
    Break = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for SymbolStatus {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Trading,
            0x1_u8 => Self::EndOfDay,
            0x2_u8 => Self::Halt,
            0x3_u8 => Self::Break,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<SymbolStatus> for u8 {
    #[inline]
    fn from(v: SymbolStatus) -> Self {
        match v {
            SymbolStatus::Trading => 0x0_u8,
            SymbolStatus::EndOfDay => 0x1_u8,
            SymbolStatus::Halt => 0x2_u8,
            SymbolStatus::Break => 0x3_u8,
            SymbolStatus::NonRepresentable => 0xfe_u8,
            SymbolStatus::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for SymbolStatus {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Trading" => Ok(Self::Trading),
            "EndOfDay" => Ok(Self::EndOfDay),
            "Halt" => Ok(Self::Halt),
            "Break" => Ok(Self::Break),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for SymbolStatus {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Trading => write!(f, "Trading"),
            Self::EndOfDay => write!(f, "EndOfDay"),
            Self::Halt => write!(f, "Halt"),
            Self::Break => write!(f, "Break"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
