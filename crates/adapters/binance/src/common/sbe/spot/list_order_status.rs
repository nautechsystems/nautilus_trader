#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ListOrderStatus {
    Canceling = 0x0_u8,
    Executing = 0x1_u8,
    AllDone = 0x2_u8,
    Reject = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ListOrderStatus {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Canceling,
            0x1_u8 => Self::Executing,
            0x2_u8 => Self::AllDone,
            0x3_u8 => Self::Reject,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ListOrderStatus> for u8 {
    #[inline]
    fn from(v: ListOrderStatus) -> Self {
        match v {
            ListOrderStatus::Canceling => 0x0_u8,
            ListOrderStatus::Executing => 0x1_u8,
            ListOrderStatus::AllDone => 0x2_u8,
            ListOrderStatus::Reject => 0x3_u8,
            ListOrderStatus::NonRepresentable => 0xfe_u8,
            ListOrderStatus::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ListOrderStatus {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Canceling" => Ok(Self::Canceling),
            "Executing" => Ok(Self::Executing),
            "AllDone" => Ok(Self::AllDone),
            "Reject" => Ok(Self::Reject),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ListOrderStatus {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Canceling => write!(f, "Canceling"),
            Self::Executing => write!(f, "Executing"),
            Self::AllDone => write!(f, "AllDone"),
            Self::Reject => write!(f, "Reject"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
