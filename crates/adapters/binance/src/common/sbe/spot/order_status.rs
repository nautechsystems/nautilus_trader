#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum OrderStatus {
    New = 0x0_u8,
    PartiallyFilled = 0x1_u8,
    Filled = 0x2_u8,
    Canceled = 0x3_u8,
    PendingCancel = 0x4_u8,
    Rejected = 0x5_u8,
    Expired = 0x6_u8,
    ExpiredInMatch = 0x9_u8,
    PendingNew = 0xb_u8,
    Unknown = 0xfd_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for OrderStatus {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::New,
            0x1_u8 => Self::PartiallyFilled,
            0x2_u8 => Self::Filled,
            0x3_u8 => Self::Canceled,
            0x4_u8 => Self::PendingCancel,
            0x5_u8 => Self::Rejected,
            0x6_u8 => Self::Expired,
            0x9_u8 => Self::ExpiredInMatch,
            0xb_u8 => Self::PendingNew,
            0xfd_u8 => Self::Unknown,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<OrderStatus> for u8 {
    #[inline]
    fn from(v: OrderStatus) -> Self {
        match v {
            OrderStatus::New => 0x0_u8,
            OrderStatus::PartiallyFilled => 0x1_u8,
            OrderStatus::Filled => 0x2_u8,
            OrderStatus::Canceled => 0x3_u8,
            OrderStatus::PendingCancel => 0x4_u8,
            OrderStatus::Rejected => 0x5_u8,
            OrderStatus::Expired => 0x6_u8,
            OrderStatus::ExpiredInMatch => 0x9_u8,
            OrderStatus::PendingNew => 0xb_u8,
            OrderStatus::Unknown => 0xfd_u8,
            OrderStatus::NonRepresentable => 0xfe_u8,
            OrderStatus::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for OrderStatus {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "New" => Ok(Self::New),
            "PartiallyFilled" => Ok(Self::PartiallyFilled),
            "Filled" => Ok(Self::Filled),
            "Canceled" => Ok(Self::Canceled),
            "PendingCancel" => Ok(Self::PendingCancel),
            "Rejected" => Ok(Self::Rejected),
            "Expired" => Ok(Self::Expired),
            "ExpiredInMatch" => Ok(Self::ExpiredInMatch),
            "PendingNew" => Ok(Self::PendingNew),
            "Unknown" => Ok(Self::Unknown),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for OrderStatus {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::New => write!(f, "New"),
            Self::PartiallyFilled => write!(f, "PartiallyFilled"),
            Self::Filled => write!(f, "Filled"),
            Self::Canceled => write!(f, "Canceled"),
            Self::PendingCancel => write!(f, "PendingCancel"),
            Self::Rejected => write!(f, "Rejected"),
            Self::Expired => write!(f, "Expired"),
            Self::ExpiredInMatch => write!(f, "ExpiredInMatch"),
            Self::PendingNew => write!(f, "PendingNew"),
            Self::Unknown => write!(f, "Unknown"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
