#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ExecutionType {
    New = 0x0_u8,
    Canceled = 0x1_u8,
    Replaced = 0x2_u8,
    Rejected = 0x3_u8,
    Trade = 0x4_u8,
    Expired = 0x5_u8,
    TradePrevention = 0x8_u8,
    Unknown = 0xfd_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ExecutionType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::New,
            0x1_u8 => Self::Canceled,
            0x2_u8 => Self::Replaced,
            0x3_u8 => Self::Rejected,
            0x4_u8 => Self::Trade,
            0x5_u8 => Self::Expired,
            0x8_u8 => Self::TradePrevention,
            0xfd_u8 => Self::Unknown,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ExecutionType> for u8 {
    #[inline]
    fn from(v: ExecutionType) -> Self {
        match v {
            ExecutionType::New => 0x0_u8,
            ExecutionType::Canceled => 0x1_u8,
            ExecutionType::Replaced => 0x2_u8,
            ExecutionType::Rejected => 0x3_u8,
            ExecutionType::Trade => 0x4_u8,
            ExecutionType::Expired => 0x5_u8,
            ExecutionType::TradePrevention => 0x8_u8,
            ExecutionType::Unknown => 0xfd_u8,
            ExecutionType::NonRepresentable => 0xfe_u8,
            ExecutionType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ExecutionType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "New" => Ok(Self::New),
            "Canceled" => Ok(Self::Canceled),
            "Replaced" => Ok(Self::Replaced),
            "Rejected" => Ok(Self::Rejected),
            "Trade" => Ok(Self::Trade),
            "Expired" => Ok(Self::Expired),
            "TradePrevention" => Ok(Self::TradePrevention),
            "Unknown" => Ok(Self::Unknown),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ExecutionType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::New => write!(f, "New"),
            Self::Canceled => write!(f, "Canceled"),
            Self::Replaced => write!(f, "Replaced"),
            Self::Rejected => write!(f, "Rejected"),
            Self::Trade => write!(f, "Trade"),
            Self::Expired => write!(f, "Expired"),
            Self::TradePrevention => write!(f, "TradePrevention"),
            Self::Unknown => write!(f, "Unknown"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
